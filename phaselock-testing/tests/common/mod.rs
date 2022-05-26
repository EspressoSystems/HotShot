#![allow(dead_code)]

use async_std::task::block_on;
use either::Either;
use phaselock::traits::implementations::{Libp2pNetwork, MemoryStorage};
use phaselock::traits::{BlockContents, NetworkingImplementation, State, Storage};
use phaselock::types::Message;
use phaselock::{
    demos::dentry::{DEntryBlock, State as DemoState, Transaction},
    traits::{implementations::MemoryNetwork, NetworkReliability},
    PhaseLockConfig,
};
use phaselock_testing::{ConsensusRoundError, Round, RoundResult, TestLauncher, TestRunner};
use phaselock_types::traits::network::TestNetworkingImplementation;
use phaselock_types::traits::state::TestState;
use phaselock_utils::test_util::{setup_backtrace, setup_logging};

use std::collections::HashSet;

use std::sync::Arc;

use Either::{Left, Right};

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
}

pub struct GeneralTestDescription {
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
}

pub struct DetailedTestDescriptionBuilder<
    NETWORK: NetworkingImplementation<Message<BLOCK, BLOCK::Transaction, STATE, N>> + Clone + 'static,
    STORAGE: Storage<BLOCK, STATE, N> + 'static,
    BLOCK: BlockContents<N> + 'static,
    STATE: State<N, Block = BLOCK> + TestState<N> + 'static,
> {
    pub general_info: GeneralTestDescription,

    /// list of rounds
    pub rounds: Option<Vec<Round<NETWORK, STORAGE, BLOCK, STATE>>>,

    /// function to generate the runner
    pub gen_runner: GenRunner<NETWORK, STORAGE, BLOCK, STATE>,
}

impl<
        NETWORK: TestNetworkingImplementation<Message<BLOCK, BLOCK::Transaction, STATE, N>>
            + Clone
            + 'static,
        STORAGE: Storage<BLOCK, STATE, N> + Default + 'static,
        BLOCK: BlockContents<N> + Default + 'static,
        STATE: State<N, Block = BLOCK> + TestState<N> + 'static,
    > TestDescription<NETWORK, STORAGE, BLOCK, STATE>
{
    /// default implementation of generate runner
    pub fn gen_runner(&self) -> TestRunner<NETWORK, STORAGE, BLOCK, STATE> {
        let launcher = TestLauncher::new(self.total_nodes);
        // modify runner to recognize timing params
        let set_timing_params = |a: &mut PhaseLockConfig| {
            a.next_view_timeout = self.timing_config.next_view_timeout;
            a.timeout_ratio = self.timing_config.timeout_ratio;
            a.round_start_delay = self.timing_config.round_start_delay;
            a.start_delay = self.timing_config.start_delay;
        };

        // create reliability to pass into runner
        let _reliability = self.network_reliability.clone();

        // create runner from launcher
        launcher
            // insert timing parameters
            .modify_default_config(set_timing_params)
            .launch()
    }
    /// execute a consensus test based on
    /// `Self`
    /// total_nodes: num nodes to run with
    /// txn_ids: vec of vec of transaction ids to send each round
    pub async fn execute(&self) -> Result<(), ConsensusRoundError> {
        setup_logging();
        setup_backtrace();

        let mut runner = match self.gen_runner {
            Some(ref gen_runner) => (gen_runner.clone())(self),
            None => self.gen_runner(),
        };

        // configure nodes/timing

        runner.add_nodes(self.start_nodes).await;

        for node in runner.nodes() {
            node.is_ready().await;
        }

        let len = self.rounds.len() as u64;
        runner.with_rounds(self.rounds.clone());

        runner
            .execute_rounds(len, self.failure_threshold as u64)
            .await
            .unwrap();

        Ok(())
    }
}

impl GeneralTestDescription {
    pub fn build<
        NETWORK: NetworkingImplementation<Message<BLOCK, BLOCK::Transaction, STATE, N>> + Clone + 'static,
        STORAGE: Storage<BLOCK, STATE, N> + 'static,
        BLOCK: BlockContents<N> + 'static,
        STATE: State<N, Block = BLOCK> + TestState<N> + 'static,
    >(
        self,
    ) -> TestDescription<NETWORK, STORAGE, BLOCK, STATE> {
        DetailedTestDescriptionBuilder {
            general_info: self,
            rounds: None,
            gen_runner: None,
        }
        .build()
    }
}

