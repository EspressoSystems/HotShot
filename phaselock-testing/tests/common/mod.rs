#![allow(dead_code)]

use either::Either;
use phaselock::{
    demos::dentry::{Account, Addition, Balance, DEntryBlock, State, Subtraction, Transaction},
    tc,
    traits::{
        election::StaticCommittee,
        implementations::{DummyReliability, MasterMap, MemoryNetwork},
        NodeImplementation,
    },
    types::PhaseLockHandle,
    PhaseLock, PhaseLockConfig, PubKey,
};
use phaselock_testing::{ConsensusTestError, TestLauncher, TransactionSnafu};
use rand::{
    distributions::{Bernoulli, Uniform},
    prelude::Distribution,
};
use serde::{de::DeserializeOwned, Serialize};
use snafu::ResultExt;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    env::{var, VarError},
    sync::{Arc, Once},
    time::Duration,
};
use tracing::{debug, error, instrument};
use tracing_error::ErrorLayer;
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    prelude::*,
    EnvFilter, Registry,
};
use Either::{Left, Right};

/// A synchronous network. Packets may be delayed, but are guaranteed
/// to arrive within `timeout` ns
#[derive(Clone, Copy, Debug)]
pub struct SynchronousNetwork {
    /// max delay of packet before arrival
    timeout_ms: u64,
    /// lowest value in milliseconds that a packet may be delayed
    delay_low_ms: u64,
}

impl NetworkReliability for SynchronousNetwork {
    /// never drop a packet
    fn sample_keep(&self) -> bool {
        true
    }
    fn sample_delay(&self) -> Duration {
        Duration::from_millis(
            Uniform::new_inclusive(self.delay_low_ms, self.timeout_ms)
                .sample(&mut rand::thread_rng()),
        )
    }
}

/// An asynchronous network. Packets may be dropped entirely
/// or delayed for arbitrarily long periods
/// probability that packet is kept = `keep_numerator` / `keep_denominator`
/// packet delay is obtained by sampling from a uniform distribution
/// between `delay_low_ms` and `delay_high_ms`, inclusive
#[derive(Debug, Clone, Copy)]
pub struct AsynchronousNetwork {
    /// numerator for probability of keeping packets
    keep_numerator: u32,
    /// denominator for probability of keeping packets
    keep_denominator: u32,
    /// lowest value in milliseconds that a packet may be delayed
    delay_low_ms: u64,
    /// highest value in milliseconds that a packet may be delayed
    delay_high_ms: u64,
}

impl NetworkReliability for AsynchronousNetwork {
    fn sample_keep(&self) -> bool {
        Bernoulli::from_ratio(self.keep_numerator, self.keep_denominator)
            .unwrap()
            .sample(&mut rand::thread_rng())
    }
    fn sample_delay(&self) -> Duration {
        Duration::from_millis(
            Uniform::new_inclusive(self.delay_low_ms, self.delay_high_ms)
                .sample(&mut rand::thread_rng()),
        )
    }
}

/// An partially synchronous network. Behaves asynchronously
/// until some arbitrary time bound, GST,
/// then synchronously after GST
#[derive(Debug, Clone, Copy)]
pub struct PartiallySynchronousNetwork {
    /// asynchronous portion of network
    asynchronous: AsynchronousNetwork,
    /// synchronous portion of network
    synchronous: SynchronousNetwork,
    /// time when GST occurs
    gst: std::time::Duration,
    /// when the network was started
    start: std::time::Instant,
}

impl NetworkReliability for PartiallySynchronousNetwork {
    /// never drop a packet
    fn sample_keep(&self) -> bool {
        true
    }
    fn sample_delay(&self) -> Duration {
        // act asyncronous before gst
        if self.start.elapsed() < self.gst {
            if self.asynchronous.sample_keep() {
                self.asynchronous.sample_delay()
            } else {
                // assume packet was "dropped" and will arrive after gst
                self.synchronous.sample_delay() + self.gst
            }
        } else {
            // act syncronous after gst
            self.synchronous.sample_delay()
        }
    }
}

#[allow(clippy::derivable_impls)]
impl Default for SynchronousNetwork {
    // disable all chance of failure
    fn default() -> Self {
        SynchronousNetwork {
            delay_low_ms: 0,
            timeout_ms: 0,
        }
    }
}

impl Default for AsynchronousNetwork {
    // disable all chance of failure
    fn default() -> Self {
        AsynchronousNetwork {
            keep_numerator: 1,
            keep_denominator: 1,
            delay_low_ms: 0,
            delay_high_ms: 0,
        }
    }
}

impl Default for PartiallySynchronousNetwork {
    fn default() -> Self {
        PartiallySynchronousNetwork {
            synchronous: SynchronousNetwork::default(),
            asynchronous: AsynchronousNetwork::default(),
            gst: std::time::Duration::new(0, 0),
            start: std::time::Instant::now(),
        }
    }
}

impl SynchronousNetwork {
    /// create new `SynchronousNetwork`
    pub fn new(timeout: u64, delay_low_ms: u64) -> Self {
        SynchronousNetwork {
            timeout_ms: timeout,
            delay_low_ms,
        }
    }
}

