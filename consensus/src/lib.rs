//! The consensus layer for hotshot. This currently implements the hotstuff paper: <https://arxiv.org/abs/1803.05069>
//!
//! To use this library, you should:
//! - Implement [`ConsensusApi`]
//! - Create a new instance of [`Consensus`]
//! - whenever a message arrives, call [`Consensus::add_consensus_message`]
//! - whenever a transaction arrives, call [`Consensus::add_transaction`]
//!

#![warn(
    clippy::all,
    clippy::pedantic,
    rust_2018_idioms,
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::panic
)]
#![allow(clippy::module_name_repetitions, clippy::unused_async)]

// mod phase;
mod traits;
mod utils;
// pub mod message_processing;

use hotshot_utils::hack::nll_todo;

use flume::{Receiver, Sender};
pub use traits::ConsensusApi;

use async_std::{
    sync::{Arc, RwLock},
    task::{sleep, spawn, JoinHandle},
};
use futures::{future::join, select, FutureExt};
use hotshot_types::{
    data::{Leaf, LeafHash, QuorumCertificate, TransactionHash, ViewNumber},
    error::{FailedToMessageLeaderSnafu, HotShotError, RoundTimedoutState, StorageSnafu},
    message::{ConsensusMessage, Proposal, TimedOut, Vote},
    traits::{
        node_implementation::{NodeImplementation, TypeMap},
        storage::Storage,
        BlockContents,
    },
};
use snafu::ResultExt;
use std::{
    collections::{btree_map::Entry, BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    time::{Duration, Instant},
};
use tracing::{debug, instrument, warn, error};

#[derive(Debug)]
pub enum ViewInner<I: NodeImplementation<N>, const N: usize> {
    Future {
        sender_chan: Sender<ConsensusMessage<I::Block, I::State, N>>,
        receiver_chan: Receiver<ConsensusMessage<I::Block, I::State, N>>,
    },
    Undecided {
        leaf: Leaf<I::Block, I::State, N>,
    },
    Decided {
        leaf: Leaf<I::Block, I::State, N>,
    },
    Failed,
}

impl<I: NodeImplementation<N>, const N: usize> View<I, N> {
    pub fn transition(&mut self) {
        nll_todo()
    }
}

impl<I: NodeImplementation<N>, const N: usize> View<I, N> {
    pub fn new() -> Self {
        let (sender_chan, receiver_chan) = flume::unbounded();
        Self {
            view_inner: ViewInner::Future {
                sender_chan,
                receiver_chan,
            },
        }
    }
}

/// This exists so we can perform state transitions mutably
#[derive(Debug)]
pub(crate) struct View<I: NodeImplementation<N>, const N: usize> {
    view_inner: ViewInner<I, N>,
}

/// The result used in this crate
pub type Result<T = ()> = std::result::Result<T, HotShotError>;

/// A reference to the consensus algorithm
///
/// This will contain the state of all rounds.
#[derive(Debug)]
pub struct Consensus<I: NodeImplementation<N>, const N: usize> {
    /// The phases that are currently loaded in memory
    // TODO(https://github.com/EspressoSystems/hotshot/issues/153): Allow this to be loaded from `Storage`?
    state_map: BTreeMap<ViewNumber, View<I, N>>,

    /// leaves that have been seen
    in_progress_leaves: HashSet<Leaf<I::Block, I::State, N>>,

    /// cur_view from pseudocode
    cur_view: ViewNumber,

    /// last view had a successful decide event
    last_decided_view: ViewNumber,

    // /// Listeners to be called when a round ends
    // /// TODO we can probably nuke this soon
    // new_round_finished_listeners: Vec<Sender<RoundFinishedEvent>>,
    /// A list of transactions
    /// TODO we should flush out the logic here more
    transactions: Arc<RwLock<HashMap<TransactionHash<N>, <I as TypeMap<N>>::Transaction>>>,

    undecided_leaves: HashMap<LeafHash<N>, Leaf<I::Block, I::State, N>>,

    locked_qc: QuorumCertificate<N>,
    pub high_qc: QuorumCertificate<N>,
    // msg_channel: Receiver<ConsensusMessage<>>,
}

#[derive(Debug, Clone)]
pub struct Replica<I: NodeImplementation<N>, const N: usize> {
    /// Reference to consensus. Replica will require a write lock on this.
    pub consensus: Arc<RwLock<Consensus<I, N>>>,
    /// channel for accepting leader proposals and timeouts messages
    pub proposal_collection_chan: Receiver<ConsensusMessage<I::Block, I::State, N>>,
}

impl<I: NodeImplementation<N>, const N: usize> Replica<I, N> {
    /// run one view of replica
    /// returns the high_qc
    pub async fn run_view<A: ConsensusApi<I, N>>(&mut self, api: &A) -> JoinHandle<QuorumCertificate<N>> {
        nll_todo()
    }
}

#[derive(Debug, Clone)]
pub struct Leader<I: NodeImplementation<N>, const N: usize> {
    /// Reference to consensus. Leader will require a read lock on this.
    pub consensus: Arc<RwLock<Consensus<I, N>>>,
}

impl<I: NodeImplementation<N>, const N: usize> Leader<I, N> {
    pub async fn run_view<A: ConsensusApi<I, N>>(&mut self, api: &A) -> JoinHandle<QuorumCertificate<N>> {
        nll_todo()
    }
}

#[derive(Debug, Clone)]
pub struct NextLeader<I: NodeImplementation<N>, const N: usize> {
    /// generic_qc before starting this
    pub generic_qc: QuorumCertificate<N>,
    pub vote_collection_chan: Receiver<ConsensusMessage<I::Block, I::State, N>>,
    pub cur_view: ViewNumber
}

impl<I: NodeImplementation<N>, const N: usize> NextLeader<I, N> {
    pub async fn run_view<A: ConsensusApi<I, N>>(&mut self, api: &A) -> JoinHandle<QuorumCertificate<N>> {
        let vote_count = 0;
        let mut qcs = HashSet::<QuorumCertificate<N>>::new();
        qcs.insert(self.generic_qc.clone());
        let threshold = api.threshold();


        while let Ok(msg) = self.vote_collection_chan.recv_async().await {
            let msg_view_number = msg.view_number();
            match msg {
                ConsensusMessage::TimedOut(t) => {
                    if t.current_view == self.cur_view {
                        qcs.insert(t.justify);
                    }

                    // append to qc set
                },
                ConsensusMessage::Vote(vote) => {
                    // add qc to qcs
                    // check validatiy of vote
                    // increment vote count
                    //
                    // if vote_count >= threshold, break
                },
                ConsensusMessage::NextViewInterrupt(view_number) => {
                    if self.cur_view == view_number {
                        break;
                    }
                },
                ConsensusMessage::Proposal(p) => {
                    error!("useful error goes here");
                },
            }
        }

        let high_qc = qcs.into_iter().max_by_key(|qc| qc.view_number).unwrap();

        // calculate the high_qc and return

        nll_todo()
    }
}

impl<I: NodeImplementation<N>, const N: usize> Consensus<I, N> {
    pub fn create_leader_state(
        &self,
        msgs: ConsensusMessage<I::Block, I::State, N>,
    ) -> Leader<I, N> {
        nll_todo()
        // Leader {
        //     self.generic_qc,
        // }
    }

    /// increment the current view
    /// NOTE may need to do gc here
    pub fn increment_view(&mut self) -> ViewNumber {
        self.cur_view += 1;
        self.cur_view
    }

    /// garbage collects based on state change
    pub async fn collect_garbage(&mut self) {}

    /// Returns channels that may be used to send/receive received proposals
    /// to the Replica task.
    /// NOTE: requires write access to `Consensus` because may
    /// insert into `self.state_map` if the view has not been constructed NOTE: requires write
    /// access to `Consensus` because may
    /// insert into `self.state_map` if the view has not been constructed
    pub fn get_future_view_pair(
        &mut self,
        msg_view_number: ViewNumber,
    ) -> Option<(
        Sender<ConsensusMessage<I::Block, I::State, N>>,
        Receiver<ConsensusMessage<I::Block, I::State, N>>,
    )> {
        if msg_view_number < self.cur_view {
            return None;
        }

        let view = self.state_map.entry(msg_view_number).or_insert(View::new());
        if let ViewInner::Future {
            sender_chan,
            receiver_chan,
        } = &view.view_inner
        {
            Some((sender_chan.clone(), receiver_chan.clone()))
        } else {
            None
        }
    }

    // pub fn spawn_network_handler() -> Sender<ConsensusMessage<I::Block, I::State, N>> {
    //     let (send_network_handler, recv_network_handler) = flume::unbounded();
    //     spawn(async move {
    //         // TODO use this somehow
    //         // TODO shutdown
    //         drop(recv_network_handler);
    //     });
    //     send_network_handler
    // }
}

/// A struct containing information about a finished round.
#[derive(Debug, Clone)]
pub struct RoundFinishedEvent {
    /// The round that finished
    pub view_number: ViewNumber,
    /// The state that this round finished as
    pub state: RoundFinishedEventState,
}

/// Contains the possible outcomes of a round.
///
/// The only successfully outcome is `Success`. More variants may be added to this enum but they will all be error states.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum RoundFinishedEventState {
    /// The round finished successfully
    Success,
    /// The round got interrupted
    Interrupted(RoundTimedoutState),
}

