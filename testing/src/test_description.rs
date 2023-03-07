use std::{collections::HashSet, num::NonZeroUsize, sync::Arc, time::Duration};

use crate::{
    ConsensusRoundError, Round, RoundPostSafetyCheck, RoundResult, RoundSetup, TestLauncher,
    TestRunner,
};
use async_compatibility_layer::logging::{setup_backtrace, setup_logging};
use either::Either::{self, Left, Right};
use futures::{future::LocalBoxFuture, FutureExt};
use hotshot::{traits::NetworkReliability, types::Message, HotShot, HotShotError, ViewRunner};
use hotshot_types::traits::election::ConsensusExchange;
use hotshot_types::{
    data::TestableLeaf,
    traits::{
        election::Membership,
        network::{CommunicationChannel, TestableNetworkingImplementation},
        node_implementation::{
            NodeImplementation, NodeType, QuorumMembership, QuorumProposal, QuorumVoteType,
            TestableNodeImplementation,
        },
        signature_key::TestableSignatureKey,
        state::{TestableBlock, TestableState},
        storage::TestableStorage,
    },
    vote::QuorumVote,
    HotShotConfig,
};
use snafu::Snafu;
use tracing::{error, info};

///! public infra for describing tests

/// TODO this should be taking in a election config
pub struct GeneralTestDescriptionBuilder {
    /// Total number of nodes in the test
    pub total_nodes: usize,
    /// nodes available at start
    pub start_nodes: usize,
    /// number of successful/passing rounds required for test to pass
    pub num_succeeds: usize,
    /// max number of failing rounds before test is failed
    pub failure_threshold: usize,
    /// Either a list of transactions submitter indexes to
    /// submit a random txn with each round, OR
    /// `tx_per_round` transactions are submitted each round
    /// to random nodes for `num_rounds + failure_threshold`
    /// Ignored if `self.rounds` is set
    pub txn_ids: Either<Vec<Vec<u64>>, usize>,
    /// Base duration for next-view timeout, in milliseconds
    pub next_view_timeout: u64,
    /// The exponential backoff ration for the next-view timeout
    pub timeout_ratio: (u64, u64),
    /// The delay a leader inserts before starting pre-commit, in milliseconds
    pub round_start_delay: u64,
    /// Delay after init before starting consensus, in milliseconds
    pub start_delay: u64,
    /// List of ids to shut down after spinning up
    pub ids_to_shut_down: Vec<HashSet<u64>>,
    /// Description of the network reliability
    pub network_reliability: Option<Arc<dyn NetworkReliability>>,
    /// number of bootstrap nodes
    pub num_bootstrap_nodes: usize,
    /// Maximum transactions allowed in a block
    pub max_transactions: NonZeroUsize,
    /// Minimum transactions required for a block
    pub min_transactions: usize,
    /// The minimum amount of time a leader has to wait to start a round
    pub propose_min_round_time: Duration,
    /// The maximum amount of time a leader can wait to start a round
    pub propose_max_round_time: Duration,
}

impl Default for GeneralTestDescriptionBuilder {
    /// by default, just a single round
    fn default() -> Self {
        Self {
            total_nodes: 5,
            start_nodes: 5,
            num_succeeds: 1,
            failure_threshold: 0,
            txn_ids: Right(1),
            next_view_timeout: 10000,
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            ids_to_shut_down: Vec::new(),
            network_reliability: None,
            num_bootstrap_nodes: 5,
            propose_min_round_time: Duration::new(0, 0),
            propose_max_round_time: Duration::new(5, 0),
            max_transactions: NonZeroUsize::new(999999).unwrap(),
            min_transactions: 0,
        }
    }
}

impl GeneralTestDescriptionBuilder {
    /// Default constructor for multiple rounds.
    pub fn default_multiple_rounds() -> Self {
        GeneralTestDescriptionBuilder {
            round_start_delay: 25,
            total_nodes: 10,
            start_nodes: 10,
            num_succeeds: 20,
            start_delay: 120000,
            ..GeneralTestDescriptionBuilder::default()
        }
    }

    /// Default constructor for stress testing.
    pub fn default_stress() -> Self {
        GeneralTestDescriptionBuilder {
            round_start_delay: 25,
            num_bootstrap_nodes: 15,
            timeout_ratio: (1, 1),
            total_nodes: 100,
            start_nodes: 100,
            num_succeeds: 5,
            next_view_timeout: 2000,
            start_delay: 20000,
            ..GeneralTestDescriptionBuilder::default()
        }
    }
}

