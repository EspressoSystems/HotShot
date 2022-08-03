use clap::Parser;
use futures::future::join_all;
use hotshot_types::traits::signature_key::{
    ed25519::{Ed25519Priv, Ed25519Pub},
    SignatureKey,
};
use hotshot_utils::test_util::{setup_backtrace, setup_logging};
use rand_xoshiro::{rand_core::SeedableRng, Xoshiro256StarStar};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroUsize,
    time::{Duration, Instant},
};
use tracing::{debug, error, instrument};

use hotshot::{
    demos::dentry::*,
    traits::{
        election::StaticCommittee,
        implementations::{MemoryStorage, Stateless, WNetwork},
    },
    types::{Event, EventType, HotShotHandle, Message},
    HotShot, HotShotConfig, H_256,
};

type Node =
    DEntryNode<WNetwork<Message<DEntryBlock, Transaction, State, Ed25519Pub, H_256>, Ed25519Pub>>;

#[derive(Debug, Parser)]
#[clap(
    name = "Double Entry Simulator",
    about = "Simulates consensus among a number of nodes"
)]
struct Opt {
    /// Number of nodes to run
    #[structopt(short = 'n', default_value = "7")]
    nodes: usize,
    /// Number of transactions to simulate
    #[structopt(short = 't', default_value = "10")]
    transactions: usize,
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
    ]
}

