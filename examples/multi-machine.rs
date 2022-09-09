use clap::Parser;
use hotshot::{
    demos::dentry::*,
    traits::{
        election::StaticCommittee,
        implementations::{MemoryStorage, Stateless, WNetwork},
        StateContents,
    },
    types::{ed25519::Ed25519Pub, Event, EventType, HotShotHandle, Message, SignatureKey},
    HotShot,
};
use hotshot_types::{
    traits::{signature_key::TestableSignatureKey, state::TestableState},
    ExecutionType, HotShotConfig,
};
use hotshot_utils::{
    async_std_or_tokio::{async_main, async_sleep, async_spawn},
    test_util::{setup_backtrace, setup_logging},
};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::BTreeMap, fs::File, io::Read, num::NonZeroUsize, path::Path, time::Duration,
};
use toml::Value;
use tracing::debug;

const TRANSACTION_COUNT: u64 = 10;

type Node = DEntryNode<WNetwork<Message<DEntryState, Ed25519Pub>, Ed25519Pub>>;

#[derive(Debug, Parser)]
#[clap(
    name = "Multi-machine consensus",
    about = "Simulates consensus among multiple machines"
)]
struct NodeOpt {
    /// Path to the node configuration file
    #[clap(
        long = "config",
        short = 'c',
        default_value = "../../../examples/node-config.toml"
    )]
    config: String,

    /// Id of the current node
    #[clap(long = "id", short = 'i', default_value = "0")]
    id: u64,
}

/// Gets IP address and port number of a node from node configuration file.
fn get_host(node_config: Value, node_id: u64) -> (String, u16) {
    let node = &node_config["nodes"][node_id.to_string()];
    let ip = node["ip"].as_str().expect("Missing IP info").to_owned();
    let port = node["port"].as_integer().expect("Missing port info") as u16;
    (ip, port)
}

/// Trys to get a networking implementation with the given id and port number.
///
/// Also starts the background task.
async fn get_networking<
    T: Clone + Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + 'static,
>(
    pub_key: Ed25519Pub,
    listen_addr: &str,
    port: u16,
) -> (WNetwork<T, Ed25519Pub>, Ed25519Pub) {
    debug!(?pub_key);
    let network = WNetwork::new(pub_key, listen_addr, port, None).await;
    if let Ok(n) = network {
        let (c, sync) = futures::channel::oneshot::channel();
        match n.generate_task(c) {
            Some(task) => {
                task.into_iter().for_each(|n| {
                    async_spawn(n);
                });
                sync.await.expect("sync.await failed");
            }
            None => {
                panic!("Failed to launch networking task");
            }
        }
        return (n, pub_key);
    }
    panic!("Failed to open a port");
}

/// Creates the initial state and hotshot for simulation.
// TODO: remove `SecretKeySet` from parameters and read `PubKey`s from files.
async fn init_state_and_hotshot(
    nodes: usize,
    threshold: usize,
    node_id: u64,
    networking: WNetwork<Message<DEntryState, Ed25519Pub>, Ed25519Pub>,
) -> (DEntryState, HotShotHandle<Node>) {
    // Create the initial state
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

    let block = DEntryBlock::genesis_from(balances);
    let state = DEntryState::default().append(&block).unwrap();

    // Create the initial hotshot
    let known_nodes: Vec<_> = (0..nodes as u64)
        .map(|x| {
            let priv_key = Ed25519Pub::generate_test_key(x);
            Ed25519Pub::from_private(&priv_key)
        })
        .collect();

    let config = HotShotConfig {
        execution_type: ExecutionType::Continuous,
        total_nodes: NonZeroUsize::new(nodes).unwrap(),
        threshold: NonZeroUsize::new(threshold).unwrap(),
        max_transactions: NonZeroUsize::new(100).unwrap(),
        known_nodes: known_nodes.clone(),
        next_view_timeout: 10000,
        timeout_ratio: (11, 10),
        round_start_delay: 1,
        start_delay: 1,
        propose_min_round_time: Duration::from_millis(0),
        propose_max_round_time: Duration::from_millis(1000),
        // FIXME should this be 5?
        num_bootstrap: 5,
    };
    debug!(?config);
    let priv_key = Ed25519Pub::generate_test_key(node_id);
    let pub_key = Ed25519Pub::from_private(&priv_key);
    let hotshot = HotShot::init(
        known_nodes.clone(),
        pub_key,
        priv_key,
        node_id,
        config,
        networking,
        MemoryStorage::new(block, state.clone()),
        Stateless::default(),
        StaticCommittee::new(known_nodes),
    )
    .await
    .expect("Could not init hotshot");
    debug!("hotshot launched");

    (state, hotshot)
}

