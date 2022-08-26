use clap::Parser;
use hotshot::{
    demos::dentry::*,
    traits::{
        election::StaticCommittee,
        implementations::{CentralizedServerNetwork, MemoryStorage, Stateless},
    },
    types::{ed25519::Ed25519Priv, Event, EventType, HotShotHandle},
    HotShot, HotShotConfig,
};
use hotshot_types::traits::{
    signature_key::{ed25519::Ed25519Pub, SignatureKey, TestableSignatureKey},
    state::TestableState,
};
use hotshot_utils::test_util::{setup_backtrace, setup_logging};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, SocketAddr},
    num::NonZeroUsize,
    time::Duration,
};
use tracing::debug;

type Node = DEntryNode<CentralizedServerNetwork<Ed25519Pub>>;

#[derive(Debug, Parser)]
#[clap(
    name = "Multi-machine consensus",
    about = "Simulates consensus among multiple machines"
)]
struct NodeOpt {
    #[clap(long = "server")]
    /// The address to connect to
    host: IpAddr,

    #[clap(long = "port", short = 'p')]
    /// The port to connect to
    port: u16,

    #[clap(long = "seed", short = 's', default_value = "0")]
    /// The common of the nodes
    seed: u64,

    #[clap(long = "total", short = 't')]
    /// The total amount of nodes that will be connecting
    total_nodes: usize,

    #[clap(long = "id", short = 'i')]
    /// Id of the current node
    id: u64,

    #[clap(long = "rounds", short = 'r', default_value = "10")]
    /// Total amount of rounds to run
    rounds: u64,
}

/// Creates the initial state and hotshot for simulation.
// TODO: remove `SecretKeySet` from parameters and read `PubKey`s from files.
async fn init_state_and_hotshot(
    nodes: usize,
    threshold: usize,
    node_id: u64,
    networking: CentralizedServerNetwork<Ed25519Pub>,
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
    let state = DEntryState {
        balances,
        nonces: BTreeSet::default(),
    };

    // Create the initial hotshot
    let known_nodes: Vec<_> = (0..nodes as u64)
        .map(|x| {
            let priv_key = Ed25519Pub::generate_test_key(x);
            Ed25519Pub::from_private(&priv_key)
        })
        .collect();

    let config = HotShotConfig {
        execution_type: hotshot::ExecutionType::Continuous,
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
        num_bootstrap: 0,
    };
    debug!(?config);
    let priv_key = Ed25519Pub::generate_test_key(node_id);
    let pub_key = Ed25519Pub::from_private(&priv_key);
    let genesis = DEntryBlock::default();
    let hotshot = HotShot::init(
        known_nodes.clone(),
        pub_key,
        priv_key,
        node_id,
        config,
        networking,
        MemoryStorage::new(genesis, state.clone()),
        Stateless::default(),
        StaticCommittee::new(known_nodes),
    )
    .await
    .expect("Could not init hotshot");
    debug!("hotshot launched");

    (state, hotshot)
}

#[async_std::main]
async fn main() {
    // Setup tracing listener
    setup_logging();
    setup_backtrace();

    let opts: NodeOpt = NodeOpt::from_args();
    let own_id = opts.id;
    println!("  - Spawning network for node {}", own_id);

    // Read node info from node configuration file
    // let node_config: Value =
    //     toml::from_str(&config_str).expect("Error while reading node config file");

    // Get secret key set
    let seed = {
        let mut bytes = [0u8; 32];
        bytes[0..8].copy_from_slice(&opts.seed.to_le_bytes());
        bytes[8..16].copy_from_slice(&opts.seed.to_le_bytes());
        bytes[16..24].copy_from_slice(&opts.seed.to_le_bytes());
        bytes[24..32].copy_from_slice(&opts.seed.to_le_bytes());
        bytes
    };

    let nodes: usize = opts.total_nodes;
    let threshold = ((nodes * 2) / 3) + 1;

    let known_nodes: Vec<Ed25519Pub> = (0..nodes)
        .map(|id| {
            let private_key = Ed25519Priv::generated_from_seed_indexed(seed, id as u64);
            Ed25519Pub::from_private(&private_key)
        })
        .collect();

    // Get networking information
    let own_pub = known_nodes[own_id as usize];
    println!("Own ID: {:?}", own_pub);
    let network = CentralizedServerNetwork::connect(
        known_nodes,
        SocketAddr::from((opts.host, opts.port)),
        own_pub,
    );

    println!("Waiting on connections...");
    loop {
        let connected_clients = network.get_connected_client_count().await;
        if connected_clients == nodes {
            break;
        }
        println!("{} / {}", connected_clients, nodes);
        async_std::task::sleep(Duration::from_secs(1)).await;
    }

    // Initialize the state and hotshot
    let (mut own_state, mut hotshot) =
        init_state_and_hotshot(nodes, threshold, own_id, network).await;
    hotshot.start().await;

    // Run random transactions
    println!("Running random transactions");
    debug!("Running random transactions");
    let mut round = 1;
    while round <= opts.rounds {
        debug!(?round);
        println!("Round {}:", round);

        let num_submitted = if own_id == (round % opts.total_nodes as u64) {
            tracing::info!("Generating txn for round {}", round);
            let state = hotshot.get_state().await;

            for _ in 0..10 {
                let txn = <DEntryState as TestableState>::create_random_transaction(&state);
                tracing::info!("Submitting txn on round {}", round);
                hotshot.submit_transaction(txn).await.unwrap();
            }
            10
        } else {
            0
        };
        println!("Submitting {} transactions", num_submitted);

        // Start consensus
        println!("  - Waiting for consensus to occur");
        debug!("Waiting for consensus to occur");
        let mut event: Event<DEntryState> = hotshot
            .next_event()
            .await
            .expect("HotShot unexpectedly closed");
        while !matches!(event.event, EventType::Decide { .. }) {
            if matches!(event.event, EventType::Leader { .. }) {
                let tx = &own_state.create_random_transaction();
                println!("  - Proposing: {:?}", tx);
                debug!("Proposing: {:?}", tx);
                hotshot
                    .submit_transaction(tx.clone())
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
        if let EventType::Decide { state, .. } = event.event {
            println!("  - Balances:");
            for (account, balance) in &state[0].balances {
                println!("    - {}: {}", account, balance);
            }
            own_state = state.as_ref()[0].clone();
        } else {
            unreachable!()
        }
        round += 1;
    }

    println!("All rounds completed");
    debug!("All rounds completed");
}
