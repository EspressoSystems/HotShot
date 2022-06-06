//! Implementation of the hotstuff paper: <https://arxiv.org/abs/1803.05069>
//!
//! To use this library, you should:
//! - Implement [`ConsensusApi`]
//! - Create a new instance of [`HotStuff`]
//! - whenever a message arrives, call [`HotStuff::add_consensus_message`]
//! - whenever a transaction arrives, call [`HotStuff::add_transaction`]
//!

#![warn(
    clippy::all,
    clippy::pedantic,
    rust_2018_idioms,
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::panic
)]
#![allow(clippy::module_name_repetitions)]

mod phase;
mod traits;
mod utils;

pub use traits::ConsensusApi;

use async_std::sync::RwLock;
use futures::channel::oneshot::Sender;
use phase::ViewState;
use phaselock_types::{
    data::{Stage, ViewNumber},
    error::{FailedToMessageLeaderSnafu, PhaseLockError, StorageSnafu},
    message::{ConsensusMessage, NewView},
    traits::{
        node_implementation::{NodeImplementation, TypeMap},
        storage::Storage,
    },
};
use snafu::ResultExt;
use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    sync::Arc,
    time::Instant,
};
use tracing::{debug, instrument, warn};

/// The result used in this crate
pub type Result<T = ()> = std::result::Result<T, PhaseLockError>;

/// A reference to the hotstuff implementation.
///
/// This will contain the state of all rounds.
#[derive(Debug)]
pub struct HotStuff<I: NodeImplementation<N>, const N: usize> {
    /// The phases that are currently loaded in memory
    // TODO: Allow this to be loaded from `Storage`?
    phases: HashMap<ViewNumber, ViewState<I, N>>,

    /// Active phases, sorted by lowest -> highest
    active_phases: VecDeque<ViewNumber>,

    /// Phases that are in memory but not active any more. These are most likely done and can be unloaded soon.
    /// sorted by lowest -> highest
    inactive_phases: VecDeque<ViewNumber>,

    /// A list of transactions. Transactions are in 1 of 3 states:
    /// - Unclaimed
    /// - Claimed (`propose` is `Some(...)`)
    /// - Rejected (`rejected` is `Some(...)`)
    transactions: Vec<TransactionState<I, N>>,

    /// Listeners to be called when a round ends
    round_finished_listeners: HashMap<ViewNumber, Vec<Sender<RoundFinishedEvent>>>,
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
    Interrupted,
}

impl<I: NodeImplementation<N>, const N: usize> Default for HotStuff<I, N> {
    fn default() -> Self {
        Self {
            phases: HashMap::new(),
            active_phases: VecDeque::new(),
            inactive_phases: VecDeque::new(),
            transactions: Vec::new(),
            round_finished_listeners: HashMap::new(),
        }
    }
}

impl<I: NodeImplementation<N>, const N: usize> HotStuff<I, N> {
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
        // Validate the incoming QC is valid
        if !api.validate_qc_in_message(&message) {
            warn!(?message, "Incoming message does not have a valid QC");
            return Ok(());
        }

        let view_number = message.view_number();
        let can_insert_view = self.can_insert_view(view_number);
        let phase = match self.phases.entry(view_number) {
            Entry::Occupied(o) => o.into_mut(),
            Entry::Vacant(v) => {
                if can_insert_view {
                    let phase = v.insert(ViewState::prepare(
                        view_number,
                        api.is_leader(view_number, Stage::Prepare).await,
                    ));
                    self.active_phases.push_back(view_number);
                    // The new view-number should always be greater than the other entries in `self.active_phases`
                    // validate that here
                    assert!(is_sorted(self.active_phases.iter()));
                    phase
                } else {
                    // Should we throw an error when we're in a unit test?
                    warn!(?view_number, "Could not insert, too old");
                    return Ok(());
                }
            }
        };

