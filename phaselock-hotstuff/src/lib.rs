//! Hotstuff implementation for phaselock
//!
//! This crate is based on the "HotStuff: BFT Consensus in the Lens of BlockChain" paper.
//! For a copy of this paper, please visit <https://arxiv.org/abs/1803.05069>.
//!
//! This crate is implemented as a state machine: [`ConsensusState`]. Every node in the network will need a single instance of this.
//!
//! This state can receive messages, in the form of [`ConsensusMessage`].
//!
//! Additionally the state needs a way to load data, store data, and send messages. This can be achieved by implementing [`ConsensusApi`].
//!
//! There are some points where the implementation differs from the algorithm. These are listed below:
//! - After the leader gets out of `Prepare` phase, it goes into `CollectTransactions` mode. This is so we can be sure that there are transactions in the queue.
//!
//! Some questions that need to be answered:
//! - What if a leader is waiting for transactions but transactions never show up, and a new replica joins?
//!
//! Some forms of attacks that need to be tested:
//! - What if a node sends `NewView(u64::MAX_VALUE)` when a leader is in `Prepare` phase?
//!

mod state;

use std::num::NonZeroU64;
use std::sync::Arc;

use phaselock_types::data::Stage;
pub use phaselock_types::error::PhaseLockError;
use phaselock_types::{PrivKey, PubKey};

use phaselock_types::event::{Event, EventType};
use phaselock_types::message::{Commit, Decide, NewView, PreCommit, Prepare, Vote};
use phaselock_types::traits::network::NetworkError;
use phaselock_types::traits::node_implementation::{NodeImplementation, TypeMap};
use state::State;

use async_trait::async_trait;

#[derive(Debug)]
pub(crate) enum ConsensusMessage<'a, I: NodeImplementation<N>, const N: usize> {
    /// Signals start of a new view
    NewView(NewView<N>),
    /// Contains the prepare qc from the leader
    Prepare(Prepare<I::Block, I::State, N>),
    /// A nodes vote on the prepare stage
    PrepareVote(Vote<N>),
    /// Contains the precommit qc from the leader
    PreCommit(PreCommit<N>),
    /// A node's vote on the precommit stage
    PreCommitVote(Vote<N>),
    /// Contains the commit qc from the leader
    Commit(Commit<N>),
    /// A node's vote on the commit stage
    CommitVote(Vote<N>),
    /// Contains the decide qc from the leader
    Decide(Decide<N>),
    /// The network has no more incoming messages to process and is idle.
    Idle {
        transactions: &'a mut Vec<<I as TypeMap<N>>::Transaction>,
    },
}

type Result<T = ()> = std::result::Result<T, PhaseLockError>;

#[async_trait]
pub trait ConsensusApi<I: NodeImplementation<N>, const N: usize>: Sync {
    /// Total number of nodes in the network. Also known as `n`.
    fn total_nodes(&self) -> u64;

    /// The amount of nodes that are required to reach a decision. Also known as `n - f`.
    fn threshold(&self) -> NonZeroU64;

    /// Get a reference to the storage implementation
    fn storage(&self) -> &I::Storage;

    /// Returns `true` if this node is leader this round.
    ///
    /// TODO: This should probably be implemented in the HotStuff layer.
    async fn is_leader_this_round(&self, view_number: u64) -> bool;

    async fn send_broadcast_message(
        &self,
        message: <I as TypeMap<N>>::ConsensusMessage,
    ) -> std::result::Result<(), NetworkError>;

    /// Notify the system of an event within `phaselock-hotstuff`.
    async fn send_event(&self, event: Event<I::Block, I::State>);

    /// Get a reference to the public key.
    fn public_key(&self) -> &PubKey;

    /// Get a reference to the private key.
    fn private_key(&self) -> &PrivKey;

    /// The `phaselock-hotstuff` implementation will call this method, with the series of blocks and states
    /// that are being committed, whenever a commit action takes place.
    ///
    /// The provided states and blocks are guaranteed to be in ascending order of age (newest to
    /// oldest).
    async fn notify(&mut self, blocks: Vec<I::Block>, states: Vec<I::State>);

    // Utility functions

    /// sends a proposal event down the channel
    async fn send_propose(&self, view_number: u64, block: &I::Block) {
        self.send_event(Event {
            view_number,
            stage: Stage::Prepare,
            event: EventType::Propose {
                block: Arc::new(block.clone()),
            },
        })
        .await;
    }