/// fine-grained spec of test
/// including what should be run every round
/// and how to generate more rounds
pub struct DetailedTestDescriptionBuilder<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>
where
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType, Time = TYPES::Time>,
    TYPES::SignatureKey: TestableSignatureKey,
    <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
    >>::Networking: TestableNetworkingImplementation<
        TYPES,
        Message<TYPES, I>,
        QuorumProposal<TYPES, I>,
        QuorumVoteType<TYPES, I>,
        QuorumMembership<TYPES, I>,
    >,
    I::Storage: TestableStorage<TYPES, I::Leaf>,
    I::Leaf: TestableLeaf<NodeType = TYPES>,
{
    /// generic information used for the test
    pub general_info: GeneralTestDescriptionBuilder,

    /// list of rounds
    pub rounds: Option<Vec<Round<TYPES, I>>>,

    /// function to generate the runner
    pub gen_runner: GenRunner<TYPES, I>,
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

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> TestDescription<TYPES, I>
where
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType, Time = TYPES::Time>,
    TYPES::SignatureKey: TestableSignatureKey,
    <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
    >>::Networking: TestableNetworkingImplementation<
        TYPES,
        Message<TYPES, I>,
        QuorumProposal<TYPES, I>,
        QuorumVoteType<TYPES, I>,
        QuorumMembership<TYPES, I>,
    >,
    I::Storage: TestableStorage<TYPES, I::Leaf>,
    I::Leaf: TestableLeaf<NodeType = TYPES>,
{
    /// default implementation of generate runner
    pub fn gen_runner(&self) -> TestRunner<TYPES, I> {
        let launcher = TestLauncher::new(
            self.total_nodes,
            self.num_bootstrap_nodes,
            self.min_transactions,
            I::Membership::default_election_config(self.total_nodes as u64),
        );
        // modify runner to recognize timing params
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
    /// execute a consensus test based on `Self`
    /// total_nodes: num nodes to run with
    /// txn_ids: vec of vec of transaction ids to send each round
    pub async fn execute(self) -> Result<(), ConsensusRoundError>
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
            node.networking().wait_for_ready().await;
            info!("EXECUTOR: NODE {:?} IS READY", idx);
        }

        let len = self.rounds.len() as u64;
        runner.with_rounds(self.rounds);

        runner
            .execute_rounds(len, self.failure_threshold as u64)
            .await
            .unwrap();

        Ok(())
    }
}

