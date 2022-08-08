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
    message::{ConsensusMessage, TimedOut, Proposal, Vote},
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
use tracing::{debug, instrument, warn};

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
        todo!()
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
    generic_qc: QuorumCertificate<N>,
    // msg_channel: Receiver<ConsensusMessage<>>,
}


#[derive(Debug, Clone)]
pub struct Replica<I: NodeImplementation<N>, const N: usize> {
    pub locked_qc: QuorumCertificate<N>,
    pub generic_qc: QuorumCertificate<N>,
    pub proposal_collection_chan: Receiver<ConsensusMessage<I::State, I::Block, N>>,
}

impl<I: NodeImplementation<N>, const N: usize> Replica<I, N> {
    /// run one view of replica
    pub async fn run_view(&mut self) -> JoinHandle<()> {
        todo!()
    }
}

#[derive(Debug, Clone)]
pub struct Leader<const N: usize> {
    /// next leader calculated this
    pub high_qc: QuorumCertificate<N>,
    /// replica calculated this
    pub generic_qc: QuorumCertificate<N>,
}

impl<const N: usize> Leader<N> {
    pub async fn run_view(&mut self) -> JoinHandle<()> {
        todo!()
    }
}

#[derive(Debug, Clone)]
pub struct NextLeader<I: NodeImplementation<N>, const N: usize> {
    pub generic_qc: QuorumCertificate<N>,
    pub vote_collection_chan: Receiver<ConsensusMessage<I::Block, I::State, N>>,
}

impl<I: NodeImplementation<N>, const N: usize> NextLeader<I, N> {
    pub async fn run_view(&mut self) -> JoinHandle<()> {
        todo!()
    }
}

impl<I: NodeImplementation<N>, const N: usize> Consensus<I, N> {
    pub fn create_leader_state(&self, msgs: ConsensusMessage<I::Block, I::State, N>) -> Leader<N>{
        todo!()
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
    pub async fn collect_garbage(&mut self) {
    }


    /// Returns a channel that may be used to send received proposals
    /// to the Replica task.
    /// NOTE: requires write access to `Consensus` because may
    /// insert into `self.state_map` if the view has not been constructed NOTE: requires write
    /// access to `Consensus` because may
    /// insert into `self.state_map` if the view has not been constructed
    pub fn get_future_view_sender(
        &mut self,
        msg_view_number: ViewNumber,
    ) -> Option<Sender<ConsensusMessage<I::Block, I::State, N>>> {
        if msg_view_number < self.cur_view {
            return None;
        }

        let view = self.state_map.entry(msg_view_number).or_insert(View::new());
        if let ViewInner::Future { sender_chan, .. } = &view.view_inner {
            Some(sender_chan.clone())
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
            cur_view: todo!(),
            last_decided_view: todo!(),
            state_map: todo!(),
            in_progress_leaves: todo!(),
            undecided_leaves: todo!(),
            locked_qc: todo!(),
            generic_qc: todo!(),
        }
    }
}

impl<I: NodeImplementation<N>, const N: usize> Consensus<I, N> {
    /// Add a consensus message to the hotstuff implementation.
    ///
    /// # Errors
    ///
    /// Will return:
    /// - Any error that a stage can encounter (usually when it's in an invalid state)
    /// - Any networking error
    /// - Any storage error
    /// - Any error that the [`ConsensusApi`] methods can return
    #[allow(clippy::missing_panics_doc)] // Clippy thinks we can panic but logically we should not be able to
    pub async fn add_consensus_message<A: ConsensusApi<I, N>>(
        &mut self,
        message: <I as TypeMap<N>>::ConsensusMessage,
        api: &mut A,
        _sender: I::SignatureKey,
    ) -> Result {
        // // Validate the incoming QC is valid
        // if !api.validate_qc_in_message(&message) {
        //     warn!(?message, "Incoming message does not have a valid QC");
        //     return Ok(());
        // }
        //
        // let view_number = message.view_number();
        // let can_insert_view = self.can_insert_view(view_number);
        // let phase = match self.view_cache.entry(view_number) {
        //     Entry::Occupied(o) => o.into_mut(),
        //     Entry::Vacant(v) => {
        //         if can_insert_view {
        //             // NOTE this is inserting into the vacant entry in self.phases
        //             let phase = v.insert(ViewState::prepare(
        //                 view_number,
        //                 api.is_leader(view_number).await,
        //             ));
        //             self.active_phases.push_back(view_number);
        //             // The new view-number should always be greater than the other entries in `self.active_phases`
        //             // validate that here
        //             assert!(is_sorted(self.active_phases.iter()));
        //             phase
        //         } else {
        //             // Should we throw an error when we're in a unit test?
        //             warn!(?view_number, "Could not insert, too old");
        //             return Ok(());
        //         }
        //     }
        // };
        //
        // phase
        //     .add_consensus_message(api, &mut self.transactions, message)
        //     .await?;
        // self.after_update(view_number);
        // Ok(())
        todo!()
    }

