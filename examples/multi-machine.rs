use phaselock::{
    demos::dentry::*,
    event::{Event, EventType},
    handle::PhaseLockHandle,
    message::Message,
    networking::w_network::WNetwork,
    tc,
    traits::{stateful_handler::Stateless, storage::memory_storage::MemoryStorage},
    PhaseLock, PhaseLockConfig, PubKey, H_256,
};
use rand_xoshiro::{rand_core::SeedableRng, Xoshiro256StarStar};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use structopt::StructOpt;
use toml::Value;
use tracing::debug;

mod common;

const TRANSACTION_COUNT: u64 = 10;

type Node = DEntryNode<WNetwork<Message<DEntryBlock, Transaction, State, H_256>>>;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "Multi-machine concensus",
    about = "Simulates consensus among multiple machines"
)]
struct NodeOpt {
    /// Path to the node configuration file
    #[structopt(
        long = "config",
        short = "c",
        default_value = "../../../examples/node-config.toml"
    )]
    config: String,

    /// Id of the current node
    #[structopt(long = "id", short = "i", default_value = "0")]
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
    pub_key: PubKey,
    port: u16,
) -> (WNetwork<T>, PubKey) {
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

/// Creates the initial state and phaselock for simulation.
// TODO: remove `SecretKeySet` from parameters and read `PubKey`s from files.
async fn init_state_and_phaselock(
    keys: &tc::SecretKeySet,
    public_keys: tc::PublicKeySet,
    secret_key_share: tc::SecretKeyShare,
    nodes: u64,
    threshold: u64,
    node_id: u64,
    networking: WNetwork<Message<DEntryBlock, Transaction, State, H_256>>,
) -> (State, PhaseLockHandle<Node, H_256>) {
    // Create the initial state
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
    let state = State {
        balances,
        nonces: BTreeSet::default(),
    };

    // Create the initial phaselock
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
        round_start_delay: 1,
        start_delay: 1,
    };
    debug!(?config);
    let genesis = DEntryBlock::default();
    let (_, phaselock) = PhaseLock::init(
        genesis,
        public_keys,
        secret_key_share,
        node_id,
        config,
        state.clone(),
        networking,
        MemoryStorage::default(),
        Stateless::default(),
    )
    .await;
    debug!("phaselock launched");

    (state, phaselock)
}

/// Provides a random valid transaction from the current state.
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
        nonce: rng.gen(),
    }
}

#[async_std::main]
async fn main() {
    // Setup tracing listener
    common::setup_tracing();

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

    // Get secret key set
    let seed: u64 = node_config["seed"]
        .as_integer()
        .expect("Missing seed value") as u64;
    let nodes = node_config["nodes"]
        .as_table()
        .expect("Missing nodes info")
        .len() as u64;
    let threshold = ((nodes * 2) / 3) + 1;
    let mut rng = Xoshiro256StarStar::seed_from_u64(seed);
    let secret_keys = tc::SecretKeySet::random(threshold as usize - 1, &mut rng);
    let public_keys = secret_keys.public_keys();
    let secret_key_share = secret_keys.secret_key_share(own_id);

    // Get networking information
    // TODO: read `PubKey`s from files.
    let (own_network, _) = get_networking(
        PubKey::from_secret_key_set_escape_hatch(&secret_keys, own_id),
        get_host(node_config.clone(), own_id).1,
    )
    .await;
    #[allow(clippy::type_complexity)]
    let mut other_nodes: Vec<(u64, PubKey, String, u16)> = Vec::new();
    for id in 0..nodes {
        if id != own_id {
            let (ip, port) = get_host(node_config.clone(), id);
            let pub_key = PubKey::from_secret_key_set_escape_hatch(&secret_keys, id);
            other_nodes.push((id, pub_key, ip, port));
        }
    }

    // Connect the networking implementations
    for (id, key, ip, port) in other_nodes {
        let socket = format!("{}:{}", ip, port);
        while own_network.connect_to(key.clone(), &socket).await.is_err() {
            println!("  - Retrying");
            debug!("Retrying");
            async_std::task::sleep(std::time::Duration::from_millis(10_000)).await;
        }
        println!("  - Connected to node {}", id);
        debug!("Connected to node {}", id);
    }

    // Wait for the networking implementations to connect
    while (own_network.connection_table_size().await as u64) < nodes - 1 {
        async_std::task::sleep(std::time::Duration::from_millis(10)).await;
    }
    println!("All nodes connected to network");
    debug!("All nodes connected to network");

    // Initialize the state and phaselock
    let (mut own_state, mut phaselock) = init_state_and_phaselock(
        &secret_keys,
        public_keys,
        secret_key_share,
        nodes,
        threshold,
        own_id,
        own_network,
    )
    .await;
    phaselock.start().await;

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
        let mut event: Event<DEntryBlock, State> = phaselock
            .next_event()
            .await
            .expect("PhaseLock unexpectedly closed");
        while !matches!(event.event, EventType::Decide { .. }) {
            if matches!(event.event, EventType::Leader { .. }) {
                let tx = random_transaction(&own_state, &mut rng);
                println!("  - Proposing: {:?}", tx);
                debug!("Proposing: {:?}", tx);
                phaselock
                    .submit_transaction(tx)
                    .await
                    .expect("Failed to submit transaction");
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
            for (account, balance) in &state[0].balances {
                println!("    - {}: {}", account, balance);
            }
            own_state = state.as_ref()[0].clone();
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
