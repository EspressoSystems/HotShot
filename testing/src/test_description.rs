use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::{collections::HashSet, sync::Arc, time::Duration};

use crate::{
    ConsensusFailedError, Round, RoundSafetyCheck, RoundResult, RoundSetup, TestLauncher,
    TestRunner, RoundPreSafetyCheck, RoundCtx, RoundHook,
};
use async_compatibility_layer::logging::{setup_backtrace, setup_logging};
use either::Either::{self, Left, Right};
use futures::Future;
use futures::{future::LocalBoxFuture, FutureExt};
use hotshot::traits::TestableNodeImplementation;
use hotshot::{traits::NetworkReliability, types::Message, HotShot, HotShotError, ViewRunner};
use hotshot_types::data::LeafType;
use hotshot_types::traits::election::ConsensusExchange;
use hotshot_types::{
    traits::{
        election::Membership,
        network::CommunicationChannel,
        node_implementation::{NodeImplementation, NodeType},
    },
    HotShotConfig,
};
use nll::nll_todo::nll_todo;
use snafu::Snafu;
use tracing::{error, info};

///! public infra for describing tests

pub struct TestBuilder {
    runner_builder: RunnerBuilder,
    round_builder: RoundBuilder,
}

/// TODO this should be taking in a election config
pub struct RunnerBuilder {
    /// Total number of nodes in the test
    pub total_nodes: usize,
    /// nodes available at start
    pub start_nodes: usize,
    /// number of successful/passing rounds required for test to pass
    /// equivalent to num_sucessful_views
    pub num_succeeds: usize,
    /// max number of failing rounds before test is failed
    pub failure_threshold: usize,
    /// number of txn per round
    /// TODO in the future we should make this sample from a distribution
    /// much like how network reliability is implemented
    pub num_txns_per_round: usize,
    /// Description of the network reliability
    /// `None` == perfect network
    pub network_reliability: Option<Arc<dyn NetworkReliability>>,
    /// number of bootstrap nodes
    pub num_bootstrap_nodes: usize,
    /// Maximum transactions allowed in a block
    pub max_transactions: NonZeroUsize,
    /// Minimum transactions required for a block
    pub min_transactions: usize,
    /// timing data
    pub timing_data: TimingData
}

impl Default for RunnerBuilder {
    /// by default, just a single round
    fn default() -> Self {
        Self {
            total_nodes: 5,
            start_nodes: 5,
            num_succeeds: 1,
            failure_threshold: 0,
            network_reliability: None,
            num_bootstrap_nodes: 5,
            max_transactions: NonZeroUsize::new(999999).unwrap(),
            min_transactions: 0,
            timing_data: TimingData::default(),
            num_txns_per_round: 20,
        }
    }
}

impl Default for TimingData {
    fn default() -> Self {
        Self {
            next_view_timeout: 10000,
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            propose_min_round_time: Duration::new(0, 0),
            propose_max_round_time: Duration::new(5, 0),
        }
    }
}

impl Default for RoundSetupBuilder {
    fn default() -> Self {
        Self {
            num_txns_per_round: 30,
            scheduled_changes: vec![]
        }
    }
}

pub struct RoundBuilder<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> {
    setup: Either<RoundSetup<TYPES, I>, RoundSetupBuilder>,
    check: Either<RoundPostSafetyCheck<TYPES, I>, RoundCheckBuilder>,
    hooks: Vec<RoundHook<TYPES, I>>
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> Default for RoundBuilder<TYPES, I> {
    fn default() -> Self {
        Self {
            setup: Either::Right(RoundSetupBuilder::default()),
            check: Either::Right(RoundCheckBuilder::default()),
            hooks: vec![]
        }
    }
}


impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> RoundBuilder<TYPES, I> {
    pub fn build(&self) -> Round<TYPES, I> {
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
            safety_check_post: check,
            hooks: self.hooks,
        }
    }
}

impl RunnerBuilder {
    /// Default constructor for multiple rounds.
    pub fn default_multiple_rounds() -> Self {
        RunnerBuilder {
            total_nodes: 10,
            start_nodes: 10,
            num_succeeds: 20,
            timing_data: TimingData {
                start_delay: 120000,
                round_start_delay: 25,
                ..TimingData::default()

            },
            ..RunnerBuilder::default()
        }
    }

    /// Default constructor for stress testing.
    pub fn default_stress() -> Self {
        RunnerBuilder {
            num_bootstrap_nodes: 15,
            total_nodes: 100,
            start_nodes: 100,
            num_succeeds: 5,
            timing_data: TimingData {
                next_view_timeout: 2000,
                timeout_ratio: (1, 1),
                start_delay: 20000,
                round_start_delay: 25,
                ..TimingData::default()
            },
            ..RunnerBuilder::default()
        }
    }
}

