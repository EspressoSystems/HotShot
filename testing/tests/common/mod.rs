#![allow(dead_code)]

use ark_bls12_381::Parameters as Param381;
use async_compatibility_layer::logging::{setup_backtrace, setup_logging};
use blake3::Hasher;
use either::Either;
use futures::{future::LocalBoxFuture, FutureExt};
use hotshot::{
    demos::vdemo::{VDemoBlock, VDemoState, VDemoTransaction},
    traits::{
        dummy::DummyState,
        election::{
            static_committee::{StaticCommittee, StaticElectionConfig, StaticVoteToken},
            vrf::{JfPubKey, VRFStakeTableConfig, VRFVoteToken, VrfImpl},
        },
        implementations::{MemoryCommChannel, MemoryStorage},
        NetworkReliability,
    },
    HotShot, HotShotError, ViewRunner,
};
use hotshot_testing::{
    ConsensusRoundError, Round, RoundPostSafetyCheck, RoundResult, RoundSetup, TestLauncher,
    TestNodeImpl, TestRunner,
};
use hotshot_types::{data::TestableLeaf, message::QuorumVote};
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal, ViewNumber},
    traits::{
        block_contents::dummy::{DummyBlock, DummyTransaction},
        election::Membership,
        network::{CommunicationChannel, TestableNetworkingImplementation},
        node_implementation::{ApplicationMetadata, NodeType, TestableNodeImplementation},
        signature_key::TestableSignatureKey,
        state::{TestableBlock, TestableState, ValidatingConsensus},
        storage::TestableStorage,
    },
    HotShotConfig,
};
use jf_primitives::{
    signatures::{
        bls::{BLSSignature, BLSVerKey},
        BLSSignatureScheme,
    },
    vrf::blsvrf::BLSVRFScheme,
};
use serde::{Deserialize, Serialize};
use snafu::Snafu;
use std::{collections::HashSet, num::NonZeroUsize, sync::Arc, time::Duration};
use tracing::{error, info};
use Either::{Left, Right};

#[derive(Debug, Snafu)]
enum RoundError<TYPES: NodeType> {
    HotShot { source: HotShotError<TYPES> },
}

pub const N: usize = 32_usize;

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
    // Maximum transactions allowed in a block
    pub max_transactions: NonZeroUsize,
    // Minimum transactions required for a block
    pub min_transactions: usize,
    /// The minimum amount of time a leader has to wait to start a round
    pub propose_min_round_time: Duration,
    /// The maximum amount of time a leader can wait to start a round
    pub propose_max_round_time: Duration,
}

