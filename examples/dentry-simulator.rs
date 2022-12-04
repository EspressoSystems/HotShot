use clap::Parser;
use futures::future::join_all;
use hotshot_types::data::{LeafType, ProposalType, ValidatingLeaf, ValidatingProposal};
use hotshot_types::traits::node_implementation::NodeTypes;
use hotshot_types::traits::{
    metrics::NoMetrics, signature_key::SignatureKey, state::TestableState,election::Election
};
use hotshot_types::{ExecutionType, HotShotConfig};
use hotshot_utils::{
    art::{async_main, async_sleep, async_spawn},
    test_util::{setup_backtrace, setup_logging},
};
use rand_xoshiro::{rand_core::SeedableRng, Xoshiro256StarStar};
use std::{
    num::NonZeroUsize,
    time::{Duration, Instant},
};
use tracing::{debug, error, instrument};

use hotshot::{
    demos::dentry::*,
    traits::{
        election::{
            static_committee::{StaticCommittee, StaticElectionConfig},
            vrf::BlsPubKey,
        },
        implementations::{MemoryStorage, WNetwork},
    },
    types::{Event, EventType, HotShotHandle},
    HotShot,
};

type Node<ELECTION: Election<DEntryTypes, ValidatingLeaf<DEntryTypes>>> = DEntryNode<
    WNetwork<DEntryTypes, ValidatingLeaf<DEntryTypes>, ValidatingProposal<DEntryTypes, ELECTION>>,
    StaticCommittee<DEntryTypes, ValidatingLeaf<DEntryTypes>>,
    ELECTION,
>;

#[derive(Debug, Parser)]
#[command(
    name = "Double Entry Simulator",
    about = "Simulates consensus among a number of nodes"
)]
struct Opt {
    /// Number of nodes to run
    #[arg(short = 'n', default_value = "7")]
    nodes: usize,
    /// Number of transactions to simulate
    #[arg(short = 't', default_value = "10")]
    transactions: usize,
}
/// Prebaked list of transactions
fn prebaked_transactions() -> Vec<DEntryTransaction> {
    vec![
        DEntryTransaction {
            add: Addition {
                account: "Ian".to_string(),
                amount: 100,
            },
            sub: Subtraction {
                account: "Joe".to_string(),
                amount: 100,
            },
            nonce: 0,
            padding: vec![0; 0],
        },
        DEntryTransaction {
            add: Addition {
                account: "John".to_string(),
                amount: 25,
            },
            sub: Subtraction {
                account: "Joe".to_string(),
                amount: 25,
            },
            nonce: 1,
            padding: vec![0; 0],
        },
    ]
}