#[async_std::main]
#[instrument]
async fn main() {
    // Setup tracing listener
    setup_logging();
    setup_backtrace();

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
    // Spawn the networking backends and connect them together
    #[allow(clippy::type_complexity)]
    let mut networkings: Vec<(
        WNetwork<Message<DEntryBlock, Transaction, State, Ed25519Pub, H_256>, Ed25519Pub>,
        u16,
        Ed25519Pub,
        u64,
    )> = Vec::new();
    for node_id in 0..nodes as u64 {
        let private_key = Ed25519Priv::generated_from_seed_indexed([0_u8; 32], node_id);
        let public_key = Ed25519Pub::from_private(&private_key);
        networkings.push(get_networking(public_key, "0.0.0.0", node_id, &mut rng).await);
    }
    // Connect the networking implementations
    for (i, (n, _, self_key, _node_id)) in networkings.iter().enumerate() {
        for (_, port, key, _other_node_id) in networkings[i..].iter() {
            if key != self_key {
                let socket = format!("localhost:{}", port);
                n.connect_to(key.clone(), &socket)
                    .await
                    .expect("Unable to connect to node");
            }
        }
    }
    // Wait for the networking implementations to connect
    for (n, _, _, _) in &networkings {
        while n.connection_table_size().await < nodes - 1 {
            async_std::task::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    // Create the hotshots
    let mut hotshots: Vec<HotShotHandle<_, H_256>> =
        join_all(networkings.into_iter().map(|(network, _, _pk, node_id)| {
            get_hotshot(nodes, threshold, node_id, network, &inital_state)
        }))
        .await;

    let prebaked_txns = prebaked_transactions();
    let prebaked_count = prebaked_txns.len() as u64;
    let mut state = None;

    let start = Instant::now();

    println!("Running through prebaked transactions");
    debug!("Running through prebaked transactions");
    for (round, tx) in prebaked_txns.into_iter().enumerate() {
        println!("Round {}:", round);
        println!("  - Proposing: {:?}", tx);
        debug!("Proposing: {:?}", tx);
        hotshots[0]
            .submit_transaction(tx)
            .await
            .expect("Failed to submit transaction");
        println!("  - Unlocking round");
        debug!("Unlocking round");
        for hotshot in &hotshots {
            hotshot.run_one_round().await;
        }
        println!("  - Waiting for consensus to occur");
        debug!("Waiting for consensus to occur");
        let mut blocks = Vec::new();
        let mut states = Vec::new();
        for (node_id, hotshot) in hotshots.iter_mut().enumerate() {
            debug!(?node_id, "Waiting on node to emit decision");
            let mut event: Event<DEntryBlock, State, H_256> = hotshot
                .next_event()
                .await
                .expect("HotShot unexpectedly closed");
            while !matches!(event.event, EventType::Decide { .. }) {
                if matches!(event.event, EventType::ViewTimeout { .. }) {
                    error!(?event, "Round timed out!");
                    panic!("Round failed");
                }
                debug! {?node_id, ?event};
                event = hotshot
                    .next_event()
                    .await
                    .expect("HotShot unexpectedly closed");
            }
            println!("    - Node {} reached decision", node_id);
            debug!(?node_id, "Decision emitted");
            if let EventType::Decide { block, state, .. } = event.event {
                blocks.push(block);
                states.push(state);
            } else {
                unreachable!()
            }
        }
        debug!("All nodes reached decision");
        assert!(states.len() == nodes);
        assert!(blocks.len() == nodes);
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
        for (account, balance) in &s_test[0].balances {
            println!("    - {}: {}", account, balance);
        }
        state = Some(s_test.clone());
    }

    println!("Running random transactions");
    debug!("Running random transactions");
    for round in prebaked_count..opt.transactions as u64 {
        debug!(?round);
        let tx = random_transaction(&state.as_ref().unwrap()[0], &mut rng);
        println!("Round {}:", round);
        println!("  - Proposing: {:?}", tx);
        debug!("Proposing: {:?}", tx);
        hotshots[0]
            .submit_transaction(tx)
            .await
            .expect("Failed to submit transaction");
        println!("  - Unlocking round");
        debug!("Unlocking round");
        for hotshot in &hotshots {
            hotshot.run_one_round().await;
        }
        println!("  - Waiting for consensus to occur");
        debug!("Waiting for consensus to occur");
        let mut blocks = Vec::new();
        let mut states = Vec::new();
        for (node_id, hotshot) in hotshots.iter_mut().enumerate() {
            debug!(?node_id, "Waiting on node to emit decision");
            let mut event: Event<DEntryBlock, State, H_256> = hotshot
                .next_event()
                .await
                .expect("HotShot unexpectedly closed");
            while !matches!(event.event, EventType::Decide { .. }) {
                if matches!(event.event, EventType::ViewTimeout { .. }) {
                    error!(?event, "Round timed out!");
                    panic!("Round failed");
                }
                debug! {?node_id, ?event};
                event = hotshot
                    .next_event()
                    .await
                    .expect("HotShot unexpectedly closed");
            }
            println!("    - Node {} reached decision", node_id);
            debug!(?node_id, "Decision emitted");
            if let EventType::Decide { block, state, .. } = event.event {
                blocks.push(block);
                states.push(state);
            } else {
                unreachable!()
            }
        }
        debug!("All nodes reached decision");
        assert!(states.len() == nodes);
        assert!(blocks.len() == nodes);
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
        for (account, balance) in &s_test[0].balances {
            println!("    - {}: {}", account, balance);
        }
        state = Some(s_test.clone());
    }

    let end = Instant::now();
    println!();
    let duration = end.duration_since(start);
    let time = duration.as_secs_f64();
    let per_round = time / (opt.transactions as f64);
    println!(
        "Completed {} rounds of consensus in {:.3} seconds ({:.3} seconds / round)",
        opt.transactions, time, per_round
    );
}

/// Provides the initial state for the simulation
fn inital_state() -> State {
    let balances: BTreeMap<Account, Balance> = vec![
        ("Joe", 1_000_000),
        ("Nathan M", 500_000_000),
        ("John", 400_000_000),
        ("Nathan Y", 600_000_000),
        ("Ian", 300_000_000),
    ]
    .into_iter()
    .map(|(x, y)| (x.to_string(), y))
    .collect();
    State {
        balances,
        nonces: BTreeSet::default(),
    }
}

/// Trys to get a networking implementation with the given id
///
/// also starts the background task
#[instrument(skip(rng))]
async fn get_networking<
    T: Clone + Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + 'static,
    R: hotshot::rand::Rng,
>(
    pub_key: Ed25519Pub,
    listen_addr: &str,
    node_id: u64,
    rng: &mut R,
) -> (WNetwork<T, Ed25519Pub>, u16, Ed25519Pub, u64) {
    debug!(?pub_key);
    for attempt in 0..50 {
        let port: u16 = rng.gen_range(10_000..50_000);
        debug!(
            ?attempt,
            ?port,
            "Attempting to bind network listener to port"
        );
        let x = WNetwork::new(pub_key.clone(), listen_addr, port, None).await;
        if let Ok(x) = x {
            let (c, sync) = futures::channel::oneshot::channel();
            match x.generate_task(c) {
                Some(task) => {
                    task.into_iter().for_each(|x| {
                        async_std::task::spawn(x);
                    });
                    sync.await.expect("sync.await failed");
                }
                None => {
                    panic!("Failed to launch networking task");
                }
            }
            return (x, port, pub_key, node_id);
        }
    }
    panic!("Failed to open a port");
}

/// Creates a hotshot
#[instrument(skip(networking, state))]
async fn get_hotshot(
    nodes: usize,
    threshold: usize,
    node_id: u64,
    networking: WNetwork<Message<DEntryBlock, Transaction, State, Ed25519Pub, H_256>, Ed25519Pub>,
    state: &State,
) -> HotShotHandle<Node, H_256> {
    let known_nodes: Vec<_> = (0..nodes)
        .map(|x| {
            Ed25519Pub::from_private(&Ed25519Priv::generated_from_seed_indexed(
                [0_u8; 32], x as u64,
            ))
        })
        .collect();
    let config = HotShotConfig {
        total_nodes: NonZeroUsize::new(nodes).unwrap(),
        threshold: NonZeroUsize::new(threshold).unwrap(),
        max_transactions: NonZeroUsize::new(100).unwrap(),
        known_nodes: known_nodes.clone(),
        next_view_timeout: 100000,
        timeout_ratio: (11, 10),
        round_start_delay: 1,
        start_delay: 1,
        propose_min_round_time: Duration::from_millis(0),
        propose_max_round_time: Duration::from_millis(1000),
        num_bootstrap: 5,
    };
    debug!(?config);
    let private_key = Ed25519Priv::generated_from_seed_indexed([0_u8; 32], node_id);
    let public_key = Ed25519Pub::from_private(&private_key);
    let genesis = DEntryBlock::default();
    let h = HotShot::init(
        genesis,
        known_nodes.clone(),
        public_key,
        private_key,
        node_id,
        config,
        state.clone(),
        networking,
        MemoryStorage::default(),
        Stateless::default(),
        StaticCommittee::new(known_nodes),
    )
    .await
    .expect("Could not init hotshot");
    debug!("hotshot launched");
    h
}
