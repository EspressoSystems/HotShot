#![allow(dead_code)]

use either::Either;
use phaselock::traits::implementations::MemoryStorage;
use phaselock::traits::Storage;
use phaselock::types::Message;
use phaselock::{
    demos::dentry::{Addition, DEntryBlock, State, Subtraction, Transaction},
    tc,
    traits::{
        election::StaticCommittee,
        implementations::{MasterMap, MemoryNetwork},
        NetworkReliability, NodeImplementation,
    },
    types::PhaseLockHandle,
    PhaseLock, PhaseLockConfig, PubKey,
};
use phaselock_testing::{
    ConsensusTestError, TestLauncher, TransactionSnafu,
};
use phaselock_utils::test_util::{setup_backtrace, setup_logging};
use serde::{de::DeserializeOwned, Serialize};
use snafu::ResultExt;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{debug, instrument};
use Either::{Left, Right};

/// Description of a consensus test
pub struct TestDescription {
    /// Total number of nodes in the test
    pub total_nodes: usize,
    /// Either a list of transactions submitter indexes to
    /// submit a random txn with each round,
    /// or (`num_rounds`, `tx_per_round`)
    /// `tx_per_round` transactions are submitted each round
    /// to random nodes for `num_rounds`
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
    pub ids_to_shut_down: HashSet<u64>,
    /// Description of the network reliability
    pub network_reliability: Option<Arc<dyn NetworkReliability>>,
    // #[allow(clippy::type_complexity)]
    // pub safety_check: Arc<
    //     dyn FnMut(
    //         &Self,
    //         RoundResult<TestBlock, TestState>
    //     ) -> Result<(), ConsensusTestError>,
    // >,
    // pub before_rounds: Vec<Arc<
    //     dyn FnMut(
    //         &Self,
    //         &mut TestRunner<TestNetwork, TestStorage, TestBlock, TestState>
    //     )>
    // >
}

pub type TestNetwork = MemoryNetwork<Message<DEntryBlock, Transaction, State, 32_usize>>;
pub type TestStorage = MemoryStorage<DEntryBlock, State, 32_usize>;
pub type TestBlock = DEntryBlock;
pub type TestState = State;

/// the default safety check that asserts node blocks and states
/// match after a round of consensus
/// FIXME Once <https://github.com/EspressoSystems/phaselock/pull/108> is merged
/// replace this with that
pub fn default_check(
    test_description: &TestDescription,
    txns: Vec<Transaction>,
    results: HashMap<u64, (Vec<State>, Vec<DEntryBlock>)>,
) -> Result<(), ConsensusTestError> {
    // Check that we were successful on all nodes
    println!(
        "results: {:?}, total nodes: {:?}",
        results, test_description.total_nodes
    );
    assert!(results.keys().len() == test_description.total_nodes);
    // probably redundant sanity check
    assert!(results.keys().len() > 0);
    let mut result_iter = results.iter();

    // prior asserts would have failed
    // so is okay to unwrap
    let (_id, (s_test, b_test)) = result_iter.next().unwrap();

    for (_, (state, block)) in result_iter {
        assert_eq!(state[0], s_test[0]);
        assert_eq!(block[0], b_test[0]);
    }
    println!("All states match");
    assert!(!b_test[0].transactions.is_empty());
    assert_eq!(b_test[0].transactions, txns);
    Ok(())
}

impl Default for TestDescription {
    fn default() -> Self {
        Self {
            total_nodes: 5,
            txn_ids: Right((1, 1)),
            next_view_timeout: 1000,
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            ids_to_shut_down: HashSet::new(),
            network_reliability: None,
        }
    }
}

