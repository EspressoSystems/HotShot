#![allow(dead_code)]

use async_std::task::block_on;
use either::Either;
use phaselock::traits::implementations::MemoryStorage;
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
    ConsensusRoundError, RoundResult, TestLauncher, TestRunner, TransactionSnafu,
};
use phaselock_utils::test_util::{setup_backtrace, setup_logging};

use snafu::ResultExt;
use std::collections::HashSet;

use std::sync::Arc;

use Either::{Left, Right};

pub const N: usize = 32_usize;

/// Description of a consensus test
pub struct TestDescription<
    NETWORK: NetworkingImplementation<Message<DEntryBlock, Transaction, DemoState, N>> + Clone + 'static,
    STORAGE: Storage<DEntryBlock, DemoState, N> + 'static,
> {
    /// Total number of nodes in the test
    pub total_nodes: usize,
    /// nodes available at start
    pub start_nodes: usize,
    /// number of successful/passing rounds required for test to pass
    pub num_rounds: usize,
    /// max number of failing rounds before test is failed
    pub failure_threshold: usize,
    /// Either a list of transactions submitter indexes to
    /// submit a random txn with each round,
    /// or (`num_rounds`, `tx_per_round`)
    /// `tx_per_round` transactions are submitted each round
    /// to random nodes for `num_rounds + failure_threshold`
    pub txn_ids: Either<Vec<Vec<u64>>, (usize, usize)>,
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
    pub setup_round: TestSetup<NETWORK, STORAGE>,
    pub safety_check_pre: TestSafetyCheckPre<NETWORK, STORAGE>,
    pub safety_check_post: TestSafetyCheckPost<NETWORK, STORAGE>,
    pub gen_runner: GenRunner<NETWORK, STORAGE>,
}

pub type GenRunner<NETWORK, STORAGE> = Option<
    Arc<
        dyn Fn(
            &TestDescription<NETWORK, STORAGE>,
        ) -> TestRunner<NETWORK, STORAGE, DEntryBlock, DemoState>,
    >,
>;

pub type TestSafetyCheckPre<NETWORK, STORAGE> = Vec<
    Arc<
        dyn Fn(
            &TestRunner<NETWORK, STORAGE, DEntryBlock, DemoState>,
        ) -> Result<(), ConsensusRoundError>,
    >,
>;
pub type TestSafetyCheckPost<NETWORK, STORAGE> = Vec<
    Arc<
        dyn Fn(
            &TestRunner<NETWORK, STORAGE, DEntryBlock, DemoState>,
            RoundResult<DEntryBlock, DemoState>,
        ) -> Result<(), ConsensusRoundError>,
    >,
>;

pub type TestSetup<NETWORK, STORAGE> = Vec<
    Arc<
        dyn Fn(
            &mut TestRunner<NETWORK, STORAGE, DEntryBlock, DemoState>,
        ) -> Vec<<DEntryBlock as BlockContents<N>>::Transaction>,
    >,
>;

pub type TestNetwork = MemoryNetwork<Message<DEntryBlock, Transaction, DemoState, N>>;
pub type TestStorage = MemoryStorage<DEntryBlock, DemoState, N>;
pub type TestTransaction = <DEntryBlock as BlockContents<N>>::Transaction;
pub type AppliedTestRunner = TestRunner<TestNetwork, TestStorage, DEntryBlock, DemoState>;
pub type TestRoundResult = RoundResult<DEntryBlock, DemoState>;

