use std::{collections::HashMap, sync::Arc};

use futures::{future::LocalBoxFuture, FutureExt};
use hotshot::traits::{NodeImplementation, TestableNodeImplementation};
use hotshot_types::{data::LeafType, traits::node_implementation::NodeType};
use tracing::error;

use crate::{
    round::{RoundCtx, RoundResult, RoundSafetyCheck, RoundSetup, StateAndBlock},
    test_errors::{ConsensusRoundError, ConsensusTestError},
    test_runner::TestRunner,
};

/// describes how to set up the round
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
                            UpDown::Up => either::Either::Left(node.idx),
                            UpDown::Down => either::Either::Right(node.idx),
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
            check_leaf: false,
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
            // TODO <https://github.com/EspressoSystems/HotShot/issues/1167>
            // We can't exactly check that the transactions all match those submitted
            // because of a type error. We ONLY have `MaybeState` and we need `State`.
            // unless we specialize, this won't happen.
            // so waiting on refactor for this
            // code is below:
            //
            //     ```
            //         let next_state /* : Option<_> */ = {
            //         if let Some(last_leaf) = ctx.prior_round_results.iter().last() {
            //             if let Some(parent_state) = last_leaf.agreed_state {
            //                 let mut block = <TYPES as NodeType>::StateType::next_block(Some(parent_state.clone()));
            //             }
            //         }
            //     };
            // ```
            check_transactions: _,
            num_failed_consecutive_rounds,
            num_failed_rounds_total,
        }: Self = self;

        RoundSafetyCheck(Arc::new(
            move |runner: &TestRunner<TYPES, I>,
                  ctx: &mut RoundCtx<TYPES, I>,
                  mut round_result: RoundResult<TYPES, <I as NodeImplementation<TYPES>>::Leaf>|
                  -> LocalBoxFuture<Result<(), ConsensusTestError>> {
                error!(
                    "VIEW {:?}",
                    ctx.total_failed_views + ctx.total_successful_views
                );

                let runner_nodes = runner.nodes();
                let num_required_successful_nodes = {
                    let num_nodes = runner.nodes().collect::<Vec<_>>().len();
                    // prevent overflow
                    if num_nodes < num_out_of_sync {
                        0
                    } else {
                        num_nodes - num_out_of_sync
                    }
                };
                error!(
                    "number required success nodes: {:?}",
                    num_required_successful_nodes
                );
                async move {
                    if round_result.txns.is_empty() {
                        error!("No transations submitted this round. No progress will be made.");
                    }

                    let failed_to_make_progress =
                        round_result.failed_nodes.len() >= num_out_of_sync;

                    if failed_to_make_progress {
                        ctx.views_since_progress += 1;
                        error!(
                            "The majority of nodes haven't made progress in {:?} views!",
                            ctx.views_since_progress
                        );
                        ctx.total_failed_views += 1;
                    } else {
                        ctx.views_since_progress = 0;
                    }

                    if ctx.views_since_progress >= num_failed_consecutive_rounds {
                        round_result.success = Err(ConsensusRoundError::TooManyTimedOutNodes);
                        ctx.prior_round_results.push(round_result);
                        ctx.print_summary();
                        return Err(ConsensusTestError::TooManyConsecutiveFailures);
                    }

                    // NOTE this only checks for timeout failures
                    if ctx.total_failed_views >= num_failed_rounds_total {
                        round_result.success = Err(ConsensusRoundError::TooManyTimedOutNodes);
                        ctx.prior_round_results.push(round_result);
                        ctx.print_summary();
                        return Err(ConsensusTestError::TooManyFailures);
                    }

                    if failed_to_make_progress {
                        ctx.prior_round_results.push(round_result);
                        ctx.print_summary();
                        return Ok(());
                    }

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
                            round_result.success = Err(ConsensusRoundError::InconsistentLeaves);
                            ctx.prior_round_results.push(round_result);
                            ctx.print_summary();
                            return Ok(());
                        }
                    }

                    let mut result_state = None;
                    let mut result_block = None;
                    let mut num_no_progress = 0;

                    let mut states = HashMap::<<I::Leaf as LeafType>::MaybeState, usize>::new();
                    let mut blocks = HashMap::<<I::Leaf as LeafType>::DeltasType, usize>::new();

                    for (_idx, leaves) in round_result.success_nodes.clone() {
                        let (state, block): StateAndBlock<
                            <I::Leaf as LeafType>::MaybeState,
                            <I::Leaf as LeafType>::DeltasType,
                        > = leaves
                            .0
                            .iter()
                            .cloned()
                            .map(|leaf| (leaf.get_state(), leaf.get_deltas()))
                            .unzip();

                        if let (Some(most_recent_state), Some(most_recent_block)) =
                            (state.iter().last(), block.iter().last())
                        {
                            match states.entry(most_recent_state.clone()) {
                                std::collections::hash_map::Entry::Occupied(mut o) => {
                                    *o.get_mut() += 1;
                                }
                                std::collections::hash_map::Entry::Vacant(v) => {
                                    v.insert(1);
                                }
                            }
                            match blocks.entry(most_recent_block.clone()) {
                                std::collections::hash_map::Entry::Occupied(mut o) => {
                                    *o.get_mut() += 1;
                                }
                                std::collections::hash_map::Entry::Vacant(v) => {
                                    v.insert(1);
                                }
                            }
                        } else {
                            num_no_progress += 1;
                        }
                    }

                    error!(
                        "states for this view {:#?}\nblocks for this view {:#?}",
                        states, blocks
                    );

                    error!(
                        "Number of nodes who made zero progress: {:#?}",
                        num_no_progress
                    );

                    if num_no_progress >= num_out_of_sync {
                        error!("No progress was made on majority of nodes");
                        ctx.views_since_progress += 1;
                        ctx.total_failed_views += 1;
                        round_result.success = Err(ConsensusRoundError::NoMajorityProgress);
                        ctx.prior_round_results.push(round_result);
                        ctx.print_summary();
                        return Ok(());
                    }

                    if check_state {
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
                            round_result.success = Err(ConsensusRoundError::InconsistentStates);
                            ctx.prior_round_results.push(round_result);
                            ctx.print_summary();
                            return Ok(());
                        }
                    }

                    if check_block {
                        for (block, num_nodes) in blocks {
                            if num_nodes >= num_required_successful_nodes {
                                result_block = Some(block);
                            }
                        }

                        if let Some(block) = result_block {
                            round_result.agreed_block = Some(block);
                        } else {
                            ctx.views_since_progress += 1;
                            ctx.total_failed_views += 1;
                            round_result.success = Err(ConsensusRoundError::InconsistentBlocks);
                            ctx.prior_round_results.push(round_result);
                            ctx.print_summary();
                            return Ok(());
                        }
                    }
                    ctx.total_successful_views += 1;
                    // redundant but just in case
                    round_result.success = Ok(());
                    if ctx.total_successful_views >= runner.num_succeeds() {
                        ctx.prior_round_results.push(round_result);
                        ctx.print_summary();
                        return Err(ConsensusTestError::CompletedTestSuccessfully);
                    }
                    ctx.print_summary();
                    ctx.prior_round_results.push(round_result);
                    Ok(())
                }
                .boxed_local()
            },
        ))
    }
}
