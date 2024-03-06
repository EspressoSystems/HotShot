use hotshot::{traits::TestableNodeImplementation, HotShotError};

use async_trait::async_trait;
use hotshot_task::task::{Task, TaskState, TestTaskState};
use hotshot_types::{
    data::{Leaf, VidDisperse},
    error::RoundTimedoutState,
    event::{Event, EventType, LeafChain},
    simple_certificate::QuorumCertificate,
    traits::node_implementation::{ConsensusTime, NodeType},
    vid::VidCommitment,
};
use snafu::Snafu;
use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    sync::Arc,
};
use tracing::error;

use crate::test_runner::{HotShotTaskCompleted, Node};
/// convenience type alias for state and block
pub type StateAndBlock<S, B> = (Vec<S>, Vec<B>);

use super::GlobalTestEvent;

/// the status of a view
#[derive(Debug, Clone)]
pub enum ViewStatus<TYPES: NodeType> {
    /// success
    Ok,
    /// failure
    Failed,
    /// safety violation
    Err(OverallSafetyTaskErr<TYPES>),
    /// in progress
    InProgress,
}

/// possible errors
#[derive(Snafu, Debug, Clone)]
pub enum OverallSafetyTaskErr<TYPES: NodeType> {
    /// inconsistent txn nums
    InconsistentTxnsNum {
        /// node idx -> number transactions
        map: HashMap<u64, usize>,
    },
    /// too many failed  views
    TooManyFailures {
        /// vec of failed views
        failed_views: HashSet<TYPES::Time>,
    },
    /// not enough decides
    NotEnoughDecides {
        /// expected number of decides
        expected: usize,
        /// acutal number of decides
        got: usize,
    },
    /// mismatched leaves for a view
    MismatchedLeaf,
    /// mismatched states for a view
    InconsistentStates,
    /// mismatched blocks for a view
    InconsistentBlocks,
}

/// Data availability task state
pub struct OverallSafetyTask<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> {
    /// handles
    pub handles: Vec<Node<TYPES, I>>,
    /// ctx
    pub ctx: RoundCtx<TYPES>,
    /// configure properties
    pub properties: OverallSafetyPropertiesDescription,
}