impl<
        NETWORK: NetworkingImplementation<Message<BLOCK, BLOCK::Transaction, STATE, N>> + Clone + 'static,
        STORAGE: Storage<BLOCK, STATE, N> + 'static,
        BLOCK: BlockContents<N> + 'static,
        STATE: State<N, Block = BLOCK> + TestState<N> + 'static,
    > DetailedTestDescriptionBuilder<NETWORK, STORAGE, BLOCK, STATE>
{
    pub fn build(self) -> TestDescription<NETWORK, STORAGE, BLOCK, STATE> {
        let timing_config = TimingData {
            next_view_timeout: self.general_info.next_view_timeout,
            timeout_ratio: self.general_info.timeout_ratio,
            round_start_delay: self.general_info.round_start_delay,
            start_delay: self.general_info.start_delay,
        };

        let rounds = if let Some(rounds) = self.rounds {
            rounds
        } else {
            self.default_populate_rounds()
        };

        // let gen_runner =

        TestDescription {
            rounds,
            gen_runner: self.gen_runner,
            timing_config,
            network_reliability: self.general_info.network_reliability,
            total_nodes: self.general_info.total_nodes,
            start_nodes: self.general_info.start_nodes,
            failure_threshold: self.general_info.failure_threshold,
        }
    }
}

/// Description of a test. Contains all metadata necessary to execute test
pub struct TestDescription<
    NETWORK: NetworkingImplementation<Message<BLOCK, BLOCK::Transaction, STATE, N>> + Clone + 'static,
    STORAGE: Storage<BLOCK, STATE, N> + 'static,
    BLOCK: BlockContents<N> + 'static,
    STATE: State<N, Block = BLOCK> + TestState<N> + 'static,
> {
    /// TODO unneeded (should be sufficient to have gen runner)
    /// the ronds to run for the test
    pub rounds: Vec<Round<NETWORK, STORAGE, BLOCK, STATE>>,
    /// function to create a [`TestRunner`]
    pub gen_runner: GenRunner<NETWORK, STORAGE, BLOCK, STATE>,
    /// timing information applied to phaselocks
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
}

/// type alias for generating a [`TestRunner`]
pub type GenRunner<NETWORK, STORAGE, BLOCK, STATE> = Option<
    Arc<
        dyn Fn(
            &TestDescription<NETWORK, STORAGE, BLOCK, STATE>,
        ) -> TestRunner<NETWORK, STORAGE, BLOCK, STATE>,
    >,
>;

/// type alias for doing setup for a consensus round
pub type TestSetup<NETWORK, STORAGE, BLOCK, STATE> = Vec<
    Arc<
        dyn Fn(
            &mut TestRunner<NETWORK, STORAGE, BLOCK, STATE>,
        ) -> Vec<<BLOCK as BlockContents<N>>::Transaction>,
    >,
>;

/// type alias for the typical network we use
pub type TestNetwork = MemoryNetwork<Message<DEntryBlock, Transaction, DemoState, N>>;
/// type alias for in memory storage we use
pub type TestStorage = MemoryStorage<DEntryBlock, DemoState, N>;
/// type alias for the test transaction type
pub type TestTransaction = <DEntryBlock as BlockContents<N>>::Transaction;
/// type alias for the test runner type
pub type AppliedTestRunner = TestRunner<TestNetwork, TestStorage, DEntryBlock, DemoState>;
/// type alias for the result of a test round
pub type TestRoundResult = RoundResult<DEntryBlock, DemoState>;

// FIXME THIS is why we need to split up metadat and anonymous functions
impl Default for GeneralTestDescription {
    /// by default, just a single round
    fn default() -> Self {
        Self {
            total_nodes: 5,
            start_nodes: 5,
            num_succeeds: 1,
            failure_threshold: 0,
            txn_ids: Right(1),
            next_view_timeout: 1000,
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            ids_to_shut_down: Vec::new(),
            network_reliability: None,
        }
    }
}

pub type TestLibp2pNetwork = Libp2pNetwork<Message<DEntryBlock, Transaction, DemoState, N>>;

/// given a description of rounds, generates such rounds
/// args
/// * `shut_down_ids`: vector of ids to shut down each round
/// * `submitter_ids`: vector of ids to submit txns to each round
/// * `num_rounds`: total number of rounds to generate
pub fn default_submitter_id_to_round<
    NETWORK: NetworkingImplementation<Message<BLOCK, BLOCK::Transaction, STATE, N>> + Clone + 'static,
    STORAGE: Storage<BLOCK, STATE, N> + 'static,
    BLOCK: BlockContents<N> + 'static,
    STATE: State<N, Block = BLOCK> + TestState<N> + 'static,
