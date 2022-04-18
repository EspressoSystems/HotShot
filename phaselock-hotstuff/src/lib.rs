#![warn(
    clippy::all,
    clippy::pedantic,
    rust_2018_idioms,
    // missing_docs, TODO(vko): re-enable this
    // clippy::missing_docs_in_private_items, TODO(vko): re-enable this
    clippy::panic
)]
#![allow(
    clippy::must_use_candidate,
    clippy::module_name_repetitions,
    clippy::missing_errors_doc, // TODO(vko): remove this
)]

mod phase;

mod traits;

pub use traits::ConsensusApi;

use phase::Phase;
use phaselock_types::{
    data::Stage,
    error::PhaseLockError,
    traits::node_implementation::{NodeImplementation, TypeMap},
};
use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    time::Instant,
};
use tracing::warn;

// TODO(vko): Introduce a HotStuff error type?
pub type Result<T = ()> = std::result::Result<T, PhaseLockError>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ViewNumber(u64);

pub struct HotStuff<I: NodeImplementation<N>, const N: usize> {
    phases: HashMap<ViewNumber, Phase<I, N>>,
    /// Active phases, sorted by lowest -> highest
    active_phases: VecDeque<ViewNumber>,
    /// Phases that are in memory but not active any more. These are most likely done.
    /// sorted by lowest -> highest
    inactive_phases: VecDeque<ViewNumber>,
    transactions: Vec<TransactionState<I, N>>,
}

impl<I: NodeImplementation<N>, const N: usize> HotStuff<I, N> {
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
                    let phase = v.insert(if api.is_leader_this_round(view_number.0).await {
                        Phase::propose(view_number)
                    } else {
                        Phase::prepare(view_number)
                    });
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

    pub async fn add_transaction<A: ConsensusApi<I, N>>(
        &mut self,
        transaction: <I as TypeMap<N>>::Transaction,
        api: &mut A,
    ) -> Result {
        self.transactions.push(TransactionState {
            transaction,
            propose: None,
        });
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

struct TransactionState<I: NodeImplementation<N>, const N: usize> {
    transaction: <I as TypeMap<N>>::Transaction,
    propose: Option<TransactionLink>,
}

impl<I: NodeImplementation<N>, const N: usize> TransactionState<I, N> {
    fn is_unclaimed(&self) -> bool {
        self.propose.is_none()
    }
}

#[allow(dead_code)]
struct TransactionLink {
    pub timestamp: Instant,
    pub view_number: ViewNumber,
}

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

trait OptionUtils<K> {
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
