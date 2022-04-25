#![allow(dead_code)]

use async_std::task::block_on;
use either::Either;
use phaselock::traits::implementations::{Libp2pNetwork, MemoryStorage};
use phaselock::traits::{BlockContents, NetworkingImplementation, Storage};
use phaselock::types::Message;
use phaselock::{
    demos::dentry::{DEntryBlock, State as DemoState, Transaction},
    traits::{
        implementations::{MasterMap, MemoryNetwork},
        NetworkReliability,
    },
    PhaseLockConfig,
};
use phaselock_testing::{
    ConsensusRoundError, Round, RoundResult, TestLauncher, TestRunner, TransactionSnafu,
};
use phaselock_utils::test_util::{setup_backtrace, setup_logging};

use snafu::ResultExt;
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

/// Description of a consensus test
pub struct TestDescriptionBuilder<
    NETWORK: NetworkingImplementation<Message<DEntryBlock, Transaction, DemoState, N>> + Clone + 'static,
    STORAGE: Storage<DEntryBlock, DemoState, N> + 'static,
> {
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
    pub rounds: Option<Vec<Round<NETWORK, STORAGE, DEntryBlock, DemoState>>>,
    /// function to generate the runner
    pub gen_runner: GenRunner<NETWORK, STORAGE>,
}

impl<
        NETWORK: NetworkingImplementation<Message<DEntryBlock, Transaction, DemoState, N>> + Clone + 'static,
        STORAGE: Storage<DEntryBlock, DemoState, N> + 'static,
    > TestDescription<NETWORK, STORAGE>
{
    /// execute a consensus test based on
    /// `Self`
    /// total_nodes: num nodes to run with
    /// txn_ids: vec of vec of transaction ids to send each round
    pub async fn execute(&self) -> Result<(), ConsensusRoundError> {
        setup_logging();
        setup_backtrace();

        let mut runner = (self.gen_runner.clone().unwrap())(self);

        // configure nodes/timing

        runner.add_nodes(self.start_nodes).await;
        let len = self.rounds.len() as u64;
        runner.with_rounds(self.rounds.clone());

        runner
            .execute_rounds(len, self.failure_threshold as u64)
            .await
            .unwrap();

        Ok(())
    }
}

impl<
        NETWORK: NetworkingImplementation<Message<DEntryBlock, Transaction, DemoState, N>> + Clone + 'static,
        STORAGE: Storage<DEntryBlock, DemoState, N> + 'static,
    > TestDescriptionBuilder<NETWORK, STORAGE>
{
    pub fn build(self) -> TestDescription<NETWORK, STORAGE> {
        let timing_config = TimingData {
            next_view_timeout: self.next_view_timeout,
            timeout_ratio: self.timeout_ratio,
            round_start_delay: self.round_start_delay,
            start_delay: self.start_delay,
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
            network_reliability: self.network_reliability,
            total_nodes: self.total_nodes,
            start_nodes: self.start_nodes,
            failure_threshold: self.failure_threshold,
        }
    }
}

/// Description of a test. Contains all metadata necessary to execute test
pub struct TestDescription<
    NETWORK: NetworkingImplementation<Message<DEntryBlock, Transaction, DemoState, N>> + Clone + 'static,
    STORAGE: Storage<DEntryBlock, DemoState, N> + 'static,
> {
    /// the ronds to run for the test
    pub rounds: Vec<Round<NETWORK, STORAGE, DEntryBlock, DemoState>>,
    /// function to create a [`TestRunner`]
    pub gen_runner: GenRunner<NETWORK, STORAGE>,
    /// timing information applied to phaselocks
    pub timing_config: TimingData,
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
pub type GenRunner<NETWORK, STORAGE> = Option<
    Arc<
        dyn Fn(
            &TestDescription<NETWORK, STORAGE>,
        ) -> TestRunner<NETWORK, STORAGE, DEntryBlock, DemoState>,
    >,
>;

/// type alias for doing setup for a consensus round
pub type TestSetup<NETWORK, STORAGE> = Vec<
    Arc<
        dyn Fn(
            &mut TestRunner<NETWORK, STORAGE, DEntryBlock, DemoState>,
        ) -> Vec<<DEntryBlock as BlockContents<N>>::Transaction>,
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

impl Default for TestDescriptionBuilder<TestNetwork, TestStorage> {
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
            rounds: None,
            gen_runner: Some(Arc::new(gen_runner_default)),
        }
    }
}

pub type TestLibp2pNetwork = Libp2pNetwork<Message<DEntryBlock, Transaction, DemoState, N>>;

impl Default for TestDescriptionBuilder<TestLibp2pNetwork, TestStorage> {
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
            rounds: None,
            gen_runner: None,
        }
    }
}

