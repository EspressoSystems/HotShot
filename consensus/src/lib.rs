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

mod leader;
mod next_leader;
mod replica;
mod traits;
mod utils;

pub use leader::Leader;
pub use next_leader::NextLeader;
pub use replica::Replica;
pub use traits::ConsensusApi;
pub use utils::{SendToTasks, TransactionHashMap, TransactionStorage, View, ViewInner, ViewQueue};

use commit::Commitment;
use hotshot_types::{
    data::{Leaf, QuorumCertificate, ViewNumber},
    error::HotShotError,
    traits::node_implementation::NodeImplementation,
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};
use tracing::{error, warn};
use utils::{Result, Terminator};

/// A reference to the consensus algorithm
///
/// This will contain the state of all rounds.
#[derive(Debug)]
pub struct Consensus<I: NodeImplementation> {
    /// The phases that are currently loaded in memory
    // TODO(https://github.com/EspressoSystems/hotshot/issues/153): Allow this to be loaded from `Storage`?
    pub state_map: BTreeMap<ViewNumber, View<I::StateType>>,

    /// cur_view from pseudocode
    pub cur_view: ViewNumber,

    /// last view had a successful decide event
    pub last_decided_view: ViewNumber,

    /// A list of undecided transactions
    pub transactions: TransactionStorage<I>,

    /// Map of leaf hash -> leaf
    /// - contains undecided leaves
    /// - includes the MOST RECENT decided leaf
    pub saved_leaves: HashMap<Commitment<Leaf<I::StateType>>, Leaf<I::StateType>>,

    /// The `locked_qc` view number
    pub locked_view: ViewNumber,

    /// the highqc per spec
    pub high_qc: QuorumCertificate<I::StateType>,
}

impl<I: NodeImplementation> Consensus<I> {
    /// increment the current view
    /// NOTE may need to do gc here
    pub fn increment_view(&mut self) -> ViewNumber {
        self.cur_view += 1;
        self.cur_view
    }

    /// gather information from the parent chain of leafs
    /// # Errors
    /// If the leaf or its ancestors are not found in storage
    pub fn visit_leaf_ancestors<F>(
        &self,
        start_from: ViewNumber,
        terminator: Terminator,
        ok_when_finished: bool,
        mut f: F,
    ) -> Result<()>
    where
        F: FnMut(&Leaf<I::StateType>) -> bool,
    {
        let mut next_leaf = if let Some(view) = self.state_map.get(&start_from) {
            *view
                .get_leaf_commitment()
                .ok_or_else(|| HotShotError::InvalidState {
                    context: format!(
                        "Visited failed view {:?} leaf. Expected successfuil leaf",
                        start_from
                    ),
                })?
        } else {
            return Err(HotShotError::InvalidState {
                context: format!("View {:?} leaf does not exist in state map ", start_from),
            });
        };

        while let Some(leaf) = self.saved_leaves.get(&next_leaf) {
            if let Terminator::Exclusive(stop_before) = terminator {
                if stop_before == leaf.view_number {
                    if ok_when_finished {
                        return Ok(());
                    }
                    break;
                }
            }
            next_leaf = leaf.parent_commitment;
            if !f(leaf) {
                return Ok(());
            }
            if let Terminator::Inclusive(stop_after) = terminator {
                if stop_after == leaf.view_number {
                    if ok_when_finished {
                        return Ok(());
                    }
                    break;
                }
            }
        }
        Err(HotShotError::LeafNotFound {})
    }

    /// garbage collects based on state change
    /// right now, this removes from both the `saved_leaves`
    /// and `state_map` fields of `Consensus`
    pub async fn collect_garbage(
        &mut self,
        old_anchor_view: ViewNumber,
        new_anchor_view: ViewNumber,
    ) {
        // state check
        let anchor_entry = self
            .state_map
            .iter()
            .next()
            .expect("INCONSISTENT STATE: anchor leaf not in state map!");
        if *anchor_entry.0 != old_anchor_view {
            error!(
                "Something about GC has failed. Older leaf exists than the previous anchor leaf."
            );
        }
        // perform gc
        self.state_map
            .range(old_anchor_view..new_anchor_view)
            .filter_map(|(_view_number, view)| view.get_leaf_commitment())
            .for_each(|leaf| {
                let _removed = self.saved_leaves.remove(leaf);
            });
        self.state_map = self.state_map.split_off(&new_anchor_view);
    }

    /// return a clone of the internal storage of unclaimed transactions
    #[must_use]
    pub fn get_transactions(&self) -> TransactionStorage<I> {
        self.transactions.clone()
    }

    /// Gets the last decided state
    /// # Panics
    /// if the last decided view's state does not exist in the state map
    /// this should never happen.
    #[must_use]
    pub fn get_decided_leaf(&self) -> Leaf<I::StateType> {
        let decided_view_num = self.last_decided_view;
        let view = self.state_map.get(&decided_view_num).unwrap();
        let leaf = view
            .get_leaf_commitment()
            .expect("Decided state not found! Consensus internally inconsistent");
        self.saved_leaves.get(leaf).unwrap().clone()
    }
}

impl<I: NodeImplementation> Default for Consensus<I> {
    fn default() -> Self {
        Self {
            transactions: Arc::default(),
            cur_view: ViewNumber::genesis(),
            last_decided_view: ViewNumber::genesis(),
            state_map: BTreeMap::default(),
            saved_leaves: HashMap::default(),
            locked_view: ViewNumber::genesis(),
            high_qc: QuorumCertificate::genesis(),
        }
    }
}