#[async_trait]
impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> TaskState
    for OverallSafetyTask<TYPES, I>
{
    type Event = GlobalTestEvent;

    type Output = HotShotTaskCompleted;

    async fn handle_event(event: Self::Event, task: &mut Task<Self>) -> Option<Self::Output> {
        match event {
            GlobalTestEvent::ShutDown => {
                tracing::error!("Shutting down SafetyTask");
                let state = task.state_mut();
                let OverallSafetyPropertiesDescription {
                    check_leaf: _,
                    check_block: _,
                    num_failed_views: num_failed_rounds_total,
                    num_successful_views,
                    threshold_calculator: _,
                    transaction_threshold: _,
                }: OverallSafetyPropertiesDescription = state.properties.clone();

                let num_incomplete_views = state.ctx.round_results.len()
                    - state.ctx.successful_views.len()
                    - state.ctx.failed_views.len();

                if state.ctx.successful_views.len() < num_successful_views {
                    return Some(HotShotTaskCompleted::Error(Box::new(
                        OverallSafetyTaskErr::<TYPES>::NotEnoughDecides {
                            got: state.ctx.successful_views.len(),
                            expected: num_successful_views,
                        },
                    )));
                }

                if state.ctx.failed_views.len() + num_incomplete_views >= num_failed_rounds_total {
                    return Some(HotShotTaskCompleted::Error(Box::new(
                        OverallSafetyTaskErr::<TYPES>::TooManyFailures {
                            failed_views: state.ctx.failed_views.clone(),
                        },
                    )));
                }
                Some(HotShotTaskCompleted::ShutDown)
            }
        }
    }

    fn should_shutdown(&self, _event: &Self::Event) -> bool {
        false
    }
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> TestTaskState
    for OverallSafetyTask<TYPES, I>
{
    type Message = Event<TYPES>;

    type Output = HotShotTaskCompleted;

    type State = Self;

    async fn handle_message(
        message: Self::Message,
        idx: usize,
        task: &mut hotshot_task::task::TestTask<Self::State, Self>,
    ) -> Option<Self::Output> {
        let OverallSafetyPropertiesDescription {
            check_leaf,
            check_block,
            num_failed_views,
            num_successful_views,
            threshold_calculator,
            transaction_threshold,
        }: OverallSafetyPropertiesDescription = task.state().properties.clone();
        let Event { view_number, event } = message;
        let key = match event {
            EventType::Error { error } => {
                task.state_mut()
                    .ctx
                    .insert_error_to_context(view_number, idx, error);
                None
            }
            EventType::Decide {
                leaf_chain,
                qc,
                block_size: maybe_block_size,
            } => {
                // Skip the genesis leaf.
                if leaf_chain.last().unwrap().0.get_view_number() == TYPES::Time::genesis() {
                    return None;
                }
                let paired_up = (leaf_chain.to_vec(), (*qc).clone());
                match task.state_mut().ctx.round_results.entry(view_number) {
                    Entry::Occupied(mut o) => {
                        o.get_mut()
                            .insert_into_result(idx, paired_up, maybe_block_size)
                    }
                    Entry::Vacant(v) => {
                        let mut round_result = RoundResult::default();
                        let key = round_result.insert_into_result(idx, paired_up, maybe_block_size);
                        v.insert(round_result);
                        key
                    }
                }
            }
            EventType::ReplicaViewTimeout { view_number } => {
                let error = Arc::new(HotShotError::<TYPES>::ViewTimeoutError {
                    view_number,
                    state: RoundTimedoutState::TestCollectRoundEventsTimedOut,
                });
                task.state_mut()
                    .ctx
                    .insert_error_to_context(view_number, idx, error);
                None
            }
            _ => return None,
        };

        // update view count
        let threshold =
            (threshold_calculator)(task.state().handles.len(), task.state().handles.len());

        let len = task.state().handles.len();
        let view = task
            .state_mut()
            .ctx
            .round_results
            .get_mut(&view_number)
            .unwrap();
        if let Some(key) = key {
            view.update_status(
                threshold,
                len,
                &key.0,
                check_leaf,
                check_block,
                transaction_threshold,
            );
            match view.status.clone() {
                ViewStatus::Ok => {
                    task.state_mut().ctx.successful_views.insert(view_number);
                    if task.state_mut().ctx.successful_views.len() >= num_successful_views {
                        task.send_event(GlobalTestEvent::ShutDown).await;
                        return Some(HotShotTaskCompleted::ShutDown);
                    }
                    return None;
                }
                ViewStatus::Failed => {
                    task.state_mut().ctx.failed_views.insert(view_number);
                    if task.state_mut().ctx.failed_views.len() > num_failed_views {
                        task.send_event(GlobalTestEvent::ShutDown).await;
                        return Some(HotShotTaskCompleted::Error(Box::new(
                            OverallSafetyTaskErr::<TYPES>::TooManyFailures {
                                failed_views: task.state_mut().ctx.failed_views.clone(),
                            },
                        )));
                    }
                    return None;
                }
                ViewStatus::Err(e) => {
                    task.send_event(GlobalTestEvent::ShutDown).await;
                    return Some(HotShotTaskCompleted::Error(Box::new(e)));
                }
                ViewStatus::InProgress => {
                    return None;
                }
            }
        } else if view.check_if_failed(threshold, len) {
            view.status = ViewStatus::Failed;
            task.state_mut().ctx.failed_views.insert(view_number);
            if task.state_mut().ctx.failed_views.len() > num_failed_views {
                task.send_event(GlobalTestEvent::ShutDown).await;
                return Some(HotShotTaskCompleted::Error(Box::new(
                    OverallSafetyTaskErr::<TYPES>::TooManyFailures {
                        failed_views: task.state_mut().ctx.failed_views.clone(),
                    },
                )));
            }
            return None;
        }
        None
    }
}

/// Result of running a round of consensus
#[derive(Debug)]
pub struct RoundResult<TYPES: NodeType> {
    /// Transactions that were submitted
    // pub txns: Vec<TYPES::Transaction>,

    /// Nodes that committed this round
    /// id -> (leaf, qc)
    success_nodes: HashMap<u64, (LeafChain<TYPES>, QuorumCertificate<TYPES>)>,

    /// Nodes that failed to commit this round
    pub failed_nodes: HashMap<u64, Arc<HotShotError<TYPES>>>,

    /// whether or not the round succeeded (for a custom defn of succeeded)
    pub status: ViewStatus<TYPES>,

    /// NOTE: technically a map is not needed
    /// left one anyway for ease of viewing
    /// leaf -> # entries decided on that leaf
    pub leaf_map: HashMap<Leaf<TYPES>, usize>,

    /// block -> # entries decided on that block
    pub block_map: HashMap<VidCommitment, usize>,

    /// node idx -> number transactions
    pub num_txns_map: HashMap<u64, usize>,
}

impl<TYPES: NodeType> Default for RoundResult<TYPES> {
    fn default() -> Self {
        Self {
            success_nodes: HashMap::default(),
            failed_nodes: HashMap::default(),
            leaf_map: HashMap::default(),
            block_map: HashMap::default(),
            num_txns_map: HashMap::default(),
            status: ViewStatus::InProgress,
        }
    }
}

/// smh my head I shouldn't need to implement this
/// Rust doesn't realize I doesn't need to implement default
impl<TYPES: NodeType> Default for RoundCtx<TYPES> {
    fn default() -> Self {
        Self {
            round_results: HashMap::default(),
            failed_views: HashSet::default(),
            successful_views: HashSet::default(),
        }
    }
}

/// context for a round
/// TODO eventually we want these to just be futures
/// that we poll when things are event driven
/// this context will be passed around
#[derive(Debug)]
pub struct RoundCtx<TYPES: NodeType> {
    /// results from previous rounds
    /// view number -> round result
    pub round_results: HashMap<TYPES::Time, RoundResult<TYPES>>,
    /// during the run view refactor
    pub failed_views: HashSet<TYPES::Time>,
    /// successful views
    pub successful_views: HashSet<TYPES::Time>,
}

impl<TYPES: NodeType> RoundCtx<TYPES> {
    /// inserts an error into the context
    pub fn insert_error_to_context(
        &mut self,
        view_number: TYPES::Time,
        idx: usize,
        error: Arc<HotShotError<TYPES>>,
    ) {
        match self.round_results.entry(view_number) {
            Entry::Occupied(mut o) => match o.get_mut().failed_nodes.entry(idx as u64) {
                Entry::Occupied(mut o2) => {
                    *o2.get_mut() = error;
                }
                Entry::Vacant(v) => {
                    v.insert(error);
                }
            },
            Entry::Vacant(v) => {
                let mut round_result = RoundResult::default();
                round_result.failed_nodes.insert(idx as u64, error);
                v.insert(round_result);
            }
        }
    }
}

impl<TYPES: NodeType> RoundResult<TYPES> {
    /// insert into round result
    #[allow(clippy::unit_arg)]
    pub fn insert_into_result(
        &mut self,
        idx: usize,
        result: (LeafChain<TYPES>, QuorumCertificate<TYPES>),
        maybe_block_size: Option<u64>,
    ) -> Option<(Leaf<TYPES>, Option<VidDisperse<TYPES>>)> {
        self.success_nodes.insert(idx as u64, result.clone());

        let maybe_leaf: Option<(Leaf<TYPES>, _)> = result.0.into_iter().last();
        if let Some((leaf, _)) = maybe_leaf.clone() {
            match self.leaf_map.entry(leaf.clone()) {
                std::collections::hash_map::Entry::Occupied(mut o) => {
                    *o.get_mut() += 1;
                }
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(1);
                }
            }

            let payload_commitment = leaf.get_payload_commitment();

            match self.block_map.entry(payload_commitment) {
                std::collections::hash_map::Entry::Occupied(mut o) => {
                    *o.get_mut() += 1;
                }
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(1);
                }
            }

            if let Some(num_txns) = maybe_block_size {
                match self.num_txns_map.entry(num_txns) {
                    Entry::Occupied(mut o) => {
                        *o.get_mut() += 1;
                    }
                    Entry::Vacant(v) => {
                        v.insert(1);
                    }
                }
            }
        }
        maybe_leaf
    }

    /// check if the test failed due to not enough nodes getting through enough views
    pub fn check_if_failed(&mut self, threshold: usize, total_num_nodes: usize) -> bool {
        let num_failed = self.failed_nodes.len();
        total_num_nodes - num_failed >= threshold
    }
    /// determines whether or not the round passes
    /// also do a safety check
    /// # Panics
    /// if the `num_txns_map` is somehow empty
    /// This should never happen because this function should never be called in that case
    #[allow(clippy::too_many_arguments, clippy::let_unit_value)]
    pub fn update_status(
        &mut self,
        threshold: usize,
        total_num_nodes: usize,
        key: &Leaf<TYPES>,
        check_leaf: bool,
        check_block: bool,
        transaction_threshold: u64,
    ) {
        let num_decided = self.success_nodes.len();
        let num_failed = self.failed_nodes.len();

        if check_leaf && self.leaf_map.len() != 1 {
            error!("LEAF MAP (that is mismatched) IS: {:?}", self.leaf_map);
            self.status = ViewStatus::Err(OverallSafetyTaskErr::MismatchedLeaf);
            return;
        }

        if check_block && self.block_map.len() != 1 {
            self.status = ViewStatus::Err(OverallSafetyTaskErr::InconsistentBlocks);
            return;
        }

        if transaction_threshold >= 1 {
            if self.num_txns_map.len() > 1 {
                self.status = ViewStatus::Err(OverallSafetyTaskErr::InconsistentTxnsNum {
                    map: self.num_txns_map.clone(),
                });
                return;
            }
            if *self.num_txns_map.iter().last().unwrap().0 < transaction_threshold {
                self.status = ViewStatus::Failed;
                return;
            }
        }

        // check for success
        if num_decided >= threshold {
            // decide on if we've succeeded.
            // if so, set state and return
            // if not, return error
            // if neither, continue through

            let block_key = key.get_payload_commitment();

            if *self.block_map.get(&block_key).unwrap() == threshold
                && *self.leaf_map.get(key).unwrap() == threshold
            {
                self.status = ViewStatus::Ok;
                return;
            }
        }

        let is_success_possible = total_num_nodes - num_failed >= threshold;
        if !is_success_possible {
            self.status = ViewStatus::Failed;
        }
    }

    /// generate leaves
    #[must_use]
    pub fn gen_leaves(&self) -> HashMap<Leaf<TYPES>, usize> {
        let mut leaves = HashMap::<Leaf<TYPES>, usize>::new();

        for (leaf_vec, _) in self.success_nodes.values() {
            let most_recent_leaf = leaf_vec.iter().last();
            if let Some((leaf, _)) = most_recent_leaf {
                match leaves.entry(leaf.clone()) {
                    std::collections::hash_map::Entry::Occupied(mut o) => {
                        *o.get_mut() += 1;
                    }
                    std::collections::hash_map::Entry::Vacant(v) => {
                        v.insert(1);
                    }
                }
            }
        }
        leaves
    }
}

