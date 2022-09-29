use clap::Parser;
use hotshot::{
    demos::dentry::*,
    traits::{
        election::StaticCommittee,
        implementations::{CentralizedServerNetwork, MemoryStorage, Stateless},
        Storage,
    },
    types::{ed25519::Ed25519Priv, HotShotHandle},
    HotShot,
};
use hotshot_centralized_server::{NetworkConfig, RunResults};
use hotshot_types::{
    traits::{
        signature_key::{ed25519::Ed25519Pub, SignatureKey},
        state::TestableState,
    },
    HotShotConfig,
};
use hotshot_utils::{
    art::{async_main, async_sleep},
    test_util::{setup_backtrace, setup_logging},
};
use std::{
    collections::{BTreeMap, VecDeque},
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
    // Create the initial block
    let accounts: BTreeMap<Account, Balance> = vec![
        ("Joe", 1_000_000),
        ("Nathan M", 500_000_000),
        ("John", 400_000_000),
        ("Nathan Y", 600_000_000),
        ("Ian", 300_000_000),
    ]
    .into_iter()
    .map(|(x, y)| (x.to_string(), y))
    .collect();
    let genesis_block = DEntryBlock::genesis_from(accounts);
    let initializer = hotshot::HotShotInitializer::from_genesis(genesis_block).unwrap();

    let priv_key = Ed25519Priv::generated_from_seed_indexed(seed, node_id);
    let pub_key = Ed25519Pub::from_private(&priv_key);
    let known_nodes = config.known_nodes.clone();
    let hotshot = HotShot::init(
        known_nodes.clone(),
        pub_key,
        priv_key,
        node_id,
        config,
        networking,
        MemoryStorage::new(),
        Stateless::default(),
        StaticCommittee::new(known_nodes),
        initializer,
    )
    .await
    .expect("Could not init hotshot");
    debug!("hotshot launched");

    let storage: &MemoryStorage<DEntryState> = hotshot.storage();

    let state = storage.get_anchored_view().await.unwrap().state;

    (state, hotshot)
}

#[async_main]
async fn main() {
    // Setup tracing listener
    setup_logging();
    setup_backtrace();

    let opts: NodeOpt = NodeOpt::parse();
    let addr: SocketAddr = (opts.host, opts.port).into();
    error!("Connecting to {addr:?} to retrieve the server config");

    let (config, run, network) = CentralizedServerNetwork::connect_with_server_config(addr).await;

    error!("Run: {:?}", run);
    error!("Config: {:?}", config);

    // Get networking information

    let node_count = config.config.total_nodes;

    debug!("Waiting on connections...");
    while !network.run_ready() {
        let connected_clients = network.get_connected_client_count().await;
        error!("{} / {}", connected_clients, node_count);
        async_sleep(Duration::from_secs(1)).await;
    }

    let NetworkConfig {
        rounds,
        transactions_per_round,
        config,
        node_index,
        seed,
        padding,
        libp2p_config: _,
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
                if let Some(state) = state.get(0) {
                    for (account, balance) in &state.balances {
                        debug!("    - {}: {}", account, balance);
                    }
                }
                for block in blocks {
                    total_transactions += block.txn_count();
                }
            }
            Err(e) => {
                timed_out_views += 1;
                error!("View: {:?}, failed with : {:?}", round, e);
            }
        }

        round += 1;
    }

    // Print metrics
    let total_time_elapsed = start.elapsed();
    let expected_transactions = transactions_per_round * rounds;
    let total_size = total_transactions * padding;
    error!("All {rounds} rounds completed in {total_time_elapsed:?}");
    error!("{timed_out_views} rounds timed out");

    // This assumes all submitted transactions make it through consensus:
    error!(
        "{} total bytes submitted in {:?}",
        total_size, total_time_elapsed
    );
    debug!("All rounds completed");

    let networking: &CentralizedServerNetwork<Ed25519Pub> = hotshot.networking();
    networking
        .send_results(RunResults {
            run,
            node_index,

            transactions_submitted: total_transactions,
            transactions_rejected: expected_transactions - total_transactions,
            transaction_size_bytes: total_size,

            rounds_succeeded: rounds as u64 - timed_out_views,
            rounds_timed_out: timed_out_views,
            total_time_in_seconds: total_time_elapsed.as_secs_f64(),
        })
        .await;
}