#[async_main]
async fn main() {
    // Setup tracing listener
    setup_logging();
    setup_backtrace();

    // Read configuration file path and node id from options
    let config_path_str = NodeOpt::from_args().config;
    let path = Path::new(&config_path_str);
    let own_id = NodeOpt::from_args().id;
    println!("  - Spawning network for node {}", own_id);

    // Read node info from node configuration file
    let mut config_file = File::open(&path)
        .unwrap_or_else(|_| panic!("Cannot find node config file: {}", path.display()));
    let mut config_str = String::new();
    config_file
        .read_to_string(&mut config_str)
        .unwrap_or_else(|err| panic!("Error while reading node config: [{}]", err));

    let node_config: Value =
        toml::from_str(&config_str).expect("Error while reading node config file");

    let nodes = node_config["nodes"]
        .as_table()
        .expect("Missing nodes info")
        .len();
    let threshold = ((nodes * 2) / 3) + 1;

    // Get networking information
    // TODO: read `PubKey`s from files.
    let (own_network, _) = get_networking(
        Ed25519Pub::from_private(&Ed25519Pub::generate_test_key(own_id)),
        "0.0.0.0",
        get_host(node_config.clone(), own_id).1,
    )
    .await;
    #[allow(clippy::type_complexity)]
    let mut other_nodes: Vec<(u64, Ed25519Pub, String, u16)> = Vec::new();
    for id in 0..nodes as u64 {
        if id != own_id {
            let (ip, port) = get_host(node_config.clone(), id);
            let pub_key = Ed25519Pub::from_private(&Ed25519Pub::generate_test_key(id));
            other_nodes.push((id, pub_key, ip, port));
        }
    }

    // Connect the networking implementations
    for (id, key, ip, port) in other_nodes {
        let socket = format!("{}:{}", ip, port);
        while own_network.connect_to(key, &socket).await.is_err() {
            println!("  - Retrying");
            debug!("Retrying");
            async_sleep(std::time::Duration::from_millis(10_000)).await;
        }
        println!("  - Connected to node {}", id);
        debug!("Connected to node {}", id);
    }

    // Wait for the networking implementations to connect
    while own_network.connection_table_size().await < nodes - 1 {
        async_sleep(std::time::Duration::from_millis(10)).await;
    }
    println!("All nodes connected to network");
    debug!("All nodes connected to network");

    // Initialize the state and hotshot
    let (mut own_state, mut hotshot) =
        init_state_and_hotshot(nodes, threshold, own_id, own_network).await;
    hotshot.start().await;

    // Run random transactions
    println!("Running random transactions");
    debug!("Running random transactions");
    let mut round: u64 = 1;
    while round < TRANSACTION_COUNT + 1 {
        debug!(?round);
        println!("Round {}:", round);

        // Start consensus
        println!("  - Waiting for consensus to occur");
        debug!("Waiting for consensus to occur");
        let mut event: Event<DEntryState> = hotshot
            .next_event()
            .await
            .expect("HotShot unexpectedly closed");
        while !matches!(event.event, EventType::Decide { .. }) {
            if matches!(event.event, EventType::Leader { .. }) {
                let tx = own_state.create_random_transaction();
                println!("  - Proposing: {:?}", tx);
                debug!("Proposing: {:?}", tx);
                hotshot
                    .submit_transaction(tx)
                    .await
                    .expect("Failed to submit transaction");
            }
            event = hotshot
                .next_event()
                .await
                .expect("HotShot unexpectedly closed");
        }
        println!("Node {} reached decision", own_id);
        debug!(?own_id, "Decision emitted");
        if let EventType::Decide { leaf_chain, .. } = event.event {
            println!("  - Balances:");
            for (account, balance) in &leaf_chain[0].state.balances {
                println!("    - {}: {}", account, balance);
            }
            own_state = leaf_chain[0].state.clone();
        } else {
            unreachable!()
        }
        round += 1;

        let mut line = String::new();
        println!("Hit any key to start the next round...");
        std::io::stdin().read_line(&mut line).unwrap();
    }

    println!("All rounds completed");
    debug!("All rounds completed");
}
