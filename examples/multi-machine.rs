use phaselock::{
    demos::dentry::*,
    event::{Event, EventType},
    handle::PhaseLockHandle,
    message::Message,
    networking::w_network::WNetwork,
    tc, PhaseLock, PhaseLockConfig, PubKey, H_256,
};
use rand_xoshiro::{rand_core::SeedableRng, Xoshiro256StarStar};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use structopt::StructOpt;
use toml::Value;
use tracing::{debug, error};

#[derive(Debug, StructOpt)]
#[structopt(
    name = "Multi-machine concensus",
    about = "Simulates consensus among multiple machines"
)]
struct NodeOpt {
    /// Path to the node configuration file
    // TODO: don't not use the hard-coded path
    #[structopt(
        long = "config",
        short = "c",
        default_value = "../../../examples/node-config.toml"
    )]
    config: String,

    /// Id of the current node
    #[structopt(long = "id", short = "i", default_value = "1")]
    id: u64,
}

/// Gets IP address and port number of a node from node configuration file.
fn get_host(node_config: Value, node_id: u64) -> (String, u16) {
    let node = &node_config["nodes"][node_id.to_string()];
    let ip = match node["ip"].as_str() {
        Some(ip) => ip.to_owned(),
        None => {
            panic!("Missing IP info")
        }
    };
    let port = match node["port"].as_integer() {
        Some(port) => port as u16,
        None => {
            panic!("Missing port info")
        }
    };
    (ip, port)
}

/// Trys to get a networking implementation with the given id and port number.
///
/// Also starts the background task.
// #[instrument(skip(sks))]
async fn get_networking<
    T: Clone + Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + 'static,
>(
    sks: &tc::SecretKeySet,
    node_id: u64,
    port: u16,
) -> (WNetwork<T>, PubKey) {
    let pub_key = PubKey::from_secret_key_set_escape_hatch(sks, node_id);
    debug!(?pub_key);
    let network = WNetwork::new(pub_key.clone(), port, None).await;
    if let Ok(n) = network {
        let (c, sync) = futures::channel::oneshot::channel();
        match n.generate_task(c) {
            Some(task) => {
                task.into_iter().for_each(|n| {
                    async_std::task::spawn(n);
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

/// Creates a phaselock with initial state for simulation.
async fn init_phaselock(
    keys: &tc::SecretKeySet,
    nodes: u64,
    threshold: u64,
    node_id: u64,
    networking: WNetwork<Message<DEntryBlock, Transaction, H_256>>,
) -> PhaseLockHandle<DEntryBlock, H_256> {
    // Create initial state
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
    let init_state = State {
        balances,
        nonces: BTreeSet::default(),
    };

    // Create the phaselock
    let known_nodes: Vec<_> = (0..nodes)
        .map(|x| PubKey::from_secret_key_set_escape_hatch(keys, x))
        .collect();

    let config = PhaseLockConfig {
        total_nodes: nodes as u32,
        threshold: threshold as u32,
        max_transactions: 100,
        known_nodes,
        next_view_timeout: 10000,
        timeout_ratio: (11, 10),
    };
    debug!(?config);
    let genesis = DEntryBlock::default();
    let (_, h) = PhaseLock::init(
        genesis,
        keys,
        node_id,
        config,
        init_state.clone(),
        networking,
    )
    .await;
    debug!("phaselock launched");
    h
}

#[async_std::main]
async fn main() {
    // Read configuration file path and node id from options
    let config_path_str = NodeOpt::from_args().config;
    let path = Path::new(&config_path_str);
    let own_id = NodeOpt::from_args().id;
    println!("  - Connecting node {}", own_id);

    // Read node info from node configuration file
    let mut config_file = match File::open(&path) {
        Ok(file) => file,
        Err(_) => {
            panic!("Cannot find node config file: {}", path.display());
        }
    };
    let mut config_str = String::new();
    config_file
        .read_to_string(&mut config_str)
        .unwrap_or_else(|err| panic!("Error while reading node config: [{}]", err));

    let node_config: Value = match toml::from_str(&config_str) {
        Ok(info) => info,
        Err(_) => {
            panic!("Error while reading node config file")
        }
    };

    // Get secret key set
    let seed: u64 = match node_config["seed"].as_integer() {
        Some(seed) => seed as u64,
        None => {
            panic!("Missing seed value")
        }
    };
    let nodes = match node_config["nodes"].as_table() {
        Some(nodes) => nodes.len() as u64,
        None => {
            panic!("Missing nodes info")
        }
    };
    let threshold = ((nodes * 2) / 3) + 1;
    let mut rng = Xoshiro256StarStar::seed_from_u64(seed);
    let sks = tc::SecretKeySet::random(threshold as usize - 1, &mut rng);

    // Spawn the networking backends of all other nodes
    #[allow(clippy::type_complexity)]
    let mut networkings: Vec<(
        WNetwork<Message<DEntryBlock, Transaction, H_256>>,
        PubKey,
        String,
        u16,
    )> = Vec::new();
    for id in 1..(nodes + 1) {
        let (ip, port) = get_host(node_config.clone(), id);
        let (network, pub_key) = get_networking(&sks, id, port).await;
        networkings.push((network.clone(), pub_key, ip, port));
    }

    // Connect the networking implementations
    let (own_network, _, _, _) = networkings[(own_id - 1) as usize].clone();
    for (_, key, ip, port) in networkings.iter() {
        let socket = format!("{}:{}", ip, port);
        own_network
            .connect_to(key.clone(), &socket)
            .await
            .expect("Unable to connect to node");
    }

    // Wait for the networking implementations to connect
    while (own_network.connection_table_size().await as u64) < nodes - 1 {
        async_std::task::sleep(std::time::Duration::from_millis(10)).await;
    }
    println!("Networks connected");

    // Create the phaselock
    let mut phaselock = init_phaselock(&sks, nodes, threshold, own_id, own_network).await;

    // Submit a transaction
    let tx = Transaction {
        add: Addition {
            account: "Ian".to_string(),
            amount: 100,
        },
        sub: Subtraction {
            account: "Joe".to_string(),
            amount: 100,
        },
        nonce: 0,
    };
    // TODO: txn leader may not be this node
    phaselock
        .submit_transaction(tx)
        .await
        .expect("Failed to submit transaction");

    println!("  - Unlocking round");
    debug!("Unlocking round");
    phaselock.run_one_round().await;

    // Start consensus
    println!("  - Waiting for consensus to occur");
    debug!("Waiting for consensus to occur");
    let mut event: Event<DEntryBlock, State> = phaselock
        .next_event()
        .await
        .expect("PhaseLock unexpectedly closed");
    // TODO: fix the failure here
    while !matches!(event.event, EventType::Decide { .. }) {
        if matches!(event.event, EventType::ViewTimeout { .. }) {
            error!(?event, "Round timed out!");
            panic!("Round failed");
        }
        event = phaselock
            .next_event()
            .await
            .expect("PhaseLock unexpectedly closed");
    }
    println!("Node {} reached decision", own_id);
    debug!(?own_id, "Decision emitted");
    if let EventType::Decide { block: _, state } = event.event {
        println!("  - Balances:");
        for (account, balance) in &state.balances {
            println!("    - {}: {}", account, balance);
        }
    } else {
        unreachable!()
    }
}