/// cross node safety properties
#[derive(Clone)]
pub struct OverallSafetyPropertiesDescription {
    /// required number of successful views
    pub num_successful_views: usize,
    /// whether or not to check the leaf
    pub check_leaf: bool,
    /// whether or not to check the block
    pub check_block: bool,
    /// whether or not to check that we have threshold amounts of transactions each block
    /// if 0: don't check
    /// if n > 0, check that at least n transactions are decided upon if such information
    /// is available
    pub transaction_threshold: u64,
    /// num of total rounds allowed to fail
    pub num_failed_views: usize,
    /// threshold calculator. Given number of live and total nodes, provide number of successes
    /// required to mark view as successful
    pub threshold_calculator: Arc<dyn Fn(usize, usize) -> usize + Send + Sync>,
}

impl std::fmt::Debug for OverallSafetyPropertiesDescription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OverallSafetyPropertiesDescription")
            .field("num successful views", &self.num_successful_views)
            .field("check leaf", &self.check_leaf)
            .field("check_block", &self.check_block)
            .field("num_failed_rounds_total", &self.num_failed_views)
            .field("transaction_threshold", &self.transaction_threshold)
            .finish_non_exhaustive()
    }
}

impl Default for OverallSafetyPropertiesDescription {
    fn default() -> Self {
        Self {
            num_successful_views: 50,
            check_leaf: false,
            check_block: true,
            num_failed_views: 0,
            transaction_threshold: 0,
            // very strict
            threshold_calculator: Arc::new(|_num_live, num_total| 2 * num_total / 3 + 1),
        }
    }
}