/// fine-grained spec of test
/// including what should be run every round
/// and how to generate more rounds
pub struct TestBuilder<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> {
    /// generic information used for the test
    pub metadata: RunnerBuilder,

    /// list of round descriptions
    pub round: RoundBuilder<TYPES, I>,
}

#[derive(Debug, Snafu)]
enum RoundError<TYPES: NodeType> {
    HotShot { source: HotShotError<TYPES> },
}

/// data describing how a round should be timed.
pub struct TimingData {
    /// Base duration for next-view timeout, in milliseconds
    pub next_view_timeout: u64,
    /// The exponential backoff ration for the next-view timeout
    pub timeout_ratio: (u64, u64),
    /// The delay a leader inserts before starting pre-commit, in milliseconds
    pub round_start_delay: u64,
    /// Delay after init before starting consensus, in milliseconds
    pub start_delay: u64,
    /// The minimum amount of time a leader has to wait to start a round
    pub propose_min_round_time: Duration,
    /// The maximum amount of time a leader can wait to start a round
    pub propose_max_round_time: Duration,
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> RunnerBuilder<TYPES, I> {
    pub fn gen_runner(&self) -> TestRunner<TYPES, I> {
        let launcher = TestLauncher::new(
            self.total_nodes,
            self.num_bootstrap_nodes,
            self.min_transactions,
            <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
                TYPES,
                I::Leaf,
                Message<TYPES, I>,
            >>::Membership::default_election_config(self.total_nodes as u64),
        );
        // FIXME timing config should be used here... but this breaks other things
        let set_timing_params =
            |a: &mut HotShotConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>| {
                a.next_view_timeout = self.timing_config.next_view_timeout;
                a.timeout_ratio = self.timing_config.timeout_ratio;
                a.round_start_delay = self.timing_config.round_start_delay;
                a.start_delay = self.timing_config.start_delay;
                a.propose_min_round_time = self.timing_config.propose_min_round_time;
                a.propose_max_round_time = self.timing_config.propose_max_round_time;
            };

        // create runner from launcher
        launcher
            // insert timing parameters
            .modify_default_config(set_timing_params)
            .launch()
    }
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> Test<TYPES, I> {
    /// execute a consensus test based on `Self`
    /// total_nodes: num nodes to run with
    /// txn_ids: vec of vec of transaction ids to send each round
    pub async fn execute(self) -> Result<(), ConsensusFailedError>
    where
        HotShot<TYPES::ConsensusType, TYPES, I>: ViewRunner<TYPES, I>,
    {
        setup_logging();
        setup_backtrace();

        let mut runner = if let Some(ref generator) = self.gen_runner {
            generator(&self)
        } else {
            self.gen_runner()
        };

        // configure nodes/timing
        runner.add_nodes(self.start_nodes).await;

        for (idx, node) in runner.nodes().collect::<Vec<_>>().iter().enumerate().rev() {
            node.quorum_network().wait_for_ready().await;
            node.committee_network().wait_for_ready().await;
            info!("EXECUTOR: NODE {:?} IS READY", idx);
        }

        runner.with_round(self.round);

        runner
            .execute_rounds(self.num_sucessful_views, self.failure_threshold)
            .await
            .unwrap();

        Ok(())
    }
}

impl RunnerBuilder {
    /// build a test description with "sane" defaults
    /// from test metadata
    pub fn build<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(
        self,
    ) -> Test<TYPES, I> {
        let round_description = self.gen_round_descriptions();
        Test {
            round: round_description.build(),
            gen_runner: self.gen_runner(),
        }
    }

    pub fn gen_round_descriptions<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(&self) -> RoundBuilder<TYPES, I> {
        let setup_desc = RoundSetupBuilder {
            num_txns_per_round: self.num_txns_per_round,
            scheduled_changes: vec![]
        };
        let check_desc = RoundCheckBuilder {
            num_out_of_sync: todo!(),
            max_consecutive_failed_rounds: todo!(),
            check_leaf: todo!(),
            check_state: todo!(),
            check_block: todo!(),
            check_transactions: todo!(),
            num_failed_consecutive_rounds: todo!(),
            num_failed_rounds_total: todo!(),
        };
    }
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>
    TestBuilder<TYPES, I>
{
    /// build a test description from a detailed testing spec
    pub fn build(self) -> Test<TYPES, I> {
        let round = self.round.build();

        nll_todo()
    }
}

impl Default for RoundCheckBuilder {
    fn default() -> Self {
        RoundCheckBuilder {
            num_out_of_sync: 10,
            max_consecutive_failed_rounds: 5,
            check_leaf: true,
            check_transactions: false,
            check_state: true,
            check_block: true,
            num_failed_consecutive_rounds: 10,
            num_failed_rounds_total: 10,
        }
    }
}

/// description to be passed to the view checker
pub struct RoundCheckBuilder {
    /// number of out of sync nodes before considered failed
    pub num_out_of_sync: usize,
    /// max number of consecutive rounds allowed to not reach decide
    pub max_consecutive_failed_rounds: usize,
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

impl RoundCheckBuilder {
    fn build<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(&self) -> RoundPostSafetyCheck<TYPES, I> {
        let Self {
            num_out_of_sync,
            max_consecutive_failed_rounds,
            check_leaf,
            check_state,
            check_block,
            check_transactions,
            num_failed_consecutive_rounds,
            num_failed_rounds_total,
        } : Self = *(self.clone());

        let post =
            RoundPostSafetyCheck(Arc::new(move |
                runner: &TestRunner<TYPES, I>,
                ctx: &mut RoundCtx<TYPES, I>,
                mut round_result: RoundResult<TYPES, <I as NodeImplementation<TYPES>>::Leaf>
            | -> LocalBoxFuture<Result<(), ConsensusFailedError>>
            {
                let runner_nodes = runner.nodes();
                let collective = runner.nodes().collect::<Vec<_>>().len() - num_out_of_sync;
                async move {

                    // No transactions were submitted?
                    // We won't make any progress. Err.
                    if round_result.txns.is_empty(){
                        round_result.success = false;
                        ctx.prior_round_results.push(round_result);
                        return Err(ConsensusFailedError::NoTransactionsSubmitted)
                    }

                    if round_result.failed_nodes.len() >= num_out_of_sync  {
                        ctx.views_since_progress += 1;
                        ctx.total_failed_views += 1;

                    } else {
                        ctx.views_since_progress = 0;
                    }

                    if ctx.views_since_progress >= num_failed_consecutive_rounds {
                        round_result.success = false;
                        ctx.prior_round_results.push(round_result);
                        return Err(ConsensusFailedError::TooManyConsecutiveFailures);
                    }

                    if ctx.total_failed_views >= num_failed_rounds_total {
                        round_result.success = false;
                        ctx.prior_round_results.push(round_result);
                        return Err(ConsensusFailedError::TooManyViewFailures);
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
                            if num_nodes >= collective {
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
                            return Err(ConsensusFailedError::InconsistentLeaves)
                        }
                    }

                    let mut result_state = None;

                    if check_state {
                        let mut states = HashMap::<<I::Leaf as LeafType>::StateCommitmentType, usize>::new();
                        for (_idx, (s, _b)) in round_result.success_nodes.clone() {

                            let most_recent_state = s.iter().last();

                            // match states.entry(s) {
                            //     std::collections::hash_map::Entry::Occupied(mut o) => {
                            //         *o.get_mut() += 1;
                            //     }
                            //     std::collections::hash_map::Entry::Vacant(v) => {
                            //         v.insert(1);
                            //     }
                            // }

                        }
                        for (state, num_nodes) in states {
                            if num_nodes >= collective {
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
                            return Err(ConsensusFailedError::InconsistentLeaves);
                        }

                    }

                    let mut result_block = None;

                    if check_block {
                        let mut blocks = HashMap::<Vec<<I::Leaf as LeafType>::DeltasType>, usize>::new();
                        for (_idx, (_s, b)) in round_result.success_nodes.clone() {

                            match blocks.entry(b) {
                                std::collections::hash_map::Entry::Occupied(mut o) => {
                                    *o.get_mut() += 1;
                                }
                                std::collections::hash_map::Entry::Vacant(v) => {
                                    v.insert(1);
                                }
                            }

                        }
                        for (block, num_nodes) in blocks {
                            if num_nodes >= collective {
                                result_block = Some(block);
                            }
                        }

                        if result_block.is_none() {
                            ctx.views_since_progress += 1;
                            ctx.total_failed_views += 1;
                            round_result.success = false;
                            ctx.prior_round_results.push(round_result);
                            return Err(ConsensusFailedError::InconsistentLeaves);
                        }

                    }

                    Ok(())

                }.boxed_local()
            }));

        post
    }
}

#[derive(Clone, Debug)]
pub enum UpDown {
    Up, Down
}

#[derive(Clone, Debug)]
pub struct ChangeNode {
    idx: usize,
    view: usize,
    updown: UpDown
}

// TODO make this fancier by varying the size
/// describes how to set up the round
/// very naive as it stands. We want to add in more support for spinning up and down nodes
#[derive(Clone, Debug)]
pub struct RoundSetupBuilder {
    /// TODO add in sampling
    /// number of transactions to submit per view
    pub num_txns_per_round: usize,
    pub scheduled_changes: Vec<ChangeNode>
}


impl RoundSetupBuilder {
    fn build<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(&self) -> RoundSetup<TYPES, I>{
        let Self {
            num_txns_per_round,
            scheduled_changes,
        } = self.clone();
        RoundSetup(Arc::new(
            move |
                    runner: &mut TestRunner<TYPES, I>,
                    ctx: &RoundCtx<TYPES, I>
                | -> LocalBoxFuture<Vec<TYPES::Transaction>> {
                let changes = scheduled_changes.clone();
                let cur_view = ctx.prior_round_results.len() + 1;
                async move {
                    let updowns =
                        changes.iter()
                        .filter(|node| node.view == cur_view)
                        .map(|node| {
                            match node.updown {
                                UpDown::Up => {
                                    Either::Left(node.idx)
                                },
                                UpDown::Down => {
                                    Either::Right(node.idx)
                                }
                            }

                        });
                    // maybe we should switch to itertools
                    // they have saner either functions
                    let startup = updowns.clone().filter_map(|node| node.left());
                    let shutdown = updowns.filter_map(|node| node.right());

                    for node in startup {
                        // TODO implement
                        // runner.shutdown(node as u64 ).await.unwrap();
                    }

                    for node in shutdown {
                        runner.shutdown(node as u64).await.unwrap();
                    }

                    let mut rng = rand::thread_rng();
                    runner
                        .add_random_transactions(num_txns_per_round as usize, &mut rng)
                        .await
                        .unwrap()
                }.boxed_local()

            }))
    }
}

/// Description of a test. Contains all metadata necessary to execute test
pub struct Test<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> {
    /// hooks to run during the test
    /// TODO long term this is the task
    pub round: Round<TYPES, I>,
    /// function to create a [`TestRunner`]
    pub gen_runner: GenRunner<TYPES, I>,
}

/// type alias for generating a [`TestRunner`]
pub type GenRunner<TYPES, I> =
    Option<Arc<dyn Fn(&Test<TYPES, I>) -> TestRunner<TYPES, I>>>;

/// type alias for doing setup for a consensus round
// pub type TestSetup<TYPES, TRANS, I> =
//     Vec<Box<dyn FnOnce(&mut TestRunner<TYPES, I>) -> LocalBoxFuture<Vec<TRANS>>>>;

/// given a description of rounds, generates such rounds
/// args
/// * `shut_down_ids`: vector of ids to shut down each round
/// * `submitter_ids`: vector of ids to submit txns to each round
/// * `num_rounds`: total number of rounds to generate
// pub fn default_submitter_id_to_round<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(
//     mut shut_down_ids: Vec<HashSet<u64>>,
//     submitter_ids: Vec<Vec<u64>>,
//     num_rounds: u64,
// ) -> TestSetup<TYPES, TYPES::Transaction, I> {
//     // make sure the lengths match so zip doesn't spit out none
//     if shut_down_ids.len() < submitter_ids.len() {
//         shut_down_ids.append(&mut vec![
//             HashSet::new();
//             submitter_ids.len() - shut_down_ids.len()
//         ])
//     }
//
//     let mut rounds: TestSetup<TYPES, TYPES::Transaction, I> = Vec::new();
//     for (round_ids, shutdown_ids) in submitter_ids.into_iter().zip(shut_down_ids.into_iter()) {
//         let run_round: RoundSetup<TYPES, TYPES::Transaction, I> = Box::new(
//             move |runner: &mut TestRunner<TYPES, I>| -> LocalBoxFuture<Vec<TYPES::Transaction>> {
//                 async move {
//                     let mut rng = rand::thread_rng();
//                     for id in shutdown_ids.clone() {
//                         runner.shutdown(id).await.unwrap();
//                     }
//                     let mut txns = Vec::new();
//                     for id in round_ids.clone() {
//                         let new_txn = runner
//                             .add_random_transaction(Some(id as usize), &mut rng)
//                             .await;
//                         txns.push(new_txn);
//                     }
//                     txns
//                 }
//                 .boxed_local()
//             },
//         );
//         rounds.push(run_round);
//     }
//
//     // if there are not enough rounds, add some autogenerated ones to keep liveness
//     if num_rounds > rounds.len() as u64 {
//         let remaining_rounds = num_rounds - rounds.len() as u64;
//         // just enough to keep round going
//         let mut extra_rounds = default_randomized_ids_to_round(vec![], remaining_rounds, 1);
//         rounds.append(&mut extra_rounds);
//     }
//
//     rounds
// }

/// generate a randomized set of transactions each round
/// * `shut_down_ids`: vec of ids to shut down each round
/// * `txns_per_round`: number of transactions to submit each round
/// * `num_rounds`: number of rounds
// pub fn default_randomized_ids_to_round<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(
//     shut_down_ids: Vec<HashSet<u64>>,
//     num_rounds: u64,
//     txns_per_round: u64,
// ) -> TestSetup<TYPES, TYPES::Transaction, I> {
//     let mut rounds: TestSetup<TYPES, TYPES::Transaction, I> = Vec::new();
//
//     for round_idx in 0..num_rounds {
//         let to_kill = shut_down_ids.get(round_idx as usize).cloned();
//         let run_round: RoundSetup<TYPES, TYPES::Transaction, I> = Box::new(
//             move |runner: &mut TestRunner<TYPES, I>| -> LocalBoxFuture<Vec<TYPES::Transaction>> {
//                 async move {
//                     let mut rng = rand::thread_rng();
//                     if let Some(to_shut_down) = to_kill.clone() {
//                         for idx in to_shut_down {
//                             runner.shutdown(idx).await.unwrap();
//                         }
//                     }
//
//                     runner
//                         .add_random_transactions(txns_per_round as usize, &mut rng)
//                         .await
//                         .unwrap()
//                 }
//                 .boxed_local()
//             },
//         );
//
//         rounds.push(Box::new(run_round));
//     }
//
//     rounds
// }

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>
    TestBuilder<TYPES, I>
{
    // /// create rounds of consensus based on the data in `self`
    // pub fn default_populate_rounds(&self) -> Vec<Round<TYPES, I>> {
    //     /// if we're calling this function, we have no rounds!
    //     let desc = self.round.right().unwrap();
    //     // total number of rounds to be prepared to run assuming there may be failures
    //     let total_rounds = self.general_info.num_succeeds + self.general_info.failure_threshold;
    //
    //     let setups = match self.general_info.txn_ids.clone() {
    //         Left(l) => default_submitter_id_to_round(
    //             self.general_info.ids_to_shut_down.clone(),
    //             l,
    //             total_rounds as u64,
    //         ),
    //         Right(tx_per_round) => default_randomized_ids_to_round(
    //             self.general_info.ids_to_shut_down.clone(),
    //             total_rounds as u64,
    //             tx_per_round as u64,
    //         ),
    //     };
    //
    //     setups
    //         .into_iter()
    //         .map(|setup| {
    //             // FIXME we should be passing in a ctx about the run
    //             // FIXME we should be returning "results" that we use to generate a report
    //             let safety_check_post: RoundPostSafetyCheck<TYPES, I> = Box::new(
    //                 move |runner: &TestRunner<TYPES, I>,
    //                       results: RoundResult<TYPES, I::Leaf>|
    //                       -> LocalBoxFuture<Result<(), ConsensusRoundError>> {
    //                     async move {
    //                         info!(?results);
    //                         error!("SAFETY CHECK BEING RUNNNN");
    //                         // this check is rather strict:
    //                         // 1) all nodes have the SAME state
    //                         // 2) no nodes failed
    //                         runner.validate_nodes(&desc, todo!()).await;
    //
    //                         if results.failures.is_empty() {
    //                             Ok(())
    //                         } else {
    //                             error!(
    //                                 "post safety check failed. Failing nodes {:?}",
    //                                 results.failures
    //                             );
    //                             Err(ConsensusRoundError::ReplicasTimedOut {})
    //                         }
    //                     }
    //                     .boxed_local()
    //                 },
    //             );
    //
    //             Round {
    //                 setup_round: setup,
    //                 safety_check_post: safety_check_post,
    //                 safety_check_pre: None,
    //             }
    //         })
    //         .collect::<Vec<_>>()
    // }
}

/// given `num_nodes`, calculate min number of honest nodes
/// for consensus to function properly
pub fn get_threshold(num_nodes: u64) -> u64 {
    ((num_nodes * 2) / 3) + 1
}

/// given `num_nodes`, calculate max number of byzantine nodes
pub fn get_tolerance(num_nodes: u64) -> u64 {
    num_nodes - get_threshold(num_nodes)
}
