use futures::future::join_all;
use phaselock_utils::test_util::{setup_backtrace, setup_logging};
use rand_xoshiro::{rand_core::SeedableRng, Xoshiro256StarStar};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroUsize,
    time::{Duration, Instant},
};
use structopt::StructOpt;
use tracing::{debug, error, instrument};

use phaselock::{
    demos::dentry::*,
    tc,
    traits::{
        election::StaticCommittee,
        implementations::{MemoryStorage, Stateless, WNetwork},
    },
    types::{Event, EventType, Message, PhaseLockHandle},
    PhaseLock, PhaseLockConfig, PubKey, H_256,
};

type Node = DEntryNode<WNetwork<Message<DEntryBlock, Transaction, State, H_256>>>;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "Double Entry Simulator",
    about = "Simulates consensus among a number of nodes"
)]
struct Opt {
    /// Number of nodes to run
    #[structopt(short = "n", default_value = "7")]
    nodes: usize,
    /// Number of transactions to simulate
    #[structopt(short = "t", default_value = "10")]
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
    let sks = tc::SecretKeySet::random(threshold as usize - 1, &mut rng);
    // Spawn the networking backends and connect them together
    #[allow(clippy::type_complexity)]
    let mut networkings: Vec<(
        WNetwork<Message<DEntryBlock, Transaction, State, H_256>>,
        u16,
        PubKey,
    )> = Vec::new();
    for node_id in 0..nodes as u64 {
        networkings.push(get_networking(&sks, "0.0.0.0", node_id, &mut rng).await);
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
        while n.connection_table_size().await < nodes - 1 {
            async_std::task::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    // Create the phaselocks
    let mut phaselocks: Vec<PhaseLockHandle<_, H_256>> =
        join_all(networkings.into_iter().map(|(network, _, pk)| {
            let node_id = pk.nonce;
            get_phaselock(&sks, nodes, threshold, node_id, network, &inital_state)
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
        phaselocks[0]
            .submit_transaction(tx)
            .await
            .expect("Failed to submit transaction");
        println!("  - Unlocking round");
        debug!("Unlocking round");
        for phaselock in &phaselocks {
            phaselock.run_one_round().await;
        }
        println!("  - Waiting for consensus to occur");
        debug!("Waiting for consensus to occur");
        let mut blocks = Vec::new();
        let mut states = Vec::new();
        for (node_id, phaselock) in phaselocks.iter_mut().enumerate() {
            debug!(?node_id, "Waiting on node to emit decision");
            let mut event: Event<DEntryBlock, State> = phaselock
                .next_event()
                .await
                .expect("PhaseLock unexpectedly closed");
            while !matches!(event.event, EventType::Decide { .. }) {
                if matches!(event.event, EventType::ViewTimeout { .. }) {
                    error!(?event, "Round timed out!");
                    panic!("Round failed");
                }
                debug! {?node_id, ?event};
                event = phaselock
                    .next_event()
                    .await
                    .expect("PhaseLock unexpectedly closed");
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
        phaselocks[0]
            .submit_transaction(tx)
            .await
            .expect("Failed to submit transaction");
        println!("  - Unlocking round");
        debug!("Unlocking round");
        for phaselock in &phaselocks {
            phaselock.run_one_round().await;
        }
        println!("  - Waiting for consensus to occur");
        debug!("Waiting for consensus to occur");
        let mut blocks = Vec::new();
        let mut states = Vec::new();
        for (node_id, phaselock) in phaselocks.iter_mut().enumerate() {
            debug!(?node_id, "Waiting on node to emit decision");
            let mut event: Event<DEntryBlock, State> = phaselock
                .next_event()
                .await
                .expect("PhaseLock unexpectedly closed");
            while !matches!(event.event, EventType::Decide { .. }) {
                if matches!(event.event, EventType::ViewTimeout { .. }) {
                    error!(?event, "Round timed out!");
                    panic!("Round failed");
                }
                debug! {?node_id, ?event};
                event = phaselock
                    .next_event()
                    .await
                    .expect("PhaseLock unexpectedly closed");
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

/// Trys to get a networking implementation with the given id
///
/// also starts the background task
#[instrument(skip(rng, sks))]
async fn get_networking<
    T: Clone + Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + 'static,
    R: phaselock::rand::Rng,
>(
    sks: &tc::SecretKeySet,
    listen_addr: &str,
    node_id: u64,
    rng: &mut R,
) -> (WNetwork<T>, u16, PubKey) {
    let pub_key = PubKey::from_secret_key_set_escape_hatch(sks, node_id);
    debug!(?pub_key);
    for attempt in 0..50 {
        let port: u16 = rng.gen_range(10_000, 50_000);
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
            return (x, port, pub_key);
        }
    }
    panic!("Failed to open a port");
}

/// Creates a phaselock
#[instrument(skip(keys, networking, state))]
async fn get_phaselock(
    keys: &tc::SecretKeySet,
    nodes: usize,
    threshold: usize,
    node_id: u64,
    networking: WNetwork<Message<DEntryBlock, Transaction, State, H_256>>,
    state: &State,
) -> PhaseLockHandle<Node, H_256> {
    let known_nodes: Vec<_> = (0..nodes)
        .map(|x| PubKey::from_secret_key_set_escape_hatch(keys, x.try_into().unwrap()))
        .collect();
    let config = PhaseLockConfig {
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
    };
    debug!(?config);
    let genesis = DEntryBlock::default();
    let h = PhaseLock::init(
        genesis,
        keys.public_keys(),
        keys.secret_key_share(node_id),
        node_id,
        config,
        state.clone(),
        networking,
        MemoryStorage::default(),
        Stateless::default(),
        StaticCommittee::new(known_nodes),
    )
    .await
    .expect("Could not init phaselock");
    debug!("phaselock launched");
    h
}