        phase
            .add_consensus_message(api, &mut self.transactions, message)
            .await?;
        self.after_update(view_number);
        Ok(())
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
        api: &mut A,
    ) -> Result {
        self.transactions.push(TransactionState::new(transaction));
        // transactions are useful for the `Prepare` phase
        // so notify these phases
        for view_number in self.active_phases.clone() {
            let phase = self
                .phases
                .get_mut(&view_number)
                .expect("Found a view in `active_phase` but it doesn't exist in `self.phases`");
            if let Stage::Prepare = phase.stage() {
                phase
                    .notify_new_transaction(api, &mut self.transactions)
                    .await?;
                self.after_update(view_number);
            }
        }

        Ok(())
    }

    /// Call this when a round should be timed out.
    ///
    /// If the given round is done, this function will not have any effect
    #[instrument]
    pub fn round_timeout(&mut self, view_number: ViewNumber) {
        if self.active_phases.iter().any(|v| v == &view_number) {
            return; // round is not running
        }

        // This view_number should always exist, because it is in our `self.active_phases` list
        self.phases.get_mut(&view_number).unwrap().timeout();
        // `after_update` will properly clean up the internal state
        self.after_update(view_number);
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
        let leader = api.get_leader(view_number, Stage::Prepare).await;
        let is_leader = api.public_key() == &leader;

        // If we don't have this phase in our phases, insert it
        let phase = match self.phases.entry(view_number) {
            Entry::Occupied(o) => o.into_mut(),
            Entry::Vacant(v) => {
                self.active_phases.push_back(view_number);
                if !is_sorted(self.active_phases.iter()) {
                    return utils::err("Internal error; phases aren't properly sorted");
                }
                v.insert(ViewState::prepare(view_number, is_leader))
            }
        };

        let newest_qc = match api.storage().get_newest_qc().await.context(StorageSnafu)? {
            Some(qc) => qc,
            None => return utils::err("No QC in storage"),
        };
        let new_view = ConsensusMessage::NewView(NewView {
            current_view: view_number,
            justify: newest_qc,
        });

        if is_leader {
            phase.add_consensus_message(api, &mut [], new_view).await?;
        } else {
            api.send_direct_message(leader, new_view).await.context(
                FailedToMessageLeaderSnafu {
                    stage: Stage::Prepare,
                },
            )?;
        }
        Ok(())
    }

    /// Register a [`Sender`] that will be notified when the given round ends.
    pub fn register_round_finished_listener(
        &mut self,
        view_number: ViewNumber,
        sender: Sender<RoundFinishedEvent>,
    ) {
        debug!(?view_number, "Attaching listener to round");
        self.round_finished_listeners
            .entry(view_number)
            .or_default()
            .push(sender);
    }

    /// To be called after a round is updated.
    ///
    /// If the round is done, this will:
    /// - notify all listeners in `self.round_finished_listeners`.
    /// - Remove the view number from `active_phases` and append it to `inactive_phases`.
    fn after_update(&mut self, view_number: ViewNumber) {
        // This phase should always exist
        let phase = self.phases.get_mut(&view_number).unwrap();
        if phase.is_done() {
            let listeners = self.round_finished_listeners.remove(&view_number);
            debug!(
                ?view_number,
                "Phase is done, notifying {} listeners",
                listeners.as_ref().map(Vec::len).unwrap_or_default()
            );
            if let Some(listeners) = listeners {
                let event = RoundFinishedEvent {
                    view_number,
                    state: if phase.was_timed_out() {
                        RoundFinishedEventState::Interrupted
                    } else {
                        RoundFinishedEventState::Success
                    },
                };
                for listener in listeners {
                    let _ = listener.send(event.clone());
                }
            }
            self.active_phases.retain(|p| p != &view_number);
            match self.inactive_phases.binary_search(&view_number) {
                Ok(_) => { /* view number is already in an inactive phase */ }
                Err(idx) => self.inactive_phases.insert(idx, view_number),
            }
        }
    }

    /// Check to see if we can insert the given view. If the view is earlier than a view we already have, this will return `false`.
    fn can_insert_view(&self, view_number: ViewNumber) -> bool {
        // We can insert a view_number when it is higher than any phase we have
        if let Some(highest) = self.active_phases.back() {
            view_number > *highest
        } else {
            // if we have no phases, always return true
            if self.inactive_phases.iter().any(|p| p >= &view_number) {
                tracing::error!(
                    "Trying to insert view number {:?} but our inactive_phases is {:?}",
                    view_number,
                    self.inactive_phases
                );
            }
            true
        }
    }
}

/// The state of a [`Transaction`].
#[derive(Clone, Debug)]
struct TransactionState<I: NodeImplementation<N>, const N: usize> {
    /// The transaction
    transaction: <I as TypeMap<N>>::Transaction,
    /// If this is `Some`, the transaction was proposed in the given round
    // TODO(vko): see if we can remove this `Arc<RwLock<..>>` and use mutable references instead
    propose: Arc<RwLock<Option<TransactionLink>>>,
    /// If this is `Some`, the transaction was rejected on the given timestamp
    rejected: Arc<RwLock<Option<Instant>>>,
}

impl<I: NodeImplementation<N>, const N: usize> TransactionState<I, N> {
    /// Create a new [`TransactionState`]
    fn new(transaction: <I as TypeMap<N>>::Transaction) -> TransactionState<I, N> {
        Self {
            transaction,
            propose: Arc::default(),
            rejected: Arc::default(),
        }
    }

    /// returns `true` if this transaction has not been proposed or rejected yet.
    async fn is_unclaimed(&self) -> bool {
        self.propose.read().await.is_none() && self.rejected.read().await.is_none()
    }
}

/// A link to a view number at a given time
// TODO: These fields are not used. In the future we can use this for:
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

/// A utility function that will return `PhaseLockError::ItemNotFound` if a value is `None`
trait OptionUtils<K> {
    /// Return `ItemNotFound` with the given hash if `self` is `None`.
    fn or_not_found<Ref: AsRef<[u8]>>(self, hash: Ref) -> Result<K>;
}

impl<K> OptionUtils<K> for Option<K> {
    fn or_not_found<Ref: AsRef<[u8]>>(self, hash: Ref) -> Result<K> {
        match self {
            Some(v) => Ok(v),
            None => Err(PhaseLockError::ItemNotFound {
                type_name: std::any::type_name::<Ref>(),
                hash: hash.as_ref().to_vec(),
            }),
        }
    }
}