>(
    mut shut_down_ids: Vec<HashSet<u64>>,
    submitter_ids: Vec<Vec<u64>>,
    num_rounds: u64,
) -> TestSetup<NETWORK, STORAGE, BLOCK, STATE> {
    // make sure the lengths match so zip doesn't spit out none
    if shut_down_ids.len() < submitter_ids.len() {
        shut_down_ids.append(&mut vec![
            HashSet::new();
            submitter_ids.len() - shut_down_ids.len()
        ])
    }

    let mut rounds: TestSetup<NETWORK, STORAGE, BLOCK, STATE> = Vec::new();
    for (round_ids, shutdown_ids) in submitter_ids.into_iter().zip(shut_down_ids.into_iter()) {
        let run_round = move |runner: &mut TestRunner<NETWORK, STORAGE, BLOCK, STATE>| -> Vec<BLOCK::Transaction> {
            for id in shutdown_ids.clone() {
                block_on(runner.shutdown(id)).unwrap();
            }
            let mut txns = Vec::new();
            for id in round_ids.clone() {
                let new_txn = runner.add_random_transaction(Some(id as usize)).unwrap();
                txns.push(new_txn);
            }
            txns
        };
        rounds.push(Arc::new(run_round));
    }

    // if there are not enoough rounds, add some autogenerated ones to keep liveness
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
pub fn default_randomized_ids_to_round<
    NETWORK: NetworkingImplementation<Message<BLOCK, BLOCK::Transaction, STATE, N>> + Clone + 'static,
    STORAGE: Storage<BLOCK, STATE, N> + 'static,
    BLOCK: BlockContents<N> + 'static,
    STATE: State<N, Block = BLOCK> + TestState<N> + 'static,
>(
    shut_down_ids: Vec<HashSet<u64>>,
    num_rounds: u64,
    txns_per_round: u64,
) -> TestSetup<NETWORK, STORAGE, BLOCK, STATE> {
    let mut rounds: TestSetup<NETWORK, STORAGE, BLOCK, STATE> = Vec::new();

    for round_idx in 0..num_rounds {
        let to_kill = shut_down_ids.get(round_idx as usize).cloned();
        let run_round = move |runner: &mut TestRunner<NETWORK, STORAGE, BLOCK, STATE>| {
            if let Some(to_shut_down) = to_kill.clone() {
                for idx in to_shut_down {
                    block_on(runner.shutdown(idx)).unwrap();
                }
            }

            runner
                .add_random_transactions(txns_per_round as usize)
                .unwrap()
        };

        rounds.push(Arc::new(run_round));
    }

    rounds
}

impl<
        NETWORK: NetworkingImplementation<Message<BLOCK, BLOCK::Transaction, STATE, N>> + Clone + 'static,
        STORAGE: Storage<BLOCK, STATE, N> + 'static,
        BLOCK: BlockContents<N> + 'static,
        STATE: State<N, Block = BLOCK> + TestState<N> + 'static,
    > DetailedTestDescriptionBuilder<NETWORK, STORAGE, BLOCK, STATE>
{
    /// create rounds of consensus based on the data in `self`
    pub fn default_populate_rounds(&self) -> Vec<Round<NETWORK, STORAGE, BLOCK, STATE>> {
        // total number of rounds to be prepared to run assuming there may be failures
        let total_rounds = self.general_info.num_succeeds + self.general_info.failure_threshold;

        let safety_check_post = move |runner: &TestRunner<NETWORK, STORAGE, BLOCK, STATE>,
                                      results: RoundResult<BLOCK, STATE>|
              -> Result<(), ConsensusRoundError> {
            tracing::info!(?results);
            // this check is rather strict:
            // 1) all nodes have the SAME state
            // 2) no nodes failed
            async_std::task::block_on(runner.validate_node_states());
            assert!(
                results.failures.is_empty(),
                "Failing nodes: {:?}",
                results.failures
            );
            Ok(())
        };

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
            .map(|setup| Round {
                setup_round: Some(setup),
                safety_check_post: Some(Arc::new(safety_check_post)),
                safety_check_pre: None,
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

#[macro_export]
macro_rules! gen_inner_fn {
    ($TEST_TYPE:ty, $fn_name:ident, $e:expr) => {
        let description: $crate::GeneralTestDescription = $e;
        let built: $TEST_TYPE = description.build();
        built.execute().await.unwrap()
    };
}

#[macro_export]
macro_rules! cross_test {
    // base case
    ($TEST_TYPE:ty, $fn_name:ident, $e:expr, true) => {
        #[async_std::test]
        #[instrument]
        async fn test() {
            gen_inner_fn!($TEST_TYPE, $fn_name, $e);
        }
    };
    ($TEST_TYPE:ty, $fn_name:ident, $e:expr, false) => {
        #[async_std::test]
        #[instrument]
        #[ignore]
        async fn test() {
            gen_inner_fn!($TEST_TYPE, $fn_name, $e);
        }
    };
}

#[macro_export]
macro_rules! cross_tests {
    // base case
    ([$NETWORK:tt], [$STORAGE:tt], [$BLOCK:tt], [$STATE:tt], $fn_name:ident, $e:expr, $keep:tt) => {
        type TestType = $crate::TestDescription<
            $NETWORK<phaselock::types::Message<$BLOCK, <$BLOCK as phaselock::traits::BlockContents< { phaselock::H_256 } > > ::Transaction, $STATE, { phaselock::H_256 } >>,
            $STORAGE<$BLOCK, $STATE, { phaselock::H_256 } >,
            $BLOCK,
            $STATE
        >;
        cross_test!(TestType, $fn_name, $e, $keep);
    };
    // reductions
    ([$NETWORK:tt, $($NETWORKS:tt),+], [$($STORAGES:tt),+], [$($BLOCKS:tt),+], [$($STATES:tt),+], $fn_name:ident, $e:expr, $keep:tt) => {
        #[cfg(test)]
        #[macro_use]
        #[allow(non_snake_case)]
        pub mod $NETWORK {
            use crate::*;
            cross_tests!([$NETWORK], [$($STORAGES),+], [$($BLOCKS),+], [$($STATES),+], $fn_name, $e, $keep);
        }
        cross_tests!([$($NETWORKS),+ ], [$($STORAGES),+], [$($BLOCKS),+], [$($STATES),+], $fn_name, $e, $keep);
    };
    ([$($NETWORKS:tt),+], [$STORAGE:tt, $($STORAGES:tt),+], [$($BLOCKS:tt),+], [$($STATES:tt),+], $fn_name:ident, $e:expr, $keep:tt) => {
        #[cfg(test)]
        #[macro_use]
        #[allow(non_snake_case)]
        pub mod $STORAGE {
            use crate::*;
            cross_tests!([$($NETWORKS),+], [$STORAGE], [$($BLOCKS),+], [$($STATES),+], $fn_name, $e, $keep);
        }
        cross_tests!([$($NETWORKS),+ ], [$($STORAGES),+], [$($BLOCKS),+], [$($STATES),+], $fn_name, $e, $keep);
    };
    ([$($NETWORKS:tt),+], [$($STORAGES:tt),+], [$BLOCK:tt, $($BLOCKS:tt),+], [$($STATES:tt),+], $fn_name:ident, $e:expr, $keep:tt) => {
        #[cfg(test)]
        #[macro_use]
        #[allow(non_snake_case)]
        pub mod $BLOCK {
            use crate::*;
            cross_tests!([$($NETWORKS),+], [$($STORAGES),+], [$BLOCK], [$($STATES),+], $fn_name, $e, $keep);
        }
        cross_tests!([$($NETWORKS),+ ], [$($STORAGES),+], [$($BLOCKS),+], [$($STATES),+], $fn_name, $e, $keep);
    };
    ([$($NETWORKS:tt),+], [$($STORAGES:tt),+], [$($BLOCKS:tt),+], [$STATE:tt, $($STATES:tt),+], $fn_name:ident, $e:expr, $keep:tt) => {
        #[cfg(test)]
        #[macro_use]
        #[allow(non_snake_case)]
        pub mod $STATE {
            use crate::*;
            cross_tests!([$($NETWORKS),+], [$($STORAGES),+], [$($BLOCKS),+], [$STATE], $fn_name, $e, $keep);
        }
        cross_tests!([$($NETWORKS),+ ], [$($STORAGES),+], [$($BLOCKS),+], [$($STATES),+], $fn_name, $e, $keep);
    };
}

#[macro_export]
macro_rules! cross_all_types {
    ($fn_name:ident, $e:expr, $keep:tt) => {
        #[cfg(test)]
        #[macro_use]
        pub mod $fn_name {
            use crate::*;

            cross_tests!(
                [MemoryNetwork, Libp2pNetwork],
                [MemoryStorage, AtomicStorage],
                [DEntryBlock],
                [State],
                $fn_name,
                $e,
                $keep
            );
        }
    };
}
