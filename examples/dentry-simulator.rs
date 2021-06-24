use futures::future::join_all;
use rand_xoshiro::{rand_core::SeedableRng, Xoshiro256StarStar};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::BTreeMap,
    env::{var, VarError},
};
use structopt::StructOpt;
use tracing::{debug, instrument};
use tracing_error::ErrorLayer;
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    prelude::*,
    EnvFilter, Registry,
};

use hotstuff::{
    demos::dentry::block::*,
    event::{Event, EventType},
    handle::HotStuffHandle,
    message::Message,
    networking::w_network::WNetwork,
    tc, HotStuff, HotStuffConfig, PubKey, H_256,
};

#[derive(Debug, StructOpt)]
#[structopt(
    name = "Double Entry Simulator",
    about = "Simulates consensus among a number of nodes"
)]
struct Opt {
    /// Number of nodes to run
    #[structopt(short = "n", default_value = "7")]
    nodes: u64,
    /// Number of transactions to simulate
    #[structopt(short = "t", default_value = "10")]
    transactions: u64,
}
/// Prebaked list of transactions
fn prebaked_transactions() -> Vec<Transaction> {
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
        },
    ]
}

#[async_std::main]
#[instrument]
async fn main() {
    // Setup tracing listener
    setup_tracing();
    // Get options
    let opt = Opt::from_args();
    debug!(?opt);
    // Setup the inital state
    let inital_state = inital_state();
    debug!(?inital_state);
    // Calculate our threshold
    let nodes = opt.nodes;
    let threshold = ((nodes * 2) / 3) + 1;
    debug!(?nodes, ?threshold);
    // Generate our private key set
    // Generated using xoshiro for reproduceability
    let mut rng = Xoshiro256StarStar::seed_from_u64(0);
    let sks = tc::SecretKeySet::random(threshold as usize - 1, &mut rng);
    // Spawn the networking backends and connect them together
    let mut networkings: Vec<(
        WNetwork<Message<DEntryBlock, Transaction, H_256>>,
        u16,
        PubKey,
    )> = Vec::new();
    for node_id in 0..nodes {
        networkings.push(get_networking(&sks, node_id, &mut rng).await);
    }
    // Connect the networking implementations
    for (i, (n, _, self_key)) in networkings.iter().enumerate() {
        for (_, port, key) in networkings[i..].iter() {
            if key != self_key {
                let socket = format!("localhost:{}", port);
                n.connect_to(key.clone(), &socket)
                    .await
                    .expect("Unable to connect to node");
            }
        }
    }
    // Wait for the networking implementations to connect
    for (n, _, _) in &networkings {
        while (n.connection_table_size().await as u64) < nodes - 1 {
            async_std::task::sleep(std::time::Duration::from_millis(10)).await;
        }
        while (n.nodes_table_size().await as u64) < nodes - 1 {
            async_std::task::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    // Create the hotstuffs
    let mut hotstuffs: Vec<HotStuffHandle<_, H_256>> =
        join_all(networkings.into_iter().map(|(network, _, pk)| {
            let node_id = pk.nonce;
            get_hotstuff(&sks, nodes, threshold, node_id, network, &inital_state)
        }))
        .await;

    let prebaked_txns = prebaked_transactions();
    let prebaked_count = prebaked_txns.len() as u64;
    let mut state = None;

    println!("Running through prebaked transactions");
    debug!("Running through prebaked transactions");
    for (round, tx) in prebaked_txns.into_iter().enumerate() {
        println!("Round {}:", round);
        println!("  - Proposing: {:?}", tx);
        debug!("Proposing: {:?}", tx);
        hotstuffs[0]
            .submit_transaction(tx)
            .await
            .expect("Failed to submit transaction");
        println!("  - Unlocking round");
        debug!("Unlocking round");
        for hotstuff in &hotstuffs {
            hotstuff.run_one_round().await;
        }
        println!("  - Waiting for consensus to occur");
        debug!("Waiting for consensus to occur");
        let mut blocks = Vec::new();
        let mut states = Vec::new();
        for (node_id, hotstuff) in hotstuffs.iter_mut().enumerate() {
            debug!(?node_id, "Waiting on node to emit decision");
            let mut event: Event<DEntryBlock, State> = hotstuff
                .next_event()
                .await
                .expect("Hotstuff unexpectedly closed");
            while !matches!(event.event, EventType::Decide { .. }) {
                debug! {?node_id, ?event};
                event = hotstuff
                    .next_event()
                    .await
                    .expect("Hotstuff unexpectedly closed");
            }
            println!("    - Node {} reached decision", node_id);
            debug!(?node_id, "Decision emitted");
            if let EventType::Decide { block, state } = event.event {
                blocks.push(block);
                states.push(state);
            } else {
                unreachable!()
            }
        }
        debug!("All nodes reached decision");
        assert!(states.len() as u64 == nodes);
        assert!(blocks.len() as u64 == nodes);
        let b_test = &blocks[0];
        for b in &blocks[1..] {
            assert!(b == b_test);
        }
        let s_test = &states[0];
        for s in &states[1..] {
            assert!(s == s_test);
        }
        println!("  - All states match");
        println!("  - Balances:");
        for (account, balance) in &s_test.balances {
            println!("    - {}: {}", account, balance);
        }
        state = Some(s_test.clone());
    }

    println!("Running random transactions");
    debug!("Running random transactions");
    for round in prebaked_count..opt.transactions {
        debug!(?round);
        let tx = random_transaction(state.as_ref().unwrap(), &mut rng);
        println!("Round {}:", round);
        println!("  - Proposing: {:?}", tx);
        debug!("Proposing: {:?}", tx);
        hotstuffs[0]
            .submit_transaction(tx)
            .await
            .expect("Failed to submit transaction");
        println!("  - Unlocking round");
        debug!("Unlocking round");
        for hotstuff in &hotstuffs {
            hotstuff.run_one_round().await;
        }
        println!("  - Waiting for consensus to occur");
        debug!("Waiting for consensus to occur");
        let mut blocks = Vec::new();
        let mut states = Vec::new();
        for (node_id, hotstuff) in hotstuffs.iter_mut().enumerate() {
            debug!(?node_id, "Waiting on node to emit decision");
            let mut event: Event<DEntryBlock, State> = hotstuff
                .next_event()
                .await
                .expect("Hotstuff unexpectedly closed");
            while !matches!(event.event, EventType::Decide { .. }) {
                debug! {?node_id, ?event};
                event = hotstuff
                    .next_event()
                    .await
                    .expect("Hotstuff unexpectedly closed");
            }
            println!("    - Node {} reached decision", node_id);
            debug!(?node_id, "Decision emitted");
            if let EventType::Decide { block, state } = event.event {
                blocks.push(block);
                states.push(state);
            } else {
                unreachable!()
            }
        }
        debug!("All nodes reached decision");
        assert!(states.len() as u64 == nodes);
        assert!(blocks.len() as u64 == nodes);
        let b_test = &blocks[0];
        for b in &blocks[1..] {
            assert!(b == b_test);
        }
        let s_test = &states[0];
        for s in &states[1..] {
            assert!(s == s_test);
        }
        println!("  - All states match");
        println!("  - Balances:");
        for (account, balance) in &s_test.balances {
            println!("    - {}: {}", account, balance);
        }
        state = Some(s_test.clone());
    }
}

/// Configures and installs the tracing listener
fn setup_tracing() {
    let internal_event_filter =
                match var("RUST_LOG_SPAN_EVENTS") {
                    Ok(value) => {
                        value
                            .to_ascii_lowercase()
                            .split(",")
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
    let fmt_layer = fmt::Layer::default()
        .with_span_events(internal_event_filter)
        .with_writer(std::io::stderr);
    Registry::default()
        .with(EnvFilter::from_default_env())
        .with(ErrorLayer::default())
        .with(fmt_layer)
        .init();
}

/// Provides the initial state for the simulation
fn inital_state() -> State {
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
    State { balances }
}

/// Trys to get a networking implementation with the given id
///
/// also starts the background task
#[instrument(skip(rng, sks))]
async fn get_networking<
    T: Clone + Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + 'static,
    R: hotstuff::rand::Rng,
>(
    sks: &tc::SecretKeySet,
    node_id: u64,
    rng: &mut R,
) -> (WNetwork<T>, u16, PubKey) {
    let pub_key = PubKey::from_secret_key_set_escape_hatch(sks, node_id);
    debug!(?pub_key);
    for attempt in 0..10 {
        let port: u16 = rng.gen_range(2000, 8000);
        debug!(
            ?attempt,
            ?port,
            "Attempting to bind network listener to port"
        );
        let x = WNetwork::new_from_strings(pub_key.clone(), vec![], port, None).await;
        if let Ok(x) = x {
            let (c, sync) = futures::channel::oneshot::channel();
            match x.generate_task(c) {
                Some(task) => {
                    async_std::task::spawn(task);
                    sync.await.expect("sync.await failed");
                }
                None => {
                    panic!("Failed to launch networking task");
                }
            }
            return (x, port, pub_key);
        }
    }
    panic!("Failed to open a port");
}

/// Creates a hotstuff
#[instrument(skip(keys, networking, state))]
async fn get_hotstuff(
    keys: &tc::SecretKeySet,
    nodes: u64,
    threshold: u64,
    node_id: u64,
    networking: WNetwork<Message<DEntryBlock, Transaction, H_256>>,
    state: &State,
) -> HotStuffHandle<DEntryBlock, H_256> {
    let known_nodes: Vec<_> = (0..nodes)
        .map(|x| PubKey::from_secret_key_set_escape_hatch(keys, x))
        .collect();
    let config = HotStuffConfig {
        total_nodes: nodes as u32,
        thershold: threshold as u32,
        max_transactions: 100,
        known_nodes,
        next_view_timeout: 500,
        timeout_ratio: (11, 10),
    };
    debug!(?config);
    let genesis = DEntryBlock::default();
    let (_, h) = HotStuff::init(genesis, keys, node_id, config, state.clone(), networking).await;
    debug!("hotstuff launched");
    h
}

/// Provides a random valid transaction from the current state
fn random_transaction<R: rand::Rng>(state: &State, mut rng: &mut R) -> Transaction {
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
    }
}
