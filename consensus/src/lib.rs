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

use flume::{Receiver, RecvError, Sender};
pub use traits::ConsensusApi;

use async_std::{
    sync::{Arc, RwLock, RwLockUpgradableReadGuard},
    task::{sleep, spawn, JoinHandle},
};
use futures::{future::join, select, FutureExt};
use hotshot_types::{
    data::{
        create_verify_hash, BlockHash, Leaf, LeafHash, QuorumCertificate, TransactionHash,
        ViewNumber,
    },
    error::{FailedToMessageLeaderSnafu, HotShotError, RoundTimedoutState, StorageSnafu},
    message::{ConsensusMessage, Proposal, TimedOut, Vote},
    traits::{
        node_implementation::{NodeImplementation, TypeMap},
        signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey},
        storage::Storage,
        BlockContents, State,
    },
};
use snafu::ResultExt;
use std::{
    collections::{
        btree_map::{Entry, OccupiedEntry},
        BTreeMap, BTreeSet, HashMap, HashSet, VecDeque,
    },
    ops::Bound::{Excluded, Included},
    time::{Duration, Instant},
};
use tracing::{debug, error, instrument, warn};

#[derive(Debug)]
pub enum ViewInner<I: NodeImplementation<N>, const N: usize> {
    Future {
        sender_chan: Sender<ConsensusMessage<I::Block, I::State, N>>,
        receiver_chan: Receiver<ConsensusMessage<I::Block, I::State, N>>,
    },
    Leaf {
        leaf: LeafHash<N>,
    },
    Failed,
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

    /// cur_view from pseudocode
    cur_view: ViewNumber,

    /// last view had a successful decide event
    last_decided_view: ViewNumber,

    // /// Listeners to be called when a round ends
    // /// TODO we can probably nuke this soon
    // new_round_finished_listeners: Vec<Sender<RoundFinishedEvent>>,
    /// A list of transactions
    /// TODO we should flush out the logic here more
    pub transactions: Arc<RwLock<Vec<<I as TypeMap<N>>::Transaction>>>,

    pub undecided_leaves: HashMap<LeafHash<N>, Leaf<I::Block, I::State, N>>,

    locked_view: ViewNumber,
    pub high_qc: QuorumCertificate<N>,
}

#[derive(Debug, Clone)]
pub struct Replica<A: ConsensusApi<I, N>, I: NodeImplementation<N>, const N: usize> {
    /// Reference to consensus. Replica will require a write lock on this.
    pub consensus: Arc<RwLock<Consensus<I, N>>>,
    /// channel for accepting leader proposals and timeouts messages
    pub proposal_collection_chan: Receiver<ConsensusMessage<I::Block, I::State, N>>,
    /// view number this view is executing in
    pub cur_view: ViewNumber,
    /// genericQC from the pseudocode
    pub high_qc: QuorumCertificate<N>,
    /// hotshot consensus api
    pub api: A,
}

