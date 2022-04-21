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

use async_std::sync::RwLock;
pub use traits::ConsensusApi;

use phase::Phase;
use phaselock_types::{
    data::Stage,
    error::PhaseLockError,
    traits::node_implementation::{NodeImplementation, TypeMap},
};
use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    sync::Arc,
    time::Instant,
};
use tracing::warn;

/// The result used in this crate
pub type Result<T = ()> = std::result::Result<T, PhaseLockError>;

/// Type-safe wrapper around `u64` so we know the thing we're talking about is a view number.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ViewNumber(u64);

/// A reference to the hotstuff implementation.
///
/// This will contain the state of all rounds.
pub struct HotStuff<I: NodeImplementation<N>, const N: usize> {
    /// The phases that are currently loaded in memory
    // TODO: Allow this to be loaded from `Storage`?
    phases: HashMap<ViewNumber, Phase<I, N>>,

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
    pub async fn add_consensus_message<A: ConsensusApi<I, N>>(
        &mut self,
        message: <I as TypeMap<N>>::ConsensusMessage,
        api: &mut A,
    ) -> Result {
        let view_number = ViewNumber(message.view_number());
        let can_insert_view = self.can_insert_view(view_number);
        let phase = match self.phases.entry(view_number) {
            Entry::Occupied(o) => o.into_mut(),
            Entry::Vacant(v) => {
                if can_insert_view {
                    let phase = v.insert(Phase::prepare(
                        view_number,
                        api.is_leader(view_number.0, Stage::Prepare).await,
                    ));
                    self.active_phases.push_back(view_number);
                    debug_assert!(is_sorted(self.active_phases.iter()));
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
            .await
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
        for view in &self.active_phases {
            let phase = self
                .phases
                .get_mut(view)
                .expect("Found a view in `active_phase` but it doesn't exist in `self.phases`");
            if let Stage::Prepare = phase.stage() {
                phase
                    .notify_new_transaction(api, &mut self.transactions)
                    .await?;
            }
        }

        Ok(())
    }

    /// Check to see if we can insert the given view. If the view is earlier than a view we already have, this will return `false`.
    fn can_insert_view(&self, view_number: ViewNumber) -> bool {
        // We can insert a view_number when it is higher than any phase we have
        if let Some(highest) = self.active_phases.back() {
            view_number > *highest
        } else {
            // if we have no phases, always return true
            // TODO(vko): What if we have inactive phases?
            assert!(
                self.inactive_phases.is_empty(),
                "Active phases is empty but inactive phases isn't"
            );
            true
        }
    }
}

/// The state of a [`Transaction`].
#[derive(Clone)]
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
    /// Create a new [`TransationState`]
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
struct TransactionLink {
    /// The time this link was made
    pub timestamp: Instant,
    /// The view number
    pub view_number: ViewNumber,
}

/// Check if the given iterator is sorted. Use internally to make sure some assumptions are correct.
fn is_sorted<'a>(mut iter: impl Iterator<Item = &'a ViewNumber> + 'a) -> bool {
    let mut previous = iter.next().unwrap();
    for item in iter {
        if item <= previous {
            return false;
        }
        previous = item;
    }
    true
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