pub struct DetailedTestDescriptionBuilder<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>
where
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType, Time = TYPES::Time>,
    TYPES::SignatureKey: TestableSignatureKey,
    I::Networking: TestableNetworkingImplementation<TYPES, I::Proposal, I::Vote, I::Election>,
    I::Storage: TestableStorage<TYPES, I::Leaf>,
    I::Leaf: TestableLeaf<NodeType = TYPES>,
{
    pub general_info: GeneralTestDescriptionBuilder,

    /// list of rounds
    pub rounds: Option<Vec<Round<TYPES, I>>>,

    /// function to generate the runner
    pub gen_runner: GenRunner<TYPES, I>,
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> TestDescription<TYPES, I>
where
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType, Time = TYPES::Time>,
    TYPES::SignatureKey: TestableSignatureKey,
    I::Networking: TestableNetworkingImplementation<TYPES, I::Proposal, I::Vote, I::Election>,
    I::Storage: TestableStorage<TYPES, I::Leaf>,
    I::Leaf: TestableLeaf<NodeType = TYPES>,
{
    /// default implementation of generate runner
    pub fn gen_runner(&self) -> TestRunner<TYPES, I> {
        let launcher = TestLauncher::new(
            self.total_nodes,
            self.num_bootstrap_nodes,
            self.min_transactions,
            I::Election::default_election_config(self.total_nodes as u64),
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
    pub fn build<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(
        self,
    ) -> TestDescription<TYPES, I>
    where
        TYPES::BlockType: TestableBlock,
        TYPES::StateType: TestableState<BlockType = TYPES::BlockType, Time = TYPES::Time>,
        TYPES::SignatureKey: TestableSignatureKey,
        I::Networking: TestableNetworkingImplementation<TYPES, I::Proposal, I::Vote, I::Election>,
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
    I::Networking: TestableNetworkingImplementation<TYPES, I::Proposal, I::Vote, I::Election>,
    I::Storage: TestableStorage<TYPES, I::Leaf>,
    I::Leaf: TestableLeaf<NodeType = TYPES>,
{
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
    I::Networking: TestableNetworkingImplementation<TYPES, I::Proposal, I::Vote, I::Election>,
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
    // Maximum transactions allowed in a block
    pub max_transactions: NonZeroUsize,
    // Minimum transactions required for a block
    pub min_transactions: usize,
}

/// type alias for generating a [`TestRunner`]
pub type GenRunner<TYPES, I> =
    Option<Arc<dyn Fn(&TestDescription<TYPES, I>) -> TestRunner<TYPES, I>>>;

/// type alias for doing setup for a consensus round
pub type TestSetup<TYPES, TRANS, I> =
    Vec<Box<dyn FnOnce(&mut TestRunner<TYPES, I>) -> LocalBoxFuture<Vec<TRANS>>>>;

/// application metadata stub
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct VrfTestMetaData {}

impl ApplicationMetadata for VrfTestMetaData {}

#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct VrfTestTypes;
impl NodeType for VrfTestTypes {
    // TODO (da) can this be SequencingConsensus?
    type ConsensusType = ValidatingConsensus;
    type Time = ViewNumber;
    type BlockType = DummyBlock;
    type SignatureKey = JfPubKey<BLSSignatureScheme<Param381>>;
    type VoteTokenType = VRFVoteToken<BLSVerKey<Param381>, BLSSignature<Param381>>;
    type Transaction = DummyTransaction;
    type ElectionConfigType = VRFStakeTableConfig;
    type StateType = DummyState;
    type ApplicationMetadataType = VrfTestMetaData;
}

/// application metadata stub
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct StaticCommitteeMetaData {}

impl ApplicationMetadata for StaticCommitteeMetaData {}

#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct StaticCommitteeTestTypes;
impl NodeType for StaticCommitteeTestTypes {
    type ConsensusType = ValidatingConsensus;
    type Time = ViewNumber;
    type BlockType = VDemoBlock;
    type SignatureKey = JfPubKey<BLSSignatureScheme<Param381>>;
    type VoteTokenType = StaticVoteToken<JfPubKey<BLSSignatureScheme<Param381>>>;
    type Transaction = VDemoTransaction;
    type ElectionConfigType = StaticElectionConfig;
    type StateType = VDemoState;
    type ApplicationMetadataType = StaticCommitteeMetaData;
}

/// type synonym for vrf committee election
/// with in-memory network
pub type StandardNodeImplType = TestNodeImpl<
    VrfTestTypes,
    ValidatingLeaf<VrfTestTypes>,
    ValidatingProposal<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
    QuorumVote<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
    MemoryCommChannel<
        VrfTestTypes,
        ValidatingProposal<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
        QuorumVote<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
        VrfImpl<
            VrfTestTypes,
            ValidatingLeaf<VrfTestTypes>,
            BLSSignatureScheme<Param381>,
            BLSVRFScheme<Param381>,
            Hasher,
            Param381,
        >,
    >,
    MemoryStorage<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
    VrfImpl<
        VrfTestTypes,
        ValidatingLeaf<VrfTestTypes>,
        BLSSignatureScheme<Param381>,
        BLSVRFScheme<Param381>,
        Hasher,
        Param381,
    >,
>;

/// type synonym for static committee
/// with in-memory network
pub type StaticNodeImplType = TestNodeImpl<
    StaticCommitteeTestTypes,
    ValidatingLeaf<StaticCommitteeTestTypes>,
    ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    QuorumVote<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    MemoryCommChannel<
        StaticCommitteeTestTypes,
        ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
        QuorumVote<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
        StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    >,
    MemoryStorage<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
>;

/// type alias for the test runner type
pub type AppliedTestRunner<TYPES, LEAF, PROPOSAL, VOTE, MEMBERSHIP> =
    TestRunner<TYPES, AppliedTestNodeImpl<TYPES, LEAF, PROPOSAL, VOTE, MEMBERSHIP>>;
pub type AppliedTestNodeImpl<TYPES, LEAF, PROPOSAL, VOTE, MEMBERSHIP> = TestNodeImpl<
    TYPES,
    LEAF,
    PROPOSAL,
    VOTE,
    MemoryCommChannel<TYPES, PROPOSAL, VOTE, MEMBERSHIP>,
    MemoryStorage<TYPES, LEAF>,
    MEMBERSHIP,
>;

// FIXME THIS is why we need to split up metadat and anonymous functions
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
    I::Networking: TestableNetworkingImplementation<TYPES, I::Proposal, I::Vote, I::Election>,
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
    I::Networking: TestableNetworkingImplementation<TYPES, I::Proposal, I::Vote, I::Election>,
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
    I::Networking: TestableNetworkingImplementation<TYPES, I::Proposal, I::Vote, I::Election>,
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

/// Generate the inside of a test.
/// Only for internal usage.
#[macro_export]
macro_rules! gen_inner_fn {
    ($TEST_TYPE:ty, $e:expr) => {
        // NOTE we need this since proptest doesn't implement async things
        async_compatibility_layer::art::async_block_on(async move {
            async_compatibility_layer::logging::setup_logging();
            async_compatibility_layer::logging::setup_backtrace();
            let description = $e;
            let built: $TEST_TYPE = description.build();
            built.execute().await.unwrap()
        });
    };
}

/// special casing for proptest. Need the inner thing to block
#[macro_export]
macro_rules! gen_inner_fn_proptest {
    ($TEST_TYPE:ty, $e:expr) => {
        // NOTE we need this since proptest doesn't implement async things
        async_compatibility_layer::art::async_block_on_with_runtime(async move {
            async_compatibility_layer::logging::setup_logging();
            async_compatibility_layer::logging::setup_backtrace();
            let description = $e;
            let built: $TEST_TYPE = description.build();
            built.execute().await.unwrap()
        });
    };
}

/// Generate a test.
/// Args:
/// - $TEST_TYPE: TestDescription type
/// - $fn_name: name of test
/// - $e: The test description
/// - $keep: whether or not to ignore the test
/// - $args: list of arguments to fuzz over (fed to proptest)
#[macro_export]
macro_rules! cross_test {
    // base case
    ($TEST_TYPE:ty, $fn_name:ident, $e:expr, keep: true, slow: false, args: $($args:tt)+) => {
        proptest::prelude::proptest!{
            #![proptest_config(
                proptest::prelude::ProptestConfig {
                    timeout: 300000,
                    cases: 10,
                    .. proptest::prelude::ProptestConfig::default()
                }
                )]
                #[test]
                fn $fn_name($($args)+) {
                    gen_inner_fn_proptest!($TEST_TYPE, $e);
                }
        }
    };
    ($TEST_TYPE:ty, $fn_name:ident, $e:expr, keep: true, slow: true, args: $($args:tt)+) => {
        proptest::prelude::proptest!{
            #![proptest_config(
                proptest::prelude::ProptestConfig {
                    timeout: 300000,
                    cases: 10,
                    .. proptest::prelude::ProptestConfig::default()
                }
                )]
                #[cfg(feature = "slow-tests")]
                #[test]
                fn $fn_name($($args)+) {
                    gen_inner_fn_proptest!($TEST_TYPE, $e);
                }
        }
    };
    ($TEST_TYPE:ty, $fn_name:ident, $e:expr, keep: false, slow: false, args: $($args:tt)+) => {
        proptest::prelude::proptest!{
            #![proptest_config(
                proptest::prelude::ProptestConfig {
                    timeout: 300000,
                    cases: 10,
                    .. proptest::prelude::ProptestConfig::default()
                }
                )]
                #[test]
                #[ignore]
                fn $fn_name($($args)+) {
                    gen_inner_fn_proptest!($TEST_TYPE, $e);
                }
        }
    };
    ($TEST_TYPE:ty, $fn_name:ident, $e:expr, keep: false, slow: true, args: $($args:tt)+) => {
        proptest::prelude::proptest!{
            #![proptest_config(
                proptest::prelude::ProptestConfig {
                    timeout: 300000,
                    cases: 10,
                    .. proptest::prelude::ProptestConfig::default()
                }
                )]
                #[cfg(feature = "slow-tests")]
                #[test]
                #[ignore]
                fn $fn_name($($args)+) {
                    gen_inner_fn!($TEST_TYPE, $e);
                }
        }
    };
    ($TEST_TYPE:ty, $fn_name:ident, $e:expr, keep: true, slow: false, args: ) => {
        #[cfg_attr(
            feature = "tokio-executor",
            tokio::test(flavor = "multi_thread", worker_threads = 2)
        )]
        #[cfg_attr(feature = "async-std-executor", async_std::test)]
        async fn $fn_name() {
            gen_inner_fn!($TEST_TYPE, $e);
        }
    };
    ($TEST_TYPE:ty, $fn_name:ident, $e:expr, keep: true, slow: true, args: ) => {
        #[cfg(feature = "slow-tests")]
        #[cfg_attr(
            feature = "tokio-executor",
            tokio::test(flavor = "multi_thread", worker_threads = 2)
        )]
        #[cfg_attr(feature = "async-std-executor", async_std::test)]
        async fn $fn_name() {
            gen_inner_fn!($TEST_TYPE, $e);
        }
    };
    ($TEST_TYPE:ty, $fn_name:ident, $e:expr, keep: false, slow: false, args: ) => {
        #[cfg_attr(
            feature = "tokio-executor",
            tokio::test(flavor = "multi_thread", worker_threads = 2)
        )]
        #[cfg_attr(feature = "async-std-executor", async_std::test)]
        #[ignore]
        async fn $fn_name() {
            gen_inner_fn!($TEST_TYPE, $e);
        }
    };
    ($TEST_TYPE:ty, $fn_name:ident, $e:expr, keep: false, slow: true, args: ) => {
        #[cfg(feature = "slow-tests")]
        #[cfg_attr(
            feature = "tokio-executor",
            tokio::test(flavor = "multi_thread", worker_threads = 2)
        )]
        #[ignore]
        async fn $fn_name() {
            gen_inner_fn!($TEST_TYPE, $e);
        }
    };
}