impl<A: ConsensusApi<I, N>, I: NodeImplementation<N>, const N: usize> Replica<A, I, N> {
    /// run one view of replica
    /// returns the high_qc
    pub async fn run_view(self) -> QuorumCertificate<N> {
        let consensus = self.consensus.upgradable_read().await;
        let view_leader_key = self.api.get_leader(self.cur_view).await;

        let leaf = loop {
            let msg = self.proposal_collection_chan.recv_async().await;
            match msg {
                Ok(msg) => {
                    // stale/newer view messages should never reach this specific task's receive channel
                    if msg.view_number() != self.cur_view {
                        continue;
                    }
                    match msg {
                        ConsensusMessage::Proposal(p) => {
                            if !view_leader_key.validate(&p.signature, &p.leaf.hash().to_vec()) {
                                continue;
                            }
                            let justify_qc = p.leaf.justify_qc;
                            let parent = if let Some(parent) =
                                consensus.undecided_leaves.get(&p.leaf.parent)
                            {
                                parent
                            } else {
                                continue;
                            };
                            if justify_qc.view_number != parent.view_number
                                || !self.api.validate_qc(&justify_qc, parent.view_number)
                            {
                                continue;
                            }
                            let leaf = if let Ok(state) = parent.state.append(&p.leaf.deltas) {
                                Leaf::new(
                                    state,
                                    p.leaf.deltas,
                                    p.leaf.parent,
                                    justify_qc,
                                    self.cur_view,
                                )
                            } else {
                                continue;
                            };
                            let leaf_hash = leaf.hash();
                            let signature = self.api.sign_vote(&leaf_hash, self.cur_view);

                            let vote = ConsensusMessage::<I::Block, I::State, N>::Vote(Vote {
                                block_hash: leaf.deltas.hash(),
                                justify_qc: leaf.justify_qc.clone(),
                                signature,
                                leaf_hash,
                                current_view: self.cur_view,
                            });

                            // send out vote

                            let next_leader = self.api.get_leader(self.cur_view + 1).await;

                            let _result = self.api.send_direct_message(next_leader, vote);

                            break leaf;
                        }
                        ConsensusMessage::NextViewInterrupt(_view_number) => {
                            let next_leader = self.api.get_leader(self.cur_view + 1).await;

                            let timed_out_msg = ConsensusMessage::TimedOut(TimedOut {
                                current_view: self.cur_view,
                                justify: self.high_qc.clone(),
                            });

                            // send timedout message to the next leader
                            let _result = self
                                .api
                                .send_direct_message(next_leader, timed_out_msg)
                                .await;

                            // exits from entire function
                            return self.high_qc;
                        }
                        ConsensusMessage::Vote(_) | ConsensusMessage::TimedOut(_) => {
                            // should only be for leader, never
                            error!("useful error goes here");
                            continue;
                        }
                    }
                }
                Err(_) => {
                    error!("useful error goes here");
                    return self.high_qc;
                }
            }
        };

        // promote lock here

        let mut consensus = RwLockUpgradableReadGuard::upgrade(consensus).await;
        consensus.state_map.insert(
            self.cur_view,
            View {
                view_inner: ViewInner::Leaf { leaf: leaf.hash() },
            },
        );
        let high_qc = leaf.justify_qc.clone();
        consensus.undecided_leaves.insert(leaf.hash(), leaf);
        let mut new_anchor_view = consensus.last_decided_view;
        let mut new_locked_view = consensus.locked_view;
        let mut last_view_number_visited = self.cur_view;
        let mut current_chain_length = 0usize;
        let mut new_decide_reached = false;
        let mut blocks = Vec::new();
        let mut states = Vec::new();
        let mut qcs = Vec::new();
        let last_decided_view = consensus.last_decided_view;
        let _outcome = consensus.visit_leaf_ancestors(
            self.cur_view,
            Terminator::Exclusive(last_decided_view),
            |leaf| {
                if !new_decide_reached && last_view_number_visited == leaf.view_number + 1 {
                    current_chain_length += 1;
                    if current_chain_length == 2 {
                        new_locked_view = leaf.view_number;
                    }
                    if current_chain_length == 3 {
                        new_anchor_view = leaf.view_number;
                        new_decide_reached = true;
                        blocks.push(leaf.deltas.clone());
                        states.push(leaf.state.clone());
                        qcs.push(leaf.justify_qc.clone());
                    }
                    last_view_number_visited = leaf.view_number;
                } else if new_decide_reached {
                    // collecting chain elements for the decide
                    blocks.push(leaf.deltas.clone());
                    states.push(leaf.state.clone());
                    qcs.push(leaf.justify_qc.clone());
                } else {
                    // nothing more to do here... we don't have a new chain extension
                    return false;
                }
                true
            },
        );
        let mut included_txns = Vec::new();
        blocks.iter().for_each(|block| {
            let mut txns = block.contained_transactions();
            included_txns.append(&mut txns);
        });
        let included_txns_set: HashSet<_> = included_txns.into_iter().collect();
        {
            let mut txns = consensus.transactions.write().await;
            *txns = txns
                .drain(..)
                .filter(|txn| !included_txns_set.contains(&I::Block::hash_transaction(txn)))
                .collect();
        }
        let decide_sent = self
            .api
            .send_decide(consensus.last_decided_view, blocks, states, qcs);
        let old_anchor_view = consensus.last_decided_view;
        consensus
            .collect_garbage(old_anchor_view, new_anchor_view)
            .await;
        decide_sent.await;
        high_qc
    }
}

#[derive(Debug, Clone)]
pub struct Leader<A: ConsensusApi<I, N>, I: NodeImplementation<N>, const N: usize> {
    /// Reference to consensus. Leader will require a read lock on this.
    pub consensus: Arc<RwLock<Consensus<I, N>>>,
    pub high_qc: QuorumCertificate<N>,
    pub cur_view: ViewNumber,
    pub transactions: Arc<RwLock<Vec<<I as TypeMap<N>>::Transaction>>>,
    pub api: A,
}