    /// sends a decide event down the channel
    async fn send_decide(&self, view_number: u64, blocks: &[(I::Block, I::State)]) {
        self.send_event(Event {
            view_number,
            stage: Stage::Prepare,
            event: EventType::Decide {
                block: Arc::new(blocks.iter().map(|x| x.0.clone()).collect()),
                state: Arc::new(blocks.iter().map(|x| x.1.clone()).collect()),
            },
        })
        .await;
    }
}

trait OptionUtils<K> {
    fn or_not_found(self, hash: impl AsRef<[u8]>) -> Result<K>;
}

impl<K> OptionUtils<K> for Option<K> {
    fn or_not_found(self, hash: impl AsRef<[u8]>) -> Result<K> {
        match self {
            Some(v) => Ok(v),
            None => Err(PhaseLockError::ItemNotFound {
                hash: hash.as_ref().to_vec(),
            }),
        }
    }
}

#[derive(Default)]
pub struct ConsensusState<I: NodeImplementation<N>, const N: usize> {
    state: State<I, N>,
    queued_transactions: Vec<<I as TypeMap<N>>::Transaction>,
}

impl<I: NodeImplementation<N>, const N: usize> ConsensusState<I, N> {
    /// Notify the ConsensusState of an incoming message.
    ///
    /// Returns the current phase of the state after this message is processed.
    pub async fn incoming_message(
        &mut self,
        message: <I as TypeMap<N>>::ConsensusMessage,
        api: &mut impl ConsensusApi<I, N>,
    ) -> Result<Phase> {
        let msg = match message {
            phaselock_types::message::ConsensusMessage::NewView(view) => {
                Some(ConsensusMessage::NewView(view))
            }
            phaselock_types::message::ConsensusMessage::Prepare(prepare) => {
                Some(ConsensusMessage::Prepare(prepare))
            }
            phaselock_types::message::ConsensusMessage::PrepareVote(vote) => {
                Some(ConsensusMessage::PrepareVote(vote))
            }
            phaselock_types::message::ConsensusMessage::PreCommit(pre_commit) => {
                Some(ConsensusMessage::PreCommit(pre_commit))
            }
            phaselock_types::message::ConsensusMessage::PreCommitVote(vote) => {
                Some(ConsensusMessage::PreCommitVote(vote))
            }
            phaselock_types::message::ConsensusMessage::Commit(commit) => {
                Some(ConsensusMessage::Commit(commit))
            }
            phaselock_types::message::ConsensusMessage::CommitVote(vote) => {
                Some(ConsensusMessage::CommitVote(vote))
            }
            phaselock_types::message::ConsensusMessage::Decide(decide) => {
                Some(ConsensusMessage::Decide(decide))
            }
            phaselock_types::message::ConsensusMessage::SubmitTransaction(transaction) => {
                self.queued_transactions.push(transaction);
                None
            }
        };
        if let Some(message) = msg {
            self.state.step(message, api).await?;
        }
        Ok(Phase::new(&self.state))
    }

    /// `idle` should be called when there are no incoming messages to be parsed.
    ///
    /// Because the networking implementation probably doesn't know how many messages are incoming, it is recommended to wait a set timeout before calling this function.
    ///
    /// Returns the current phase of the state after this message is processed.
    pub async fn idle(&mut self, api: &mut impl ConsensusApi<I, N>) -> Result<Phase> {
        self.state
            .step(
                ConsensusMessage::Idle {
                    transactions: &mut self.queued_transactions,
                },
                api,
            )
            .await?;
        Ok(Phase::new(&self.state))
    }
}

/// A public representation of the internal phase of `phaselock-hotstuff`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Phase {
    BeforeRound,
    Leader(Stage),
    Replica(Stage),
    AfterRound,
}

impl Phase {
    pub(crate) fn new<I: NodeImplementation<N>, const N: usize>(state: &State<I, N>) -> Self {
        match state {
            State::BeforeRound { .. } => Self::BeforeRound,
            State::Leader(state::LeaderPhase::Prepare(_)) => Self::Leader(Stage::Prepare),
            State::Leader(state::LeaderPhase::CollectTransactions(_)) => {
                // `CollectTransactions` is part of our `Prepare` stage
                Self::Leader(Stage::Prepare)
            }
            State::Leader(state::LeaderPhase::PreCommit(_)) => Self::Leader(Stage::PreCommit),
            State::Leader(state::LeaderPhase::Commit(_)) => Self::Leader(Stage::Commit),
            State::AfterRound { .. } => Self::AfterRound,
        }
    }
}