impl AsynchronousNetwork {
    /// create new `AsynchronousNetwork`
    pub fn new(
        keep_numerator: u32,
        keep_denominator: u32,
        delay_low_ms: u64,
        delay_high_ms: u64,
    ) -> Self {
        AsynchronousNetwork {
            keep_numerator,
            keep_denominator,
            delay_low_ms,
            delay_high_ms,
        }
    }
}

impl PartiallySynchronousNetwork {
    /// create new `PartiallySynchronousNetwork`
    pub fn new(
        asynchronous: AsynchronousNetwork,
        synchronous: SynchronousNetwork,
        gst: std::time::Duration,
    ) -> Self {
        PartiallySynchronousNetwork {
            asynchronous,
            synchronous,
            gst,
            start: std::time::Instant::now(),
        }
    }
}

/// FIXME transplant this
/// # Arguments
///
/// * `nodes_to_fail` - a set of nodes to be failed, i.e., nodes whose
/// phaselocks will never get unpaused, and a boolean indicating whether
/// to fail the first or last `num_failed_nodes` nodes.
pub struct TestDescription {
    pub total_nodes: usize,
    /// either a list of transactions for each round
    /// or (num_rounds, tx_per_round)
    pub txn_ids: Either<Vec<Vec<u64>>, (usize, usize)>,
    pub next_view_timeout: u64,
    pub timeout_ratio: (u64, u64),
    pub round_start_delay: u64,
    pub start_delay: u64,
    pub ids_to_shut_down: HashSet<u64>,
    pub network_reliability: Option<Arc<dyn NetworkReliability>>,
    #[allow(clippy::type_complexity)]
    pub safety_check: Arc<
        dyn Fn(
            &Self,
            Vec<Transaction>,
            HashMap<u64, (Vec<State>, Vec<DEntryBlock>)>,
        ) -> Result<(), ConsensusTestError>,
    >,
}

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
            safety_check: Arc::new(default_check),
        }
    }
}