impl<A: ConsensusApi<I, N>, I: NodeImplementation<N>, const N: usize> Leader<A, I, N> {
    /// TODO have this consume self instead of taking a mutable reference. We never use self again.
    pub async fn run_view(self) -> QuorumCertificate<N> {
        let parent_view_number = self.high_qc.view_number;
        let consensus = self.consensus.read().await;
        let mut reached_decided = false;

        let parent_leaf = if let Some(parent_view) = consensus.state_map.get(&parent_view_number) {
            match &parent_view.view_inner {
                ViewInner::Leaf { leaf } => {
                    if let Some(leaf) = consensus.undecided_leaves.get(leaf) {
                        if leaf.view_number == consensus.last_decided_view {
                            reached_decided = true;
                        }
                        leaf
                    } else {
                        error!("error goes here");
                        return self.high_qc;
                    }
                }
                // can happen if future api is whacked
                ViewInner::Future { .. } | ViewInner::Failed => {
                    error!("error goes here");
                    return self.high_qc;
                }
            }
        } else {
            error!("error goes here");
            return self.high_qc;
        };

        let original_parent_hash = parent_leaf.hash();
        let starting_state = &parent_leaf.state;

        let mut previous_used_txns_vec = parent_leaf.deltas.contained_transactions();

        let next_parent_hash = original_parent_hash;

        while !reached_decided {
            if let Some(next_parent_leaf) = consensus.undecided_leaves.get(&next_parent_hash) {
                let mut next_parent_txns = next_parent_leaf.deltas.contained_transactions();
                previous_used_txns_vec.append(&mut next_parent_txns);
            } else {
                // TODO do some sort of sanity check on the view number that it matches decided
                break;
            }
        }

        let previous_used_txns = previous_used_txns_vec
            .into_iter()
            .collect::<HashSet<TransactionHash<N>>>();

        let txns = self.transactions.read().await;
        let unclaimed_txns: Vec<_> = txns
            .iter()
            .filter(|txn| !previous_used_txns.contains(&I::Block::hash_transaction(*txn)))
            .collect();

        let mut block = starting_state.next_block();
        unclaimed_txns.iter().for_each(|txn| {
            let new_block_check = block.add_transaction_raw(txn);
            if let Ok(new_block) = new_block_check {
                if starting_state.validate_block(&new_block) {
                    block = new_block;
                }
            }
        });

        if let Ok(new_state) = starting_state.append(&block) {
            let leaf = Leaf {
                view_number: self.cur_view,
                justify_qc: self.high_qc.clone(),
                parent: original_parent_hash,
                deltas: block,
                state: new_state,
            };
            let signature = self.api.sign_proposal(&leaf.hash(), self.cur_view);
            let message = ConsensusMessage::Proposal(Proposal { leaf, signature });
            // TODO add erroring stuff
            let _ = self.api.send_broadcast_message(message).await;
        }

        self.high_qc.clone()
    }
}

#[derive(Debug, Clone)]
pub struct NextLeader<A: ConsensusApi<I, N>, I: NodeImplementation<N>, const N: usize> {
    /// generic_qc before starting this
    pub generic_qc: QuorumCertificate<N>,
    pub vote_collection_chan: Receiver<ConsensusMessage<I::Block, I::State, N>>,
    pub cur_view: ViewNumber,
    pub api: A,
}

/// type alias for a less ugly mapping of signatures
pub type Signatures = BTreeMap<EncodedPublicKey, EncodedSignature>;