/// the default safety check that asserts node blocks and states
/// match after a round of consensus
/// FIXME Once <https://github.com/EspressoSystems/phaselock/pull/108> is merged
/// replace this with that
pub fn default_check(results: TestRoundResult) -> Result<(), ConsensusRoundError> {
    // Check that we were successful on all nodes
    if results.failures.keys().len() != 0 {
        return Err(ConsensusRoundError::SafetyFailed {
            description: format!(
                "Not all nodes reached a decision. Decided: {:?}, failed: {:?}",
                results.results.keys().len(),
                results.failures.keys().len()
            ),
        });
    }

    // probably redundant sanity check
    if results.results.keys().len() < 5 {
        return Err(ConsensusRoundError::SafetyFailed {
            description: "No nodes in consensus. Can't run round without at least 5 nodes."
                .to_string(),
        });
    }
    let mut result_iter = results.results.iter();

    // prior asserts would have failed if there were no keys
    // so unwrap will not fail
    let (_id, (s_test, b_test)) = result_iter.next().unwrap();

    for (_, (state, block)) in result_iter {
        if state[0] != s_test[0] {
            return Err(ConsensusRoundError::SafetyFailed {
                description: "State doesn't match".to_string(),
            });
        }

        if block[0] != b_test[0] {
            return Err(ConsensusRoundError::SafetyFailed {
                description: "State doesn't match".to_string(),
            });
        }
    }
    if b_test[0].transactions.is_empty() {
        return Err(ConsensusRoundError::SafetyFailed {
            description: "No txns submitted this round".to_string(),
        });
    }
    if b_test[0].transactions != results.txns {
        return Err(ConsensusRoundError::SafetyFailed {
            description: "Committed doesn't match what submitted.".to_string(),
        });
    }
    Ok(())
}

impl Default for TestDescription<TestNetwork, TestStorage> {
    /// by default, just a single round
    fn default() -> Self {
        Self {
            total_nodes: 5,
            start_nodes: 5,
            num_rounds: 1,
            failure_threshold: 0,
            txn_ids: Right((1, 1)),
            next_view_timeout: 1000,
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            ids_to_shut_down: Vec::new(),
            network_reliability: None,
            setup_round: Vec::new(),
            safety_check_pre: Vec::new(),
            safety_check_post: Vec::new(),
            gen_runner: Some(Arc::new(gen_runner_default)),
        }
    }
}

/// args
/// * `shut_down_ids`: vector of ids to shut down each round
pub fn default_submitter_id_to_round<
    NETWORK: NetworkingImplementation<Message<DEntryBlock, Transaction, DemoState, N>> + Clone + 'static,
    STORAGE: Storage<DEntryBlock, DemoState, N> + 'static,
>(
    mut shut_down_ids: Vec<HashSet<u64>>,
    submitter_ids: Vec<Vec<u64>>,
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

    rounds
}

/// generate transactions
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
    > TestDescription<NETWORK, STORAGE>
{
    pub fn default_populate_rounds(self) -> Self {
        let total_rounds = self.num_rounds + self.failure_threshold;

        let pre_checks = vec![];
        let safety_check_post =
            move |_runner: &TestRunner<NETWORK, STORAGE, DEntryBlock, DemoState>,
                  results: TestRoundResult|
                  -> Result<(), ConsensusRoundError> { default_check(results) };

        let rounds = match self.txn_ids.clone() {
            Left(l) => default_submitter_id_to_round(self.ids_to_shut_down.clone(), l),
            Right((num_rounds, tx_per_round)) => default_randomized_ids_to_round(
                self.ids_to_shut_down.clone(),
                num_rounds as u64,
                tx_per_round as u64,
            ),
        };

        TestDescription {
            setup_round: rounds,
            safety_check_pre: pre_checks,
            safety_check_post: vec![Arc::new(safety_check_post); total_rounds],
            ..self
        }
    }

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
        runner.with_rounds_setup(self.setup_round.clone());
        runner.with_safety_check_pre(self.safety_check_pre.clone());
        runner.with_safety_check_post(self.safety_check_post.clone());

        runner
            .execute_rounds(self.num_rounds as u64, self.failure_threshold as u64)
            .await
            .unwrap();

        Ok(())
    }
}

pub fn gen_runner_default(
    desc: &TestDescription<TestNetwork, TestStorage>,
) -> TestRunner<TestNetwork, TestStorage, DEntryBlock, DemoState> {
    let launcher = TestLauncher::new(desc.total_nodes);

    // modify runner to recognize timing params
    let set_timing_params = |a: &mut PhaseLockConfig| {
        a.next_view_timeout = desc.next_view_timeout;
        a.timeout_ratio = desc.timeout_ratio;
        a.round_start_delay = desc.round_start_delay;
        a.start_delay = desc.start_delay;
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
            move |pubkey| MemoryNetwork::new(pubkey, master_map.clone(), reliability.clone())
        })
        .launch()
}

pub fn get_threshold(num_nodes: u64) -> u64 {
    ((num_nodes * 2) / 3) + 1
}

pub fn get_tolerance(num_nodes: u64) -> u64 {
    num_nodes - get_threshold(num_nodes)
}