#[async_main]
#[instrument]
async fn main() {
    // Setup tracing listener
    setup_logging();
    setup_backtrace();

    // Get options
    let opt = Opt::parse();
    debug!(?opt);

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
        WNetwork<
            DEntryTypes,
            LeafType<NodeType = DEntryTypes>,
            ProposalType<NodeTypes = DEntryTypes>,
        >,
        u16,
        BlsPubKey,
        u64,
    )> = Vec::new();
    for node_id in 0..nodes as u64 {
        let (public_key, _private_key) =
            <BlsPubKey as SignatureKey>::generated_from_seed_indexed([0_u8; 32], node_id);
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
            async_sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    // Create the hotshots
    let mut hotshots: Vec<
        HotShotHandle<DEntryTypes, Node<Election<DEntryTypes, ValidatingLeaf<DEntryTypes>>>>,
    > = join_all(
        networkings
            .into_iter()
            .map(|(network, _, _pk, node_id)| get_hotshot(nodes, node_id, network)),
    )
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
            hotshot.start_one_round().await;
        }
        println!("  - Waiting for consensus to occur");
        debug!("Waiting for consensus to occur");
        let mut blocks: Vec<Vec<DEntryBlock>> = Vec::new();
        let mut states: Vec<Vec<DEntryState>> = Vec::new();
        for (node_id, hotshot) in hotshots.iter_mut().enumerate() {
            debug!(?node_id, "Waiting on node to emit decision");
            let mut event: Event<DEntryTypes, LeafType<NodeType = DEntryTypes>> = hotshot
                .next_event()
                .await
                .expect("HotShot unexpectedly closed");
            while !matches!(event.event, EventType::Decide { .. }) {
                if matches!(event.event, EventType::ReplicaViewTimeout { .. }) {
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
            if let EventType::Decide { leaf_chain, .. } = event.event {
                let (block, state) = leaf_chain
                    .iter()
                    .cloned()
                    .map(|leaf| (leaf.deltas, leaf.state))
                    .unzip();
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
        let tx = &state.as_ref().unwrap()[0].create_random_transaction(&mut rng);
        println!("Round {}:", round);
        println!("  - Proposing: {:?}", tx);
        debug!("Proposing: {:?}", tx);
        hotshots[0]
            .submit_transaction(tx.clone())
            .await
            .expect("Failed to submit transaction");
        println!("  - Unlocking round");
        debug!("Unlocking round");
        for hotshot in &hotshots {
            hotshot.start_one_round().await;
        }
        println!("  - Waiting for consensus to occur");
        debug!("Waiting for consensus to occur");
        let mut blocks: Vec<Vec<DEntryBlock>> = Vec::new();
        let mut states: Vec<Vec<DEntryState>> = Vec::new();
        for (node_id, hotshot) in hotshots.iter_mut().enumerate() {
            debug!(?node_id, "Waiting on node to emit decision");
            let mut event: Event<DEntryTypes> = hotshot
                .next_event()
                .await
                .expect("HotShot unexpectedly closed");
            while !matches!(event.event, EventType::Decide { .. }) {
                if matches!(event.event, EventType::ReplicaViewTimeout { .. }) {
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
            if let EventType::Decide { leaf_chain, .. } = event.event {
                let (block, state) = leaf_chain
                    .iter()
                    .cloned()
                    .map(|leaf| (leaf.deltas, leaf.state))
                    .unzip();
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

/// Trys to get a networking implementation with the given id
///
/// also starts the background task
#[instrument(skip(rng))]
async fn get_networking<R: hotshot::rand::Rng>(
    pub_key: BlsPubKey,
    listen_addr: &str,
    node_id: u64,
    rng: &mut R,
) -> (WNetwork<DEntryTypes>, u16, BlsPubKey, u64) {
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
                        async_spawn(x);
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
// #[instrument(skip(networking))]
async fn get_hotshot<
    LEAF: LeafType<NodeType = DEntryTypes>,
    PROPOSAL: ProposalType<NodeTypes = DEntryTypes>,
>(
    nodes: usize,
    node_id: u64,
    networking: WNetwork<DEntryTypes, LEAF, PROPOSAL>,
) -> HotShotHandle<DEntryTypes, Node> {
    let known_nodes: Vec<_> = (0..nodes)
        .map(|x| <BlsPubKey as SignatureKey>::generated_from_seed_indexed([0_u8; 32], x as u64).0)
        .collect();
    let config = HotShotConfig {
        execution_type: ExecutionType::Continuous,
        total_nodes: NonZeroUsize::new(nodes).unwrap(),
        max_transactions: NonZeroUsize::new(100).unwrap(),
        min_transactions: 0,
        known_nodes: known_nodes.clone(),
        next_view_timeout: 100000,
        timeout_ratio: (11, 10),
        round_start_delay: 1,
        start_delay: 1,
        propose_min_round_time: Duration::from_millis(0),
        propose_max_round_time: Duration::from_millis(1000),
        num_bootstrap: 5,
        election_config: Some(StaticElectionConfig {}),
    };
    let (public_key, private_key) = BlsPubKey::generated_from_seed_indexed([0_u8; 32], node_id);
    let genesis_block = DEntryBlock::genesis();
    let initializer = hotshot::HotShotInitializer::from_genesis(genesis_block).unwrap();
    let h = HotShot::<DEntryTypes, Node>::init(
        public_key,
        private_key,
        node_id,
        config,
        networking,
        MemoryStorage::new(),
        StaticCommittee::new(known_nodes),
        initializer,
        NoMetrics::new(),
    )
    .await
    .expect("Could not init hotshot");
    debug!("hotshot launched");
    h
}