impl<A: ConsensusApi<I, N>, I: NodeImplementation<N>, const N: usize> NextLeader<A, I, N> {
    /// run one view of the next leader task
    pub async fn run_view(self) -> QuorumCertificate<N> {
        let mut qcs = HashSet::<QuorumCertificate<N>>::new();
        qcs.insert(self.generic_qc.clone());

        let mut vote_outcomes: HashMap<LeafHash<N>, (BlockHash<N>, Signatures)> = HashMap::new();
        // NOTE will need to refactor this during VRF integration
        let threshold = self.api.threshold();

        while let Ok(msg) = self.vote_collection_chan.recv_async().await {
            if msg.view_number() != self.cur_view {
                continue;
            }
            match msg {
                ConsensusMessage::TimedOut(t) => {
                    qcs.insert(t.justify);
                }
                ConsensusMessage::Vote(vote) => {
                    qcs.insert(vote.justify_qc);

                    match vote_outcomes.entry(vote.leaf_hash) {
                        std::collections::hash_map::Entry::Occupied(mut o) => {
                            let (bh, map) = o.get_mut();
                            if *bh != vote.block_hash {
                                error!("Mismatch between blockhash in received votes. This is probably an error without byzantine nodes.");
                            }
                            map.insert(vote.signature.0.clone(), vote.signature.1.clone());
                        }
                        std::collections::hash_map::Entry::Vacant(location) => {
                            let mut map = BTreeMap::new();
                            map.insert(vote.signature.0, vote.signature.1);
                            location.insert((vote.block_hash, map));
                        }
                    }

                    let (block_hash, map) = vote_outcomes.get(&vote.leaf_hash).unwrap();

                    if map.len() >= threshold.into() {
                        // NOTE this is slow, shouldn't check all the signatures EVERY time
                        let result = self.api.get_valid_signatures(
                            map.clone(),
                            create_verify_hash(&vote.leaf_hash, self.cur_view),
                        );
                        if let Ok(valid_signatures) = result {
                            // construct QC
                            let qc = QuorumCertificate {
                                block_hash: *block_hash,
                                leaf_hash: vote.leaf_hash,
                                view_number: self.cur_view,
                                signatures: valid_signatures,
                                genesis: false,
                            };
                            return qc;
                        } else {
                            continue;
                        }
                    }
                }
                ConsensusMessage::NextViewInterrupt(_view_number) => {
                    break;
                }
                ConsensusMessage::Proposal(p) => {
                    error!("useful error goes here");
                }
            }
        }

        qcs.into_iter().max_by_key(|qc| qc.view_number).unwrap()
    }
}

#[derive(Debug, Clone)]
pub struct ViewIterator<'a, I: NodeImplementation<N>, const N: usize> {
    consensus: &'a Consensus<I, N>,
    terminator: Terminator,
    is_error: bool,
    leaf: &'a Leaf<I::Block, I::State, N>,
}

impl<I: NodeImplementation<N>, const N: usize> Consensus<I, N> {
    /// increment the current view
    /// NOTE may need to do gc here
    pub fn increment_view(&mut self) -> ViewNumber {
        self.cur_view += 1;
        self.cur_view
    }

    /// gather information from the parent chain of leafs
    pub fn visit_leaf_ancestors<F>(
        &self,
        start_from: ViewNumber,
        terminator: Terminator,
        mut f: F,
    ) -> Result<()>
    where
        F: FnMut(&Leaf<I::Block, I::State, N>) -> bool,
    {
        let mut next_leaf = if let Some(view) = self.state_map.get(&start_from) {
            if let ViewInner::Leaf { leaf } = view.view_inner {
                leaf
            } else {
                return Err(HotShotError::InvalidState {
                    context: String::default(),
                });
            }
        } else {
            return Err(HotShotError::InvalidState {
                context: String::default(),
            });
        };
        loop {
            if let Some(leaf) = self.undecided_leaves.get(&next_leaf) {
                if let Terminator::Exclusive(stop_before) = terminator {
                    if stop_before == leaf.view_number {
                        return Ok(());
                    }
                }
                next_leaf = leaf.parent;
                if !f(leaf) {
                    return Ok(());
                }
                if let Terminator::Inclusive(stop_after) = terminator {
                    if stop_after == leaf.view_number {
                        return Ok(());
                    }
                }
            } else {
                return Err(HotShotError::ItemNotFound {
                    type_name: "Leaf",
                    hash: next_leaf.to_vec(),
                });
            }
        }
    }

    /// garbage collects based on state change
    pub async fn collect_garbage(
        &mut self,
        old_anchor_view: ViewNumber,
        new_anchor_view: ViewNumber,
    ) {
        if let Some(entry) = self.state_map.iter().next() {
            if *entry.0 != old_anchor_view {
                error!("useful error message here");
            }
        }
        self.state_map
            .range((Included(&old_anchor_view), Excluded(&new_anchor_view)))
            .filter_map(|(view_number, view)| {
                if let ViewInner::Leaf { leaf } = &view.view_inner {
                    Some((view_number, leaf))
                } else {
                    None
                }
            })
            .for_each(|(_view_number, leaf)| {
                let _removed = self.undecided_leaves.remove(leaf);
            });
    }

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
            undecided_leaves: nll_todo(),
            locked_view: nll_todo(),
            high_qc: nll_todo(),
        }
    }
}

impl<I: NodeImplementation<N>, const N: usize> Consensus<I, N> {
    /// return a clone of the internal storage of unclaimed transactions
    pub fn get_transactions(&self) -> Arc<RwLock<Vec<<I as TypeMap<N>>::Transaction>>> {
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

#[derive(Copy, Clone, Debug)]
pub enum Terminator {
    Exclusive(ViewNumber),
    Inclusive(ViewNumber),
}