/// given a description of rounds, generates such rounds
/// args
/// * `shut_down_ids`: vector of ids to shut down each round
/// * `submitter_ids`: vector of ids to submit txns to each round
/// * `num_rounds`: total number of rounds to generate
pub fn default_submitter_id_to_round<
    NETWORK: NetworkingImplementation<Message<DEntryBlock, Transaction, DemoState, N>> + Clone + 'static,
    STORAGE: Storage<DEntryBlock, DemoState, N> + 'static,
>(
    mut shut_down_ids: Vec<HashSet<u64>>,
    submitter_ids: Vec<Vec<u64>>,
    num_rounds: u64,
) -> TestSetup<NETWORK, STORAGE> {
    // make sure the lengths match so zip doesn't spit out none
    if shut_down_ids.len() < submitter_ids.len() {
        shut_down_ids.append(&mut vec![
            HashSet::new();
            submitter_ids.len() - shut_down_ids.len()
        ])
    }

    let mut rounds: TestSetup<NETWORK, STORAGE> = Vec::new();
    for (round_ids, shutdown_ids) in submitter_ids.into_iter().zip(shut_down_ids.into_iter()) {
        let run_round = move |runner: &mut TestRunner<NETWORK, STORAGE, DEntryBlock, DemoState>| -> Vec<<DEntryBlock as BlockContents<N>>::Transaction> {
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
    NETWORK: NetworkingImplementation<Message<DEntryBlock, Transaction, DemoState, N>> + Clone + 'static,
    STORAGE: Storage<DEntryBlock, DemoState, N> + 'static,
>(
    shut_down_ids: Vec<HashSet<u64>>,
    num_rounds: u64,
    txns_per_round: u64,
) -> TestSetup<NETWORK, STORAGE> {
    let mut rounds: TestSetup<NETWORK, STORAGE> = Vec::new();

    for round_idx in 0..num_rounds {
        let to_kill = shut_down_ids.get(round_idx as usize).cloned();
        let run_round = move |runner: &mut TestRunner<NETWORK, STORAGE, DEntryBlock, DemoState>| {
            if let Some(to_shut_down) = to_kill.clone() {
                for idx in to_shut_down {
                    block_on(runner.shutdown(idx)).unwrap();
                }
            }

            runner
                .add_random_transactions(txns_per_round as usize)
                .context(TransactionSnafu)
                .unwrap()
        };

        rounds.push(Arc::new(run_round));
    }

    rounds
}

impl<
        NETWORK: NetworkingImplementation<Message<DEntryBlock, Transaction, DemoState, N>> + Clone + 'static,
        STORAGE: Storage<DEntryBlock, DemoState, N> + 'static,
    > TestDescriptionBuilder<NETWORK, STORAGE>
{
    /// create rounds of consensus based on the data in `self`
    pub fn default_populate_rounds(&self) -> Vec<Round<NETWORK, STORAGE, DEntryBlock, DemoState>> {
        // total number of rounds to be prepared to run assuming there may be failures
        let total_rounds = self.num_succeeds + self.failure_threshold;

        let safety_check_post =
            move |runner: &TestRunner<NETWORK, STORAGE, DEntryBlock, DemoState>,
                  results: TestRoundResult|
                  -> Result<(), ConsensusRoundError> {
                tracing::info!(?results);
                async_std::task::block_on(runner.validate_node_states());
                Ok(())
            };

        let setups = match self.txn_ids.clone() {
            Left(l) => {
                default_submitter_id_to_round(self.ids_to_shut_down.clone(), l, total_rounds as u64)
            }
            Right(tx_per_round) => default_randomized_ids_to_round(
                self.ids_to_shut_down.clone(),
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

/// runner creation function with a bunch of sane defaults
pub fn gen_runner_default(
    desc: &TestDescription<TestNetwork, TestStorage>,
) -> TestRunner<TestNetwork, TestStorage, DEntryBlock, DemoState> {
    let launcher = TestLauncher::<MemoryNetwork<_>, _, _, _>::new(desc.total_nodes);

    // modify runner to recognize timing params
    let set_timing_params = |a: &mut PhaseLockConfig| {
        a.next_view_timeout = desc.timing_config.next_view_timeout;
        a.timeout_ratio = desc.timing_config.timeout_ratio;
        a.round_start_delay = desc.timing_config.round_start_delay;
        a.start_delay = desc.timing_config.start_delay;
    };

    // create reliability to pass into runner
    let reliability = desc.network_reliability.clone();
    // create runner from launcher
    launcher
        // insert timing parameters
        .modify_default_config(set_timing_params)
        // overwrite network to preserve public key, but use a common master_map
        .with_network({
            let master_map = MasterMap::new();
            move |_node_id, pubkey| {
                MemoryNetwork::new(pubkey, master_map.clone(), reliability.clone())
            }
        })
        .launch()
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