/// total_nodes: num nodes to run with
/// txn_ids: vec of vec of transaction ids to send each round
pub async fn run_rounds(test_description: TestDescription) -> Result<(), ConsensusTestError> {
    setup_logging();
    setup_backtrace();

    // configure nodes/timing

    let launcher = TestLauncher::new(test_description.total_nodes);

    let f = |a: &mut PhaseLockConfig| {
        a.next_view_timeout = test_description.next_view_timeout;
        a.timeout_ratio = test_description.timeout_ratio;
        a.round_start_delay = test_description.round_start_delay;
        a.start_delay = test_description.start_delay;
    };

    let reliability = test_description.network_reliability.clone();
    // create runner from launcher
    let mut runner = launcher
        .modify_default_config(f)
        .with_network({
            let master_map = MasterMap::new();
            move |pubkey| MemoryNetwork::new(pubkey, master_map.clone(), reliability.clone())
        })
        .launch();
    runner.add_nodes(test_description.total_nodes).await;
    for node in runner.nodes() {
        let qc = node.storage().get_newest_qc().await.unwrap().unwrap();
        assert_eq!(qc.view_number, 0);
    }

    // map `txn_ids` to generated transaction ids
    // do this ahead of time to "fail fast"
    let ids = runner.ids();

    let round_iter: std::vec::IntoIter<Either<Vec<u64>, usize>> = match test_description.txn_ids {
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
    for shut_down_id in &test_description.ids_to_shut_down {
        let mapped_id = *ids
            .get(*shut_down_id as usize)
            .ok_or(ConsensusTestError::NoSuchNode {
                node_ids: ids.clone(),
                requested_id: *shut_down_id,
            })?;
        runner.shutdown(mapped_id).await?;
    }

    // run rounds
    for txn_senders in round_iter {
        let txns = match txn_senders {
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
        let (states, errors) = runner.run_one_round().await;
        if !errors.is_empty() {
            error!("ERRORS RUNNING CONSENSUS, {:?}", errors);
        }

        let safety_check = test_description.safety_check.clone();
        (*safety_check)(&test_description, txns, states)?;
    }

    println!("SHUTTING DOWN ALL");
    runner.shutdown_all().await;
    println!("Consensus completed\n");
    Ok(())
}

/// Provides a common starting state
pub fn get_starting_state() -> State {
    let balances: BTreeMap<Account, Balance> = vec![
        ("Joe", 1_000_000),
        ("Nathan M", 500_000),
        ("John", 400_000),
        ("Nathan Y", 600_000),
        ("Ian", 0),
    ]
    .into_iter()
    .map(|(x, y)| (x.to_string(), y))
    .collect();
    State {
        balances,
        nonces: BTreeSet::default(),
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

/// Provides a common list of transactions
pub fn get_ten_prebaked_trasnactions() -> Vec<Transaction> {
    vec![
        Transaction {
            add: Addition {
                account: "Ian".to_string(),
                amount: 100,
            },
            sub: Subtraction {
                account: "Joe".to_string(),
                amount: 100,
            },
            nonce: 0,
        },
        Transaction {
            add: Addition {
                account: "John".to_string(),
                amount: 25,
            },
            sub: Subtraction {
                account: "Joe".to_string(),
                amount: 25,
            },
            nonce: 1,
        },
        Transaction {
            add: Addition {
                account: "Nathan Y".to_string(),
                amount: 534044,
            },
            sub: Subtraction {
                account: "Nathan Y".to_string(),
                amount: 534044,
            },
            nonce: 2,
        },
        Transaction {
            add: Addition {
                account: "Nathan Y".to_string(),
                amount: 957954,
            },
            sub: Subtraction {
                account: "Joe".to_string(),
                amount: 957954,
            },
            nonce: 3,
        },
        Transaction {
            add: Addition {
                account: "Nathan M".to_string(),
                amount: 40,
            },
            sub: Subtraction {
                account: "Ian".to_string(),
                amount: 40,
            },
            nonce: 4,
        },
        Transaction {
            add: Addition {
                account: "John".to_string(),
                amount: 83187,
            },
            sub: Subtraction {
                account: "John".to_string(),
                amount: 83187,
            },
            nonce: 5,
        },
        Transaction {
            add: Addition {
                account: "Joe".to_string(),
                amount: 340375,
            },
            sub: Subtraction {
                account: "Nathan M".to_string(),
                amount: 340375,
            },
            nonce: 6,
        },
        Transaction {
            add: Addition {
                account: "Ian".to_string(),
                amount: 180862,
            },
            sub: Subtraction {
                account: "John".to_string(),
                amount: 180862,
            },
            nonce: 7,
        },
        Transaction {
            add: Addition {
                account: "Nathan Y".to_string(),
                amount: 373073,
            },
            sub: Subtraction {
                account: "Joe".to_string(),
                amount: 373073,
            },
            nonce: 8,
        },
        Transaction {
            add: Addition {
                account: "Nathan Y".to_string(),
                amount: 71525,
            },
            sub: Subtraction {
                account: "Nathan M".to_string(),
                amount: 71525,
            },
            nonce: 9,
        },
    ]
}

static INIT: Once = Once::new();
static INIT_2: Once = Once::new();

pub fn setup_backtrace() {
    INIT_2.call_once(|| {
        color_eyre::install().unwrap();
    });
}

pub fn setup_logging() {
    INIT.call_once(|| {
            let internal_event_filter =
                match var("RUST_LOG_SPAN_EVENTS") {
                    Ok(value) => {
                        value
                            .to_ascii_lowercase()
                            .split(',')
                            .map(|filter| match filter.trim() {
                                "new" => FmtSpan::NEW,
                                "enter" => FmtSpan::ENTER,
                                "exit" => FmtSpan::EXIT,
                                "close" => FmtSpan::CLOSE,
                                "active" => FmtSpan::ACTIVE,
                                "full" => FmtSpan::FULL,
                                _ => panic!("test-env-log: RUST_LOG_SPAN_EVENTS must contain filters separated by `,`.\n\t\
                                             For example: `active` or `new,close`\n\t\
                                             Supported filters: new, enter, exit, close, active, full\n\t\
                                             Got: {}", value),
                            })
                            .fold(FmtSpan::NONE, |acc, filter| filter | acc)
                    },
                    Err(VarError::NotUnicode(_)) =>
                        panic!("test-env-log: RUST_LOG_SPAN_EVENTS must contain a valid UTF-8 string"),
                    Err(VarError::NotPresent) => FmtSpan::NONE,
                };
            let fmt_env = var("RUST_LOG_FORMAT").map(|x| x.to_lowercase());
            match fmt_env.as_deref().map(|x| x.trim()) {
                Ok("full") => {
                    let fmt_layer = fmt::Layer::default()
                        .with_span_events(internal_event_filter)
                        .with_ansi(true)
                        .with_test_writer();
                    Registry::default()
                        .with(EnvFilter::from_default_env())
                        .with(ErrorLayer::default())
                        .with(fmt_layer)
                        .init();
                },
                Ok("json") => {
                    let fmt_layer = fmt::Layer::default()
                        .with_span_events(internal_event_filter)
                        .json()
                        .with_test_writer();
                    Registry::default()
                        .with(EnvFilter::from_default_env())
                        .with(ErrorLayer::default())
                        .with(fmt_layer)
                        .init();
                },
                Ok("compact") => {
                    let fmt_layer = fmt::Layer::default()
                        .with_span_events(internal_event_filter)
                        .with_ansi(true)
                        .compact()
                        .with_test_writer();
                    Registry::default()
                        .with(EnvFilter::from_default_env())
                        .with(ErrorLayer::default())
                        .with(fmt_layer)
                        .init();
                },
                _ => {
                    let fmt_layer = fmt::Layer::default()
                        .with_span_events(internal_event_filter)
                        .with_ansi(true)
                        .pretty()
                        .with_test_writer();
                    Registry::default()
                        .with(EnvFilter::from_default_env())
                        .with(ErrorLayer::default())
                        .with(fmt_layer)
                        .init();
                },
            };
        });
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