impl GeneralTestDescriptionBuilder {
    /// build a test description from the builder
    pub fn build<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(
        self,
    ) -> TestDescription<TYPES, I>
    where
        TYPES::BlockType: TestableBlock,
        TYPES::StateType: TestableState<BlockType = TYPES::BlockType, Time = TYPES::Time>,
        TYPES::SignatureKey: TestableSignatureKey,
        <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
            TYPES,
            I::Leaf,
            Message<TYPES, I>,
        >>::Networking: TestableNetworkingImplementation<
            TYPES,
            Message<TYPES, I>,
            QuorumProposal<TYPES, I>,
            QuorumVoteType<TYPES, I>,
            QuorumMembership<TYPES, I>,
        >,
        I::Storage: TestableStorage<TYPES, I::Leaf>,
        I::Leaf: TestableLeaf<NodeType = TYPES>,
    {
        DetailedTestDescriptionBuilder {
            general_info: self,
            rounds: None,
            gen_runner: None,
        }
        .build()
    }
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> DetailedTestDescriptionBuilder<TYPES, I>
where
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType, Time = TYPES::Time>,
    TYPES::SignatureKey: TestableSignatureKey,
    <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
    >>::Networking: TestableNetworkingImplementation<
        TYPES,
        Message<TYPES, I>,
        QuorumProposal<TYPES, I>,
        QuorumVoteType<TYPES, I>,
        QuorumMembership<TYPES, I>,
    >,
    I::Storage: TestableStorage<TYPES, I::Leaf>,
    I::Leaf: TestableLeaf<NodeType = TYPES>,
{
    /// build a test description from a detailed testing spec
    pub fn build(self) -> TestDescription<TYPES, I> {
        let timing_config = TimingData {
            next_view_timeout: self.general_info.next_view_timeout,
            timeout_ratio: self.general_info.timeout_ratio,
            round_start_delay: self.general_info.round_start_delay,
            start_delay: self.general_info.start_delay,
            propose_min_round_time: self.general_info.propose_min_round_time,
            propose_max_round_time: self.general_info.propose_max_round_time,
        };

        let rounds = if let Some(rounds) = self.rounds {
            rounds
        } else {
            self.default_populate_rounds()
        };

        TestDescription {
            rounds,
            gen_runner: self.gen_runner,
            timing_config,
            network_reliability: self.general_info.network_reliability,
            total_nodes: self.general_info.total_nodes,
            start_nodes: self.general_info.start_nodes,
            failure_threshold: self.general_info.failure_threshold,
            num_bootstrap_nodes: self.general_info.num_bootstrap_nodes,
            max_transactions: self.general_info.max_transactions,
            min_transactions: self.general_info.min_transactions,
        }
    }
}

/// Description of a test. Contains all metadata necessary to execute test
pub struct TestDescription<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>
where
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType, Time = TYPES::Time>,
    TYPES::SignatureKey: TestableSignatureKey,
    // I::QuorumExchange: ConsensusExchange<
    //     TYPES,
    //     I::Leaf,
    //     Message<TYPES, I>,
    //     Networking = TestableNetworkingImplementation<TYPES, I::Proposal, I::Vote, I::Membership>,
    // >,
    I::Storage: TestableStorage<TYPES, I::Leaf>,
    I::Leaf: TestableLeaf<NodeType = TYPES>,
{
    /// TODO unneeded (should be sufficient to have gen runner)
    /// the ronds to run for the test
    pub rounds: Vec<Round<TYPES, I>>,
    /// function to create a [`TestRunner`]
    pub gen_runner: GenRunner<TYPES, I>,
    /// timing information applied to hotshots
    pub timing_config: TimingData,
    /// TODO this should be implementation detail of network (perhaps fed into
    /// constructor/generator).
    /// Description of the network reliability (dropped/mutated packets etc)
    /// if `None`, good networking conditions are assumed
    pub network_reliability: Option<Arc<dyn NetworkReliability>>,
    /// Total number of nodes in the test
    pub total_nodes: usize,
    /// nodes available at start
    /// The rest are assumed to be started in the round setup
    pub start_nodes: usize,
    /// max number of failing rounds before test is failed
    pub failure_threshold: usize,
    /// number bootstrap nodes
    pub num_bootstrap_nodes: usize,
    /// Maximum transactions allowed in a block
    pub max_transactions: NonZeroUsize,
    /// Minimum transactions required for a block
    pub min_transactions: usize,
}

/// type alias for generating a [`TestRunner`]
pub type GenRunner<TYPES, I> =
    Option<Arc<dyn Fn(&TestDescription<TYPES, I>) -> TestRunner<TYPES, I>>>;

/// type alias for doing setup for a consensus round
pub type TestSetup<TYPES, TRANS, I> =
    Vec<Box<dyn FnOnce(&mut TestRunner<TYPES, I>) -> LocalBoxFuture<Vec<TRANS>>>>;

/// given a description of rounds, generates such rounds
/// args
/// * `shut_down_ids`: vector of ids to shut down each round
/// * `submitter_ids`: vector of ids to submit txns to each round
/// * `num_rounds`: total number of rounds to generate
pub fn default_submitter_id_to_round<TYPES, I: TestableNodeImplementation<TYPES>>(
    mut shut_down_ids: Vec<HashSet<u64>>,
    submitter_ids: Vec<Vec<u64>>,
    num_rounds: u64,
) -> TestSetup<TYPES, TYPES::Transaction, I>
where
    TYPES: NodeType,
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType, Time = TYPES::Time>,
    TYPES::SignatureKey: TestableSignatureKey,
    <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
    >>::Networking: TestableNetworkingImplementation<
        TYPES,
        Message<TYPES, I>,
        QuorumProposal<TYPES, I>,
        QuorumVoteType<TYPES, I>,
        QuorumMembership<TYPES, I>,
    >,
    I::Storage: TestableStorage<TYPES, I::Leaf>,
    I::Leaf: TestableLeaf<NodeType = TYPES>,
{
    // make sure the lengths match so zip doesn't spit out none
    if shut_down_ids.len() < submitter_ids.len() {
        shut_down_ids.append(&mut vec![
            HashSet::new();
            submitter_ids.len() - shut_down_ids.len()
        ])
    }

    let mut rounds: TestSetup<TYPES, TYPES::Transaction, I> = Vec::new();
    for (round_ids, shutdown_ids) in submitter_ids.into_iter().zip(shut_down_ids.into_iter()) {
        let run_round: RoundSetup<TYPES, TYPES::Transaction, I> = Box::new(
            move |runner: &mut TestRunner<TYPES, I>| -> LocalBoxFuture<Vec<TYPES::Transaction>> {
                async move {
                    let mut rng = rand::thread_rng();
                    for id in shutdown_ids.clone() {
                        runner.shutdown(id).await.unwrap();
                    }
                    let mut txns = Vec::new();
                    for id in round_ids.clone() {
                        let new_txn = runner
                            .add_random_transaction(Some(id as usize), &mut rng)
                            .await;
                        txns.push(new_txn);
                    }
                    txns
                }
                .boxed_local()
            },
        );
        rounds.push(run_round);
    }

    // if there are not enough rounds, add some autogenerated ones to keep liveness
    if num_rounds > rounds.len() as u64 {
        let remaining_rounds = num_rounds - rounds.len() as u64;
        // just enough to keep round going
        let mut extra_rounds = default_randomized_ids_to_round(vec![], remaining_rounds, 1);
        rounds.append(&mut extra_rounds);
    }

    rounds
}

/// generate a randomized set of transactions each round
/// * `shut_down_ids`: vec of ids to shut down each round
/// * `txns_per_round`: number of transactions to submit each round
/// * `num_rounds`: number of rounds
pub fn default_randomized_ids_to_round<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(
    shut_down_ids: Vec<HashSet<u64>>,
    num_rounds: u64,
    txns_per_round: u64,
) -> TestSetup<TYPES, TYPES::Transaction, I>
where
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType, Time = TYPES::Time>,
    TYPES::SignatureKey: TestableSignatureKey,
    <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
    >>::Networking: TestableNetworkingImplementation<
        TYPES,
        Message<TYPES, I>,
        QuorumProposal<TYPES, I>,
        QuorumVoteType<TYPES, I>,
        QuorumMembership<TYPES, I>,
    >,
    I::Storage: TestableStorage<TYPES, I::Leaf>,
    I::Leaf: TestableLeaf<NodeType = TYPES>,
{
    let mut rounds: TestSetup<TYPES, TYPES::Transaction, I> = Vec::new();

    for round_idx in 0..num_rounds {
        let to_kill = shut_down_ids.get(round_idx as usize).cloned();
        let run_round: RoundSetup<TYPES, TYPES::Transaction, I> = Box::new(
            move |runner: &mut TestRunner<TYPES, I>| -> LocalBoxFuture<Vec<TYPES::Transaction>> {
                async move {
                    let mut rng = rand::thread_rng();
                    if let Some(to_shut_down) = to_kill.clone() {
                        for idx in to_shut_down {
                            runner.shutdown(idx).await.unwrap();
                        }
                    }

                    runner
                        .add_random_transactions(txns_per_round as usize, &mut rng)
                        .await
                        .unwrap()
                }
                .boxed_local()
            },
        );

        rounds.push(Box::new(run_round));
    }

    rounds
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> DetailedTestDescriptionBuilder<TYPES, I>
where
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType, Time = TYPES::Time>,
    TYPES::SignatureKey: TestableSignatureKey,
    <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
    >>::Networking: TestableNetworkingImplementation<
        TYPES,
        Message<TYPES, I>,
        QuorumProposal<TYPES, I>,
        QuorumVoteType<TYPES, I>,
        QuorumMembership<TYPES, I>,
    >,
    I::Storage: TestableStorage<TYPES, I::Leaf>,
    I::Leaf: TestableLeaf<NodeType = TYPES>,
{
    /// create rounds of consensus based on the data in `self`
    pub fn default_populate_rounds(&self) -> Vec<Round<TYPES, I>> {
        // total number of rounds to be prepared to run assuming there may be failures
        let total_rounds = self.general_info.num_succeeds + self.general_info.failure_threshold;

        let setups = match self.general_info.txn_ids.clone() {
            Left(l) => default_submitter_id_to_round(
                self.general_info.ids_to_shut_down.clone(),
                l,
                total_rounds as u64,
            ),
            Right(tx_per_round) => default_randomized_ids_to_round(
                self.general_info.ids_to_shut_down.clone(),
                total_rounds as u64,
                tx_per_round as u64,
            ),
        };

        setups
            .into_iter()
            .map(|setup| {
                let safety_check_post: RoundPostSafetyCheck<TYPES, I> = Box::new(
                    move |runner: &TestRunner<TYPES, I>,
                          results: RoundResult<TYPES, I::Leaf>|
                          -> LocalBoxFuture<Result<(), ConsensusRoundError>> {
                        async move {
                            info!(?results);
                            // this check is rather strict:
                            // 1) all nodes have the SAME state
                            // 2) no nodes failed
                            runner.validate_node_states().await;

                            if results.failures.is_empty() {
                                Ok(())
                            } else {
                                error!(
                                    "post safety check failed. Failing nodes {:?}",
                                    results.failures
                                );
                                Err(ConsensusRoundError::ReplicasTimedOut {})
                            }
                        }
                        .boxed_local()
                    },
                );

                Round {
                    setup_round: Some(setup),
                    safety_check_post: Some(safety_check_post),
                    safety_check_pre: None,
                }
            })
            .collect::<Vec<_>>()
    }
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