/// Macro to generate tests for all types based on a description
/// Arguments:
/// - $NETWORKS: a space delimited list of Network implementations
/// - $STORAGES: a space delimited list of Storage implementations
/// - $BLOCKS: a space delimited list of Block implementations
/// - $STATES: a space delimited list of State implementations
/// - $fn_name: a identifier for the outermost test module
/// - $expr: a TestDescription for the test
/// - $keep:
///   - true is a noop
///   - false forces test to be ignored
/// - $args: list of arguments to fuzz over (fed to proptest)
///
// TestNodeImpl<DEntryState, MemoryStorage<DEntryState>, TestNetwork, Ed25519Pub, StaticCommittee<DEntryState>>
#[macro_export]
macro_rules! cross_tests {
    // reduce networks -> individual network modules
    ([ $NETWORK:tt $($NETWORKS:tt)* ], [ $($STORAGES:tt)+ ], [ $($BLOCKS:tt)+ ], [ $($STATES:tt)+ ], $fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt, args: $($args:tt)*) => {
        #[ macro_use ]
        #[ allow(non_snake_case) ]
        mod $NETWORK {
            use $crate::*;
            cross_tests!($NETWORK, [ $($STORAGES)+ ], [ $($BLOCKS)+ ], [ $($STATES)+ ], $fn_name, $e, keep: $keep, slow: $slow, args: $($args)*);
        }
        cross_tests!([ $($NETWORKS)*  ], [ $($STORAGES)+ ], [ $($BLOCKS)+ ], [ $($STATES)+ ], $fn_name, $e, keep: $keep, slow: $slow, args: $($args)* );
    };
    // catchall for empty network list (base case)
    ([  ], [ $($STORAGE:tt)+ ], [ $($BLOCKS:tt)+ ], [  $($STATES:tt)*  ], $fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt, args: $($args:tt)*) => {
    };
    // reduce storages -> individual storage modules
    ($NETWORK:tt, [ $STORAGE:tt $($STORAGES:tt)* ], [ $($BLOCKS:tt)+ ], [ $($STATES:tt)+ ], $fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt, args: $($args:tt)*) => {
        #[ macro_use ]
        #[ allow(non_snake_case) ]
        mod $STORAGE {
            use $crate::*;
            cross_tests!($NETWORK, $STORAGE, [ $($BLOCKS)+ ], [ $($STATES)+ ], $fn_name, $e, keep: $keep, slow: $slow, args: $($args)*);
        }
        cross_tests!($NETWORK, [ $($STORAGES),* ], [ $($BLOCKS),+ ], [ $($STATES),+ ], $fn_name, $e, keep: $keep, slow: $slow, args: $($args)*);
    };
    // catchall for empty storage list (base case)
    ($NETWORK:tt, [  ], [ $($BLOCKS:tt)+ ], [  $($STATES:tt)*  ], $fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt, args: $($args:tt)*) => {
    };
    // reduce blocks -> individual block modules
    ($NETWORK:tt, $STORAGE:tt, [ $BLOCK:tt $($BLOCKS:tt)* ], [ $($STATES:tt)+ ], $fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt, args: $($args:tt)*) => {
        #[ macro_use ]
        #[ allow(non_snake_case) ]
        mod $BLOCK {
            use $crate::*;
            cross_tests!($NETWORK, $STORAGE, $BLOCK, [ $($STATES)+ ], $fn_name, $e, keep: $keep, slow: $slow, args: $($args)*);
        }
        cross_tests!($NETWORK, $STORAGE, [ $($BLOCKS),* ], [ $($STATES),+ ], $fn_name, $e, keep: $keep, slow: $slow, args: $($args)*);
    };
    // catchall for empty block list (base case)
    ($NETWORK:tt, $STORAGE:tt, [  ], [  $($STATES:tt)*  ], $fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt, args: $($args:tt)*) => {
    };
    // reduce states -> individual state modules
    ($NETWORK:tt, $STORAGE:tt, $BLOCK:tt, [ $STATE:tt $( $STATES:tt)* ], $fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt, args: $($args:tt)*) => {
        #[ macro_use ]
        #[ allow(non_snake_case) ]
        mod $STATE {
            use $crate::*;
            cross_tests!($NETWORK, $STORAGE, $BLOCK, $STATE, $fn_name, $e, keep: $keep, slow: $slow, args: $($args)*);
        }
        cross_tests!($NETWORK, $STORAGE, $BLOCK, [ $($STATES)* ], $fn_name, $e, keep: $keep, slow: $slow, args: $($args)*);
    };
    // catchall for empty state list (base case)
    ($NETWORK:tt, $STORAGE:tt, $BLOCK:tt, [  ], $fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt, args: $($args:tt)*) => {
    };
    // base reduction
    // NOTE: unclear why `tt` is needed instead of `ty`
    ($NETWORK:tt, $STORAGE:tt, $BLOCK:tt, $STATE:tt, $fn_name:ident, $e:expr, keep: $keep:tt, slow: false, args: $($args:tt)*) => {

        type TestType = $crate::TestDescription<
            common::StaticCommitteeTestTypes,
            hotshot_testing::TestNodeImpl<
                common::StaticCommitteeTestTypes,
                hotshot_types::data::ValidatingLeaf<common::StaticCommitteeTestTypes>,
                hotshot_types::data::ValidatingProposal<
                    common::StaticCommitteeTestTypes,
                    hotshot_types::data::ValidatingLeaf<common::StaticCommitteeTestTypes>
                >,
                hotshot_types::message::QuorumVote<common::StaticCommitteeTestTypes, hotshot_types::data::ValidatingLeaf<common::StaticCommitteeTestTypes>>,
                $NETWORK<
                    common::StaticCommitteeTestTypes,
                    hotshot_types::data::ValidatingProposal<
                        common::StaticCommitteeTestTypes,
                        hotshot_types::data::ValidatingLeaf<common::StaticCommitteeTestTypes>
                    >,
                    hotshot_types::message::QuorumVote<common::StaticCommitteeTestTypes,hotshot_types::data::ValidatingLeaf<common::StaticCommitteeTestTypes>>,
                    hotshot::traits::election::static_committee::StaticCommittee<
                        common::StaticCommitteeTestTypes,
                        hotshot_types::data::ValidatingLeaf<common::StaticCommitteeTestTypes>
                    >
                >,
                $STORAGE<common::StaticCommitteeTestTypes, hotshot_types::data::ValidatingLeaf<common::StaticCommitteeTestTypes>>,
                hotshot::traits::election::static_committee::StaticCommittee<common::StaticCommitteeTestTypes, hotshot_types::data::ValidatingLeaf<common::StaticCommitteeTestTypes>>
            >
        >;
        cross_test!(TestType, $fn_name, $e, keep: $keep, slow: false, args: $($args)*);
    };
    // base reduction
    // NOTE: unclear why `tt` is needed instead of `ty`
    ($NETWORK:tt, $STORAGE:tt, $BLOCK:tt, $STATE:tt, $fn_name:ident, $e:expr, keep: $keep:tt, slow: true, args: $($args:tt)*) => {
        #[cfg(feature = "slow-tests")]
        type TestType = $crate::TestDescription<
            hotshot_testing::TestNodeImpl<
                $STATE,
                $STORAGE<$STATE>,
                $NETWORK,
                hotshot_types::traits::signature_key::ed25519::Ed25519Pub,
                hotshot::traits::election::static_committee::StaticCommittee<$STATE>
            >
        >;

        cross_test!(TestType, $fn_name, $e, keep: $keep, slow: true, args: $($args)*);
    };
}