impl<I: NodeImplementation<N>, const N: usize> Default for Consensus<I, N> {
    fn default() -> Self {
        Self {
            transactions: Arc::default(),
            cur_view: nll_todo(),
            last_decided_view: nll_todo(),
            state_map: nll_todo(),
            in_progress_leaves: nll_todo(),
            undecided_leaves: nll_todo(),
            locked_qc: nll_todo(),
            high_qc: nll_todo(),
        }
    }
}

impl<I: NodeImplementation<N>, const N: usize> Consensus<I, N> {
    /// return a clone of the internal storage of unclaimed transactions
    pub fn get_transactions(
        &self,
    ) -> Arc<RwLock<HashMap<TransactionHash<N>, <I as TypeMap<N>>::Transaction>>> {
        self.transactions.clone()
    }
}

/// The state of a [`Transaction`].
#[derive(Debug)]
struct TransactionState<I: NodeImplementation<N>, const N: usize> {
    /// The transaction
    transaction: <I as TypeMap<N>>::Transaction,
    /// If this is `Some`, the transaction was proposed in the given round
    propose: Option<TransactionLink>,
    /// If this is `Some`, the transaction was rejected on the given timestamp
    rejected: Option<Instant>,
}

impl<I: NodeImplementation<N>, const N: usize> TransactionState<I, N> {
    /// Create a new [`TransactionState`]
    fn new(transaction: <I as TypeMap<N>>::Transaction) -> TransactionState<I, N> {
        Self {
            transaction,
            propose: None,
            rejected: None,
        }
    }

