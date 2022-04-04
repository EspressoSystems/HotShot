#![allow(dead_code)]

use phaselock::{
    demos::dentry::{Account, Addition, Balance, State, Subtraction, Transaction},
    tc,
    traits::{
        implementations::{DummyReliability, MasterMap, MemoryNetwork},
        NodeImplementation,
    },
    types::PhaseLockHandle,
    PhaseLock, PhaseLockConfig, PubKey,
};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    env::{var, VarError},
    sync::{Arc, Once},
};
use tracing::{debug, instrument};
use tracing_error::ErrorLayer;
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    prelude::*,
    EnvFilter, Registry,
};

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
        let network = MemoryNetwork::new(
            pub_key.clone(),
            master.clone(),
            Option::<DummyReliability>::None,
        );
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
    I: NodeImplementation<N>,
    <I as NodeImplementation<N>>::Block: Default,
    <I as NodeImplementation<N>>::Storage: Default,
    <I as NodeImplementation<N>>::StatefulHandler: Default,
{
    // Create the initial phaselocks
    let config = PhaseLockConfig {
        total_nodes: num_nodes as u32,
        threshold: threshold as u32,
        max_transactions: 100,
        known_nodes,
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