    /// Add a transaction to the hotstuff implementation.
    ///
    /// # Errors
    ///
    /// Will return:
    /// - Any error that a stage can encounter (usually when it's in an invalid state)
    /// - Any networking error
    /// - Any storage error
    /// - Any error that the [`ConsensusApi`] methods can return
    pub async fn add_transaction<A: ConsensusApi<I, N>>(
        &mut self,
        transaction: <I as TypeMap<N>>::Transaction,
        _api: &mut A,
    ) -> Result {
        let txn_hash = <I::Block as BlockContents<N>>::hash_transaction(&transaction);
        // The API contract requires the hash to be unique
        // so we can assume entry == incoming txn
        // even if eq not satisfied
        // so insert is an idempotent operation
        self.transactions.write().await.insert(txn_hash, transaction);

        Ok(())
    }

    /// Call this when a round should be timed out.
    ///
    /// If the given round is done, this function will not have any effect
    #[instrument]
    pub fn round_timeout(&mut self, view_number: ViewNumber) {
        let msg = ConsensusMessage::<I::Block, I::State, N>::NextViewInterrupt(view_number);
        // TODO send to next leader (if applicable)

        // TODO send to replica (if applicable)

    }

    /// Send out a [`NextView`] message to the leader of the given round.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The phase already exists
    /// - INTERNAL: Phases are not properly sorted
    /// - The storage layer returned an error
    /// - There were no QCs in the storage
    /// - A broadcast message could not be send
    pub async fn next_view<A: ConsensusApi<I, N>>(
        &mut self,
        view_number: ViewNumber,
        api: &mut A,
    ) -> Result {
        todo!()
        // let leader = api.get_leader(view_number).await;
        // let is_leader = api.public_key() == &leader;
        //
        // // If we don't have this phase in our phases, insert it
        // let phase = match self.view_cache.entry(view_number) {
        //     Entry::Occupied(o) => o.into_mut(),
        //     Entry::Vacant(v) => {
        //         self.active_phases.push_back(view_number);
        //         if !is_sorted(self.active_phases.iter()) {
        //             return utils::err("Internal error; phases aren't properly sorted");
        //         }
        //         v.insert(ViewState::prepare(view_number, is_leader))
        //     }
        // };
        //
        // let newest_qc = match api.storage().get_newest_qc().await.context(StorageSnafu)? {
        //     Some(qc) => qc,
        //     None => return utils::err("No QC in storage"),
        // };
        // let new_view = ConsensusMessage::NewView(NewView {
        //     current_view: view_number,
        //     justify: newest_qc,
        // });
        //
        // if is_leader {
        //     phase.add_consensus_message(api, &mut [], new_view).await?;
        // } else {
        //     api.send_direct_message(leader, new_view).await.context(
        //         FailedToMessageLeaderSnafu { },
        //     )?;
        // }
        // Ok(())
    }

    /// Register a [`Sender`] that will be notified when the given round ends.
    pub fn register_round_finished_listener(
        &mut self,
        view_number: ViewNumber,
        sender: Sender<RoundFinishedEvent>,
    ) {
        todo!()
    }

    /// To be called after a round is updated.
    ///
    /// If the round is done, this will:
    /// - notify all listeners in `self.round_finished_listeners`.
    /// - Remove the view number from `active_phases` and append it to `inactive_phases`.
    fn after_update(&mut self, view_number: ViewNumber) {
        todo!()
        // // This phase should always exist
        // let phase = self.view_cache.get_mut(&view_number).unwrap();
        // if phase.is_done() {
        //     let listeners = self.round_finished_listeners.remove(&view_number);
        //     debug!(
        //         ?view_number,
        //         "Phase is done, notifying {} listeners",
        //         listeners.as_ref().map(Vec::len).unwrap_or_default()
        //     );
        //     if let Some(listeners) = listeners {
        //         let event = RoundFinishedEvent {
        //             view_number,
        //             state: if let Some(reason) = phase.get_timedout_reason() {
        //                 RoundFinishedEventState::Interrupted(reason)
        //             } else {
        //                 RoundFinishedEventState::Success
        //             },
        //         };
        //         for listener in listeners {
        //             let _ = listener.send(event.clone());
        //         }
        //     }
        //     self.active_phases.retain(|p| p != &view_number);
        //     match self.inactive_phases.binary_search(&view_number) {
        //         Ok(_) => { /* view number is already in an inactive phase */ }
        //         Err(idx) => self.inactive_phases.insert(idx, view_number),
        //     }
        // }
    }

    /// Check to see if we can insert the given view. If the view is earlier than a view we already have, this will return `false`.
    fn can_insert_view(&self, view_number: ViewNumber) -> bool {
        todo!()
        // // We can insert a view_number when it is higher than any phase we have
        // if let Some(highest) = self.active_phases.back() {
        //     view_number > *highest
        // } else {
        //     // if we have no active phases, check if all `inactive_phases` are less than `view_number`
        //     self.inactive_phases.iter().all(|p| p < &view_number)
        // }
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