impl TestDescription {
    /// execute a consensus test based on
    /// `Self`
    /// total_nodes: num nodes to run with
    /// txn_ids: vec of vec of transaction ids to send each round
    pub async fn execute(&self) -> Result<(), ConsensusTestError> {
        setup_logging();
        setup_backtrace();

        // configure nodes/timing

        let launcher = TestLauncher::new(self.total_nodes);

        let f = |a: &mut PhaseLockConfig| {
            a.next_view_timeout = self.next_view_timeout;
            a.timeout_ratio = self.timeout_ratio;
            a.round_start_delay = self.round_start_delay;
            a.start_delay = self.start_delay;
        };

        let reliability = self.network_reliability.clone();
        // create runner from launcher
        let mut runner = launcher
            .modify_default_config(f)
            .with_network({
                let master_map = MasterMap::new();
                move |pubkey| MemoryNetwork::new(pubkey, master_map.clone(), reliability.clone())
            })
            .launch();
        runner.add_nodes(self.total_nodes).await;
        for node in runner.nodes() {
            let qc = node.storage().get_newest_qc().await.unwrap().unwrap();
            assert_eq!(qc.view_number, 0);
        }

        // map `txn_ids` to generated transaction ids
        // do this ahead of time to "fail fast"
        let ids = runner.ids();

        let round_iter: std::vec::IntoIter<Either<Vec<u64>, usize>> = match self.txn_ids {
            Left(ref all_ids) => {
                let mut mapped_txn_ids = Vec::new();
                for round_txn_ids in all_ids {
                    let mut mapped_round_txn_ids = Vec::new();
                    for txn_id in round_txn_ids {
                        let mapped_id =
                            *ids.get(*txn_id as usize)
                                .ok_or(ConsensusTestError::NoSuchNode {
                                    node_ids: ids.clone(),
                                    requested_id: *txn_id,
                                })?;
                        mapped_round_txn_ids.push(mapped_id);
                    }
                    mapped_txn_ids.push(Left(mapped_round_txn_ids));
                }
                mapped_txn_ids.into_iter()
            }
            Right((num_rounds, tx_per_round)) => (0..num_rounds)
                .map(|_| Right(tx_per_round))
                .collect::<Vec<_>>()
                .into_iter(),
        };

        // shut down requested nodes
        for shut_down_id in &self.ids_to_shut_down {
            let mapped_id =
                *ids.get(*shut_down_id as usize)
                    .ok_or(ConsensusTestError::NoSuchNode {
                        node_ids: ids.clone(),
                        requested_id: *shut_down_id,
                    })?;
            runner.shutdown(mapped_id).await?;
        }

        // run rounds
        for txn_senders in round_iter {
            let _txns = match txn_senders {
                Right(num_txns) => runner
                    .add_random_transactions(num_txns)
                    .context(TransactionSnafu)?,
                Left(sender_ids) => {
                    assert!(!sender_ids.is_empty());
                    let mut txns = Vec::new();
                    for txn_sender in sender_ids {
                        let txn = runner
                            .add_random_transaction(Some(txn_sender as usize))
                            .context(TransactionSnafu)?;
                        txns.push(txn);
                    }
                    txns
                }
            };
            // empty round will never finish, so assert that is at least 1 txn
            // FIXME this might error out if round fails no transactions are submitted
            // should submit dummy transactions
            // run round then assert state is correct

            // FIXME
            // let (states, errors) = runner.run_one_round().await;
            // let (states, errors) = todo!();
            // if !errors.is_empty() {
            //     error!("ERRORS RUNNING CONSENSUS, {:?}", errors);
            // }
            //
            // let safety_check = self.safety_check.clone();
            // (*safety_check)(self, txns, states)?;
        }

        println!("SHUTTING DOWN ALL");
        runner.shutdown_all().await;
        println!("Consensus completed\n");
        Ok(())
    }
}

/// generates random transactions with no regard for going negative
/// # Panics when accounts is empty
pub fn random_transactions<R: rand::Rng>(
    mut rng: &mut R,
    num_txns: usize,
    accounts: Vec<String>,
) -> Vec<Transaction> {
    use rand::seq::IteratorRandom;
    let mut txns = Vec::new();
    for _ in 0..num_txns {
        let input_account = accounts.iter().choose(&mut rng).unwrap();
        let output_account = accounts.iter().choose(&mut rng).unwrap();
        let amount = rng.gen_range(0, 100);
        txns.push(Transaction {
            add: Addition {
                account: input_account.to_string(),
                amount,
            },
            sub: Subtraction {
                account: output_account.to_string(),
                amount,
            },
            nonce: rng.gen(),
        })
    }
    txns
}

