use std::{collections::HashMap, sync::Arc};

use either::Either::{self, Left, Right};
use futures::{future::LocalBoxFuture, FutureExt};
use hotshot::traits::{NodeImplementation, TestableNodeImplementation};
use hotshot_types::{data::LeafType, traits::node_implementation::NodeType};
use tracing::error;

use crate::{
    round::{Round, RoundCtx, RoundHook, RoundResult, RoundSafetyCheck, RoundSetup},
    test_errors::ConsensusTestError,
    test_runner::TestRunner,
};

/// a builder for a round
pub struct RoundBuilder<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> {
    /// the setup / description for the round
    pub setup: Either<RoundSetup<TYPES, I>, RoundSetupBuilder>,
    /// the safety check for the round
    pub check: Either<RoundSafetyCheck<TYPES, I>, RoundSafetyCheckBuilder>,
    /// the hooks to run each round
    pub hooks: Vec<RoundHook<TYPES, I>>,
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> RoundBuilder<TYPES, I> {
    /// build the `Round` from the description
    pub fn build(self) -> Round<TYPES, I> {
        let setup = match self.setup {
            Left(setup) => setup,
            Right(desc) => desc.build(),
        };
        let check = match self.check {
            Left(check) => check,
            Right(desc) => desc.build(),
        };
        Round {
            setup_round: setup,
            safety_check: check,
            hooks: self.hooks,
        }
    }
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> Default for RoundBuilder<TYPES, I> {
    fn default() -> Self {
        Self {
            setup: Right(RoundSetupBuilder::default()),
            check: Right(RoundSafetyCheckBuilder::default()),
            hooks: vec![],
        }
    }
}

// TODO make this fancier by varying the size
/// describes how to set up the round
/// very naive as it stands. We want to add in more support for spinning up and down nodes
#[derive(Clone, Debug)]
pub struct RoundSetupBuilder {
    /// TODO add in sampling
    /// number of transactions to submit per view
    pub num_txns_per_round: usize,
    /// scheduled changes (spinning a node up or down)
    pub scheduled_changes: Vec<ChangeNode>,
}

impl Default for RoundSetupBuilder {
    fn default() -> Self {
        Self {
            num_txns_per_round: 30,
            scheduled_changes: vec![],
        }
    }
}

/// Spin the node up or down
#[derive(Clone, Debug)]
pub enum UpDown {
    /// spin the node up
    Up,
    /// spin the node down
    Down,
}

/// denotes a change in node state
#[derive(Clone, Debug)]
pub struct ChangeNode {
    /// the index of the node
    pub idx: usize,
    /// the view on which to take action
    pub view: usize,
    /// spin the node up or down
    pub updown: UpDown,
}

impl RoundSetupBuilder {
    /// build the round setup
    pub fn build<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(
        &self,
    ) -> RoundSetup<TYPES, I> {
        let Self {
            num_txns_per_round,
            scheduled_changes,
        } = self.clone();
        RoundSetup(Arc::new(
            move |runner: &mut TestRunner<TYPES, I>,
                  ctx: &RoundCtx<TYPES, I>|
                  -> LocalBoxFuture<Vec<TYPES::Transaction>> {
                let changes = scheduled_changes.clone();
                let cur_view = ctx.prior_round_results.len() + 1;
                async move {
                    let updowns = changes
                        .iter()
                        .filter(|node| node.view == cur_view)
                        .map(|node| match node.updown {
                            UpDown::Up => Either::Left(node.idx),
                            UpDown::Down => Either::Right(node.idx),
                        });
                    // maybe we should switch to itertools
                    // they have saner either functions
                    let startup = updowns.clone().filter_map(|node| node.left());
                    let shutdown = updowns.filter_map(|node| node.right());

                    for _node in startup {
                        // TODO implement
                        // runner.shutdown(node as u64 ).await.unwrap();
                    }

                    for node in shutdown {
                        runner.shutdown(node as u64).await.unwrap();
                    }

                    let mut rng = rand::thread_rng();
                    runner
                        .add_random_transactions(num_txns_per_round, &mut rng)
                        .await
                        .unwrap()
                }
                .boxed_local()
            },
        ))
    }
}

/// description to be passed to the view checker
#[derive(Clone, Debug)]
pub struct RoundSafetyCheckBuilder {
    /// number of out of sync nodes before considered failed
    pub num_out_of_sync: usize,
    /// whether or not to check the leaf
    pub check_leaf: bool,
    /// whether or not to check the state
    pub check_state: bool,
    /// whether or not to check the block
    pub check_block: bool,
    /// whether or not to check the transaction pool
    pub check_transactions: bool,
    /// num of consecutive failed rounds before failing
    pub num_failed_consecutive_rounds: usize,
    /// num of total rounds allowed to fail
    pub num_failed_rounds_total: usize,
}

impl Default for RoundSafetyCheckBuilder {
    fn default() -> Self {
        Self {
            num_out_of_sync: 5,
            check_leaf: true,
            check_state: true,
            check_block: true,
            check_transactions: true,
            num_failed_consecutive_rounds: 5,
            num_failed_rounds_total: 10,
        }
    }
}

impl RoundSafetyCheckBuilder {
    /// builds a saety check based on a `RoundSafetyCheckBuilder`
    pub fn build<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(
        self,
    ) -> RoundSafetyCheck<TYPES, I> {
        let Self {
            num_out_of_sync,
            check_leaf,
            check_state,
            check_block,
            // TODO is it possible to do this check?
            // We can't exactly check that the transactions all match those submitted
            // since we only known about state commitment
            check_transactions: _,
            num_failed_consecutive_rounds,
            num_failed_rounds_total,
        }: Self = self;

        RoundSafetyCheck(Arc::new(
            move |runner: &TestRunner<TYPES, I>,
                  ctx: &mut RoundCtx<TYPES, I>,
                  mut round_result: RoundResult<TYPES, <I as NodeImplementation<TYPES>>::Leaf>|
                  -> LocalBoxFuture<Result<(), ConsensusTestError>> {
                let runner_nodes = runner.nodes();
                let num_required_successful_nodes =
                    runner.nodes().collect::<Vec<_>>().len() - num_out_of_sync;
                async move {
                    if round_result.txns.is_empty() {
                        error!("No transations submitted this round. No progress will be made.");
                    }

                    if round_result.failed_nodes.len() >= num_out_of_sync {
                        ctx.views_since_progress += 1;
                        ctx.total_failed_views += 1;
                    } else {
                        ctx.views_since_progress = 0;
                    }

                    if ctx.views_since_progress >= num_failed_consecutive_rounds {
                        round_result.success = false;
                        ctx.prior_round_results.push(round_result);
                        return Err(ConsensusTestError::TooManyConsecutiveFailures);
                    }

                    if ctx.total_failed_views >= num_failed_rounds_total {
                        round_result.success = false;
                        ctx.prior_round_results.push(round_result);
                        return Err(ConsensusTestError::TooManyFailures);
                    }

                    // TODO this code is repetitive. Clean it up with either a function or doing
                    // all three checks at once.
                    let mut result_leaves = None;

                    if check_leaf {
                        let mut leaves = HashMap::<I::Leaf, usize>::new();
                        // group all the leaves since thankfully leaf implements hash
                        for node in runner_nodes {
                            let decide_leaf = node.get_decided_leaf().await;
                            match leaves.entry(decide_leaf) {
                                std::collections::hash_map::Entry::Occupied(mut o) => {
                                    *o.get_mut() += 1;
                                }
                                std::collections::hash_map::Entry::Vacant(v) => {
                                    v.insert(1);
                                }
                            }
                        }
                        for (leaf, num_nodes) in leaves {
                            if num_nodes >= num_required_successful_nodes {
                                result_leaves = Some(leaf);
                            }
                        }

                        if let Some(leaf) = result_leaves {
                            round_result.agreed_leaf = Some(leaf);
                        } else {
                            ctx.views_since_progress += 1;
                            ctx.total_failed_views += 1;
                            round_result.success = false;
                            ctx.prior_round_results.push(round_result);
                            return Err(ConsensusTestError::InconsistentLeaves);
                        }
                    }

                    let mut result_state = None;

                    if check_state {
                        let mut states =
                            HashMap::<<I::Leaf as LeafType>::MaybeState, usize>::new();
                        for (_idx, (s, _b)) in round_result.success_nodes.clone() {
                            if let Some(most_recent_state) = s.iter().last() {
                                match states.entry(most_recent_state.clone()) {
                                    std::collections::hash_map::Entry::Occupied(mut o) => {
                                        *o.get_mut() += 1;
                                    }
                                    std::collections::hash_map::Entry::Vacant(v) => {
                                        v.insert(1);
                                    }
                                }
                            }
                        }
                        for (state, num_nodes) in states {
                            if num_nodes >= num_required_successful_nodes {
                                result_state = Some(state);
                            }
                        }

                        if let Some(state) = result_state {
                            round_result.agreed_state = Some(state);
                        } else {
                            ctx.views_since_progress += 1;
                            ctx.total_failed_views += 1;
                            round_result.success = false;
                            ctx.prior_round_results.push(round_result);
                            return Err(ConsensusTestError::InconsistentStates);
                        }
                    }

                    let mut result_block = None;

                    if check_block {
                        let mut blocks = HashMap::<<I::Leaf as LeafType>::DeltasType, usize>::new();
                        for (_idx, (_s, b)) in round_result.success_nodes.clone() {
                            if let Some(most_recent_state) = b.iter().last() {
                                match blocks.entry(most_recent_state.clone()) {
                                    std::collections::hash_map::Entry::Occupied(mut o) => {
                                        *o.get_mut() += 1;
                                    }
                                    std::collections::hash_map::Entry::Vacant(v) => {
                                        v.insert(1);
                                    }
                                }
                            }
                        }
                        for (block, num_nodes) in blocks {
                            if num_nodes >= num_required_successful_nodes {
                                result_block = Some(block);
                            }
                        }

                        if result_block.is_none() {
                            ctx.views_since_progress += 1;
                            ctx.total_failed_views += 1;
                            round_result.success = false;
                            ctx.prior_round_results.push(round_result);
                            return Err(ConsensusTestError::InconsistentBlocks);
                        }
                    }

                    Ok(())
                }
                .boxed_local()
            },
        ))
    }
}