/// Macro to generate tests for all types based on a description
/// Arguments:
/// - $fn_name: a identifier for the outermost test module
/// - $expr: a TestDescription for the test
/// - $keep:
///   - true is a noop
///   - false forces test to be ignored
#[macro_export]
macro_rules! cross_all_types {
    ($fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt) => {
        #[cfg(test)]
        #[macro_use]
        pub mod $fn_name {
            use $crate::*;

            cross_tests!(
                [ MemoryCommChannel ],
                [ MemoryStorage ],
                [ VDEntryBlock  ],
                [ VDEntryState ],
                $fn_name,
                $e,
                keep: $keep,
                slow: $slow,
                args:
                );
        }
    };
}

/// Macro to generate property-based tests for all types based on a description
/// Arguments:
/// - $fn_name: a identifier for the outermost test module
/// - $expr: a TestDescription for the test
/// - $keep:
///   - true is a noop
///   - false forces test to be ignored
/// - $args: list of arguments to fuzz over. The syntax of these must match
///          function arguments in the same style as
///          <https://docs.rs/proptest/latest/proptest/macro.proptest.html>
///          these arguments are available for usage in $expr
#[macro_export]
macro_rules! cross_all_types_proptest {
    ($fn_name:ident, $e:expr, keep: $keep:tt, slow: $slow:tt, args: $($args:tt)+) => {
        #[cfg(test)]
        #[macro_use]
        pub mod $fn_name {
            use $crate::*;

            cross_tests!(
                [ MemoryCommChannel ],
                [ MemoryStorage ], // AtomicStorage
                [ VDEntryBlock  ],
                [ VDEntryState ],
                $fn_name,
                $e,
                keep: $keep,
                slow: $slow,
                args: $($args)+
                );
        }
    };
}