pub fn get_threshold(num_nodes: u64) -> u64 {
    ((num_nodes * 2) / 3) + 1
}

pub fn get_tolerance(num_nodes: u64) -> u64 {
    num_nodes - get_threshold(num_nodes)
}

/// Gets networking backends of all nodes.
pub async fn get_networkings<
    T: Clone + Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + 'static,
>(
    num_nodes: u64,
    sks: &tc::SecretKeySet,
) -> (Arc<MasterMap<T>>, Vec<(MemoryNetwork<T>, PubKey)>) {
    let master = MasterMap::<T>::new();
    let mut networkings: Vec<(MemoryNetwork<T>, PubKey)> = Vec::new();
    for node_id in 0..num_nodes {
        let pub_key = PubKey::from_secret_key_set_escape_hatch(sks, node_id);
        let network = MemoryNetwork::new(pub_key.clone(), master.clone(), Option::None);
        networkings.push((network, pub_key));
    }
    (master, networkings)
}

/// Provides a random valid transaction from the current state.
pub fn random_transaction<R: rand::Rng>(state: &State, mut rng: &mut R) -> Transaction {
    use rand::seq::IteratorRandom;
    let input_account = state.balances.keys().choose(&mut rng).unwrap();
    let output_account = state.balances.keys().choose(&mut rng).unwrap();
    let amount = rng.gen_range(0, state.balances[input_account]);
    Transaction {
        add: Addition {
            account: output_account.to_string(),
            amount,
        },
        sub: Subtraction {
            account: input_account.to_string(),
            amount,
        },
        nonce: rng.gen(),
    }
}

/// Creates the initial state and phaselocks.
///
/// Returns the initial state and the phaselocks of unfailing nodes.
///
/// # Arguments
///
/// * `nodes_to_fail` - a set of nodes to be failed, i.e., nodes whose
/// phaselocks will never get unpaused, and a boolean indicating whether
/// to fail the first or last `num_failed_nodes` nodes.
#[instrument(skip(sks, networkings))]
#[allow(clippy::too_many_arguments)]
pub async fn init_state_and_phaselocks<I, const N: usize>(
    sks: &tc::SecretKeySet,
    num_nodes: u64,
    known_nodes: Vec<PubKey>,
    nodes_to_fail: HashSet<u64>,
    threshold: u64,
    networkings: Vec<(I::Networking, PubKey)>,
    timeout_ratio: (u64, u64),
    next_view_timeout: u64,
    state: <I as NodeImplementation<N>>::State,
) -> (
    <I as NodeImplementation<N>>::State,
    Vec<PhaseLockHandle<I, N>>,
)
where
    I: NodeImplementation<N, Election = StaticCommittee<<I as NodeImplementation<N>>::State, N>>,
    <I as NodeImplementation<N>>::Block: Default,
    <I as NodeImplementation<N>>::Storage: Default,
    <I as NodeImplementation<N>>::StatefulHandler: Default,
{
    // Create the initial phaselocks
    let config = PhaseLockConfig {
        total_nodes: num_nodes as u32,
        threshold: threshold as u32,
        max_transactions: 100,
        known_nodes: known_nodes.clone(),
        next_view_timeout,
        timeout_ratio,
        round_start_delay: 1,
        start_delay: 1,
    };
    debug!(?config);
    let genesis = <I::Block>::default();
    let mut phaselocks = Vec::new();
    for node_id in 0..num_nodes {
        let phaselock = PhaseLock::init(
            genesis.clone(),
            sks.public_keys(),
            sks.secret_key_share(node_id),
            node_id,
            config.clone(),
            state.clone(),
            networkings[node_id as usize].0.clone(),
            I::Storage::default(),
            I::StatefulHandler::default(),
            StaticCommittee::new(known_nodes.clone()),
        )
        .await
        .expect("Could not init phaselock");
        if !nodes_to_fail.contains(&node_id) {
            phaselocks.push(phaselock);
        }
        debug!("phaselock launched");
    }

    (state, phaselocks)
}
