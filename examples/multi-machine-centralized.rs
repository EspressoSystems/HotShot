use async_compatibility_layer::{
    art::async_sleep,
    logging::{setup_backtrace, setup_logging},
};
use clap::Parser;
use hotshot::{
    demos::dentry::*,
    traits::{
        election::static_committee::{GeneralStaticCommittee, StaticElectionConfig},
        implementations::{CentralizedCommChannel, CentralizedServerNetwork, MemoryStorage},
        Storage,
    },
    types::{ed25519::Ed25519Priv, HotShotHandle},
    HotShot,
};
use hotshot_centralized_server::{NetworkConfig, RunResults};
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal},
    traits::{
        metrics::NoMetrics,
        node_implementation::NodeType,
        signature_key::{ed25519::Ed25519Pub, SignatureKey},
        state::TestableState,
    },
    HotShotConfig,
};
use std::{
    cmp,
    collections::{BTreeMap, VecDeque},
    mem,
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
};
use tracing::{debug, error, instrument};

type ThisLeaf = ValidatingLeaf<DEntryTypes>;
type ThisElection =
    GeneralStaticCommittee<DEntryTypes, ThisLeaf, <DEntryTypes as NodeType>::SignatureKey>;
type ThisNetworking = CentralizedCommChannel<DEntryTypes>;
#[allow(dead_code)]
type ThisProposal = ValidatingProposal<DEntryTypes, ThisElection>;
type Node = DEntryNode<ThisNetworking, ThisElection>;

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
    networking: ThisNetworking,
    config: HotShotConfig<Ed25519Pub, StaticElectionConfig>,
    seed: [u8; 32],
    node_id: u64,
) -> (DEntryState, HotShotHandle<DEntryTypes, Node>) {
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
        pub_key,
        priv_key,
        node_id,
        config,
        networking,
        MemoryStorage::new(),
        GeneralStaticCommittee::new(known_nodes),
        initializer,
        NoMetrics::new(),
    )
    .await
    .expect("Could not init hotshot");
    debug!("hotshot launched");

    let storage: &MemoryStorage<DEntryTypes, ThisLeaf> = hotshot.storage();

    let state = storage.get_anchored_view().await.unwrap().state;

    (state, hotshot)
}

#[cfg_attr(
    feature = "tokio-executor",
    tokio::main(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::main)]
#[instrument]
async fn main() {
    // Setup tracing listener
    setup_logging();
    setup_backtrace();

    let mut rng = rand::thread_rng();
    let opts: NodeOpt = NodeOpt::parse();
    let addr: SocketAddr = (opts.host, opts.port).into();
    error!("Connecting to {addr:?} to retrieve the server config");

    let (config, run, network) =
        CentralizedServerNetwork::connect_with_server_config(NoMetrics::new(), addr).await;
    let network = CentralizedCommChannel::new(network);

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
        start_delay_seconds: _,
        key_type_name,
        election_config_type_name,
    } = config;
    assert_eq!(key_type_name, std::any::type_name::<Ed25519Pub>());
    assert_eq!(
        election_config_type_name,
        std::any::type_name::<StaticElectionConfig>()
    );

    // Initialize the state and hotshot
    let (_own_state, mut hotshot) = init_state_and_hotshot(network, config, seed, node_index).await;

    hotshot.start().await;

    let size = mem::size_of::<DEntryTransaction>();
    let adjusted_padding = if padding < size { 0 } else { padding - size };
    let mut txs: VecDeque<DEntryTransaction> = VecDeque::new();
    let state = hotshot.get_state().await;
    // This assumes that no node will be a leader more than 5x the expected number of times they should be the leader
    let tx_to_gen = transactions_per_round * (cmp::max(rounds / node_count, 1) + 5);
    error!("Generated {} transactions", tx_to_gen);
    for _ in 0..tx_to_gen {
        let mut txn = <DEntryState as TestableState>::create_random_transaction(&state, &mut rng);
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

    let networking: &ThisNetworking = hotshot.networking();
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