    /// returns `true` if this transaction has not been proposed or rejected yet.
    async fn is_unclaimed(&self) -> bool {
        self.propose.is_none() && self.rejected.is_none()
    }
}

/// A link to a view number at a given time
// TODO(https://github.com/EspressoSystems/hotshot/issues/257): These fields are not used. In the future we can use this for:
// - debugging
// - persistent storage
// - cleaning up old transactions out of memory
#[allow(dead_code)]
#[derive(Debug)]
struct TransactionLink {
    /// The time this link was made
    pub timestamp: Instant,
    /// The view number
    pub view_number: ViewNumber,
}

/// Check if the given iterator is sorted. Use internally to make sure some assumptions are correct.
fn is_sorted<'a>(mut iter: impl Iterator<Item = &'a ViewNumber> + 'a) -> bool {
    match iter.next() {
        // An empty list is always sorted
        None => true,

        Some(mut previous) => {
            // iterate through 1..n view numbers
            for item in iter {
                if item <= previous {
                    return false;
                }
                previous = item;
            }
            true
        }
    }
}

/// A utility function that will return `HotShotError::ItemNotFound` if a value is `None`
trait OptionUtils<K> {
    /// Return `ItemNotFound` with the given hash if `self` is `None`.
    fn or_not_found<Ref: AsRef<[u8]>>(self, hash: Ref) -> Result<K>;
}

impl<K> OptionUtils<K> for Option<K> {
    fn or_not_found<Ref: AsRef<[u8]>>(self, hash: Ref) -> Result<K> {
        match self {
            Some(v) => Ok(v),
            None => Err(HotShotError::ItemNotFound {
                type_name: std::any::type_name::<Ref>(),
                hash: hash.as_ref().to_vec(),
            }),
        }
    }
}
