use clap::Parser;
use hotshot::{
    demos::dentry::*,
    traits::{
        election::StaticCommittee,
        implementations::{CentralizedServerNetwork, MemoryStorage, Stateless},
    },
    types::{ed25519::Ed25519Priv, HotShotHandle},
    HotShot,
};
use hotshot_centralized_server::NetworkConfig;
use hotshot_types::traits::block_contents::Genesis;
use hotshot_types::{
    traits::{
        signature_key::{ed25519::Ed25519Pub, SignatureKey},
        state::TestableState,
    },
    HotShotConfig,
};
use hotshot_utils::test_util::{setup_backtrace, setup_logging};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    mem,
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
};
use tracing::{debug, error};

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

    let priv_key = Ed25519Priv::generated_from_seed_indexed(seed, node_id);
    let pub_key = Ed25519Pub::from_private(&priv_key);
    let genesis = DEntryBlock::genesis();
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
    error!("Connecting to {addr:?} to retrieve the server config");

    let (config, network) = CentralizedServerNetwork::connect_with_server_config(addr).await;

    // Get networking information

    let node_count = config.config.total_nodes;

    debug!("Waiting on connections...");
    loop {
        let connected_clients = network.get_connected_client_count().await;
        if connected_clients == node_count.get() {
            break;
        }
        debug!("{} / {}", connected_clients, node_count);
        async_std::task::sleep(Duration::from_secs(1)).await;
    }

    let NetworkConfig {
        rounds,
        transactions_per_round,
        config,
        node_index,
        seed,
        padding,
    } = config;

    // Initialize the state and hotshot
    let (_own_state, mut hotshot) = init_state_and_hotshot(network, config, seed, node_index).await;

    hotshot.start().await;

    let size = mem::size_of::<DEntryTransaction>();
    let adjusted_padding = if padding < size { 0 } else { padding - size };
    let mut txs: VecDeque<DEntryTransaction> = VecDeque::new();
    let state = hotshot.get_state().await;
    for _ in 0..((transactions_per_round * rounds) / node_count) + 1 {
        let mut txn = <DEntryState as TestableState>::create_random_transaction(&state);
        txn.padding = vec![0; adjusted_padding];
        txs.push_back(txn);
    }

    let start = Instant::now();

    // Run random transactions
    debug!("Running random transactions");
    error!("Adjusted padding size is = {:?}", adjusted_padding);
    let mut timed_out_views: u64 = 0;
    let mut round = 1;
    let mut total_transactions = 0;

    while round <= rounds {
        debug!(?round);
        error!("Round {}:", round);

        let num_submitted = if node_index == ((round % node_count) as u64) {
            tracing::info!("Generating txn for round {}", round);

            for _ in 0..transactions_per_round {
                let txn = txs.pop_front().unwrap();
                tracing::info!("Submitting txn on round {}", round);
                hotshot.submit_transaction(txn).await.unwrap();
            }
            transactions_per_round
        } else {
            0
        };
        error!("Submitting {} transactions", num_submitted);

        // Start consensus
        error!("  - Waiting for consensus to occur");
        debug!("Waiting for consensus to occur");

        let view_results = hotshot.collect_round_events().await;

        match view_results {
            Ok((state, blocks)) => {
                for (account, balance) in &state[0].balances {
                    debug!("    - {}: {}", account, balance);
                }
                for block in blocks {
                    total_transactions += block.transactions.len();
                }
            }
            Err(e) => {
                timed_out_views += 1;
                error!("View: {:?}, failed with : {:?}", round, e);
            }
        }

        round += 1;
    }
    let end = Instant::now();

    // Print metrics
    let total_time_elapsed = end - start;
    let total_size = total_transactions * padding;
    println!("All {} rounds completed in {:?}", rounds, end - start);
    error!("{} rounds timed out", timed_out_views);
    // This assumes all submitted transactions make it through consensus:
    error!(
        "{} total bytes submitted in {:?}",
        total_size, total_time_elapsed
    );
    debug!("All rounds completed");
}
