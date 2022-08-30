use clap::Parser;
use hotshot::{
    demos::dentry::*,
    traits::{
        election::StaticCommittee,
        implementations::{CentralizedServerNetwork, MemoryStorage, Stateless},
    },
    types::{ed25519::Ed25519Priv, Event, EventType, HotShotHandle},
    HotShot, H_256,
};
use hotshot_centralized_server::NetworkConfig;
use hotshot_types::{
    traits::{
        signature_key::{ed25519::Ed25519Pub, SignatureKey},
        state::TestableState,
    },
    HotShotConfig,
};
use hotshot_utils::test_util::{setup_backtrace, setup_logging};
use rand_xoshiro::{rand_core::SeedableRng, Xoshiro256StarStar};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, SocketAddr},
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
    /// The address to connect to
    host: IpAddr,

    /// The port to connect to
    port: u16,
}

/// Creates the initial state and hotshot for simulation.
// TODO: remove `SecretKeySet` from parameters and read `PubKey`s from files.
async fn init_state_and_hotshot(
    networking: CentralizedServerNetwork<Ed25519Pub>,
    config: HotShotConfig<Ed25519Pub>,
    seed: [u8; 32],
    node_id: u64,
) -> (State, HotShotHandle<Node, H_256>) {
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
    let state = State {
        balances,
        nonces: BTreeSet::default(),
    };

    let priv_key = Ed25519Priv::generated_from_seed_indexed(seed, node_id);
    let pub_key = Ed25519Pub::from_private(&priv_key);
    let genesis = DEntryBlock::default();
    let known_nodes = config.known_nodes.clone();
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
    let addr: SocketAddr = (opts.host, opts.port).into();
    println!("Connecting to {addr:?} to retrieve the server config");

    let (config, network) = CentralizedServerNetwork::connect_with_server_config(addr).await;

    // Get networking information

    let node_count = config.config.total_nodes;

    println!("Waiting on connections...");
    loop {
        let connected_clients = network.get_connected_client_count().await;
        if connected_clients == node_count.get() {
            break;
        }
        println!("{} / {}", connected_clients, node_count);
        async_std::task::sleep(Duration::from_secs(1)).await;
    }

    let NetworkConfig {
        rounds,
        transactions_per_round,
        config,
        node_index,
        seed,
    } = config;

    let mut rng = Xoshiro256StarStar::from_seed(seed);
    // Initialize the state and hotshot
    let (mut own_state, mut hotshot) =
        init_state_and_hotshot(network, config, seed.clone(), node_index).await;
    hotshot.start().await;

    // Run random transactions
    println!("Running random transactions");
    debug!("Running random transactions");
    let mut round = 1;
    while round <= rounds {
        debug!(?round);
        println!("Round {}:", round);

        let num_submitted = if node_index == ((round % node_count) as u64) {
            tracing::info!("Generating txn for round {}", round);
            let state = hotshot.get_state().await;

            for _ in 0..transactions_per_round {
                let txn = <State as TestableState<H_256>>::create_random_transaction(&state);
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
        let mut event: Event<DEntryBlock, State, H_256> = hotshot
            .next_event()
            .await
            .expect("HotShot unexpectedly closed");
        while !matches!(event.event, EventType::Decide { .. }) {
            if matches!(event.event, EventType::Leader { .. }) {
                let tx = random_transaction(&own_state, &mut rng);
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
        println!("Node {} reached decision", node_index);
        debug!(?node_index, "Decision emitted");
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
