use async_std::sync::RwLock;
use hotshot::{
    demos::dentry::*,
    traits::{
        election::StaticCommittee,
        implementations::{Libp2pNetwork, MemoryStorage, Stateless},
        NetworkError,
    },
    types::{HotShotHandle, Message},
    HotShot, HotShotConfig, H_256,
};
use hotshot_types::traits::{
    signature_key::{ed25519::Ed25519Pub, SignatureKey, TestableSignatureKey},
    state::TestableState,
};
use libp2p::{multiaddr, Multiaddr, PeerId};
use libp2p_networking::network::{NetworkNodeConfigBuilder, NetworkNodeType};

use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroUsize,
    str::FromStr,
    sync::Arc,
    time::Duration,
};
use structopt::StructOpt;

use tracing::debug;

/// convert node string into multi addr
pub fn parse_node(s: &str) -> Result<Multiaddr, multiaddr::Error> {
    let mut i = s.split(':');
    let ip = i.next().ok_or(multiaddr::Error::InvalidMultiaddr)?;
    let port = i.next().ok_or(multiaddr::Error::InvalidMultiaddr)?;
    Multiaddr::from_str(&format!("/ip4/{}/tcp/{}", ip, port))
}

#[derive(StructOpt, Debug)]
#[structopt(
    name = "Multi-machine consensus",
    about = "Simulates consensus among multiple machines"
)]
pub struct CliOpt {
    /// list of bootstrap node addrs
    #[structopt(long = "to_connect_addrs")]
    #[structopt(parse(try_from_str = parse_node), use_delimiter = true)]
    pub to_connect_addrs: Vec<Multiaddr>,
    /// total number of nodes
    #[structopt(long = "num_nodes")]
    pub num_nodes: usize,
    /// the role this node plays
    #[structopt(long = "node_type")]
    pub node_type: NetworkNodeType,
    /// internal interface to bind to
    #[structopt(long = "bound_addr")]
    #[structopt(parse(try_from_str = parse_node))]
    pub bound_addr: Multiaddr,

    /// number of txns to run
    #[structopt(long = "num_txns")]
    pub num_txns: u32,

    /// Id of the current node
    #[structopt(long = "id", short = "i", default_value = "0")]
    pub id: u64,

    /// seed used to generate ids
    #[structopt(long = "seed")]
    pub seed: u64,
}

type Node = DEntryNode<
    Libp2pNetwork<Message<DEntryBlock, Transaction, State, Ed25519Pub, H_256>, Ed25519Pub>,
>;

/// Creates the initial state and hotshot for simulation.
// TODO: remove `SecretKeySet` from parameters and read `PubKey`s from files.
async fn init_state_and_hotshot(
    nodes: usize,
    threshold: usize,
    node_id: u64,
    networking: Libp2pNetwork<
        Message<DEntryBlock, Transaction, State, Ed25519Pub, H_256>,
        Ed25519Pub,
    >,
) -> (State, HotShotHandle<Node, H_256>) {
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

    // Create the initial hotshot
    let known_nodes: Vec<_> = (0..nodes as u64)
        .map(|x| {
            let priv_key = Ed25519Pub::generate_test_key(x);
            Ed25519Pub::from_private(&priv_key)
        })
        .collect();

    let config = HotShotConfig {
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
    };
    debug!(?config);
    let priv_key = Ed25519Pub::generate_test_key(node_id);
    let pub_key = Ed25519Pub::from_private(&priv_key);
    let genesis = DEntryBlock::default();
    let hotshot = HotShot::init(
        genesis,
        known_nodes.clone(),
        pub_key,
        priv_key,
        node_id,
        config,
        state.clone(),
        networking,
        MemoryStorage::default(),
        Stateless::default(),
        StaticCommittee::new(known_nodes),
    )
    .await
    .expect("Could not init hotshot");
    debug!("hotshot launched");

    (state, hotshot)
}

pub async fn new_libp2p_network(
    pubkey: Ed25519Pub,
    bs: Vec<Multiaddr>,
    node_id: usize,
    node_type: NetworkNodeType,
    bound_addr: Multiaddr,
) -> Result<
    Libp2pNetwork<Message<DEntryBlock, Transaction, State, Ed25519Pub, H_256>, Ed25519Pub>,
    NetworkError,
> {
    let config = NetworkNodeConfigBuilder::default()
        .replication_factor(NonZeroUsize::new(20).unwrap())
        .node_type(node_type)
        .max_num_peers(15)
        .min_num_peers(4)
        .bound_addr(bound_addr)
        .build()
        .unwrap();
    let bs: Vec<(Option<PeerId>, Multiaddr)> = bs.into_iter().map(|addr| (None, addr)).collect();
    Libp2pNetwork::new(config, pubkey, Arc::new(RwLock::new(bs)), node_id as usize).await
}

#[async_std::main]
async fn main() {
    let args = CliOpt::from_args();
    let to_connect_addrs = args.to_connect_addrs;
    let num_nodes = args.num_nodes;
    let bound_addr = args.bound_addr;
    let num_views = args.num_txns;
    let node_type = args.node_type;
    let own_id = args.id;
    let threshold = ((num_nodes * 2) / 3) + 1;
    println!("starting main");

    let own_priv_key = Ed25519Pub::generate_test_key(own_id);
    let own_pub_key = Ed25519Pub::from_private(&own_priv_key);

    println!("Done with keygen");
    let own_network = new_libp2p_network(
        own_pub_key,
        to_connect_addrs,
        own_id as usize,
        node_type,
        bound_addr,
    )
    .await
    .unwrap();

    println!("Done with network creation");

    // Initialize the state and hotshot
    let (_own_state, mut hotshot) =
        init_state_and_hotshot(num_nodes, threshold, own_id, own_network).await;

    println!("Finished init, starting hotshot!");
    hotshot.start().await;

    println!("waiting for connections to hotshot!");
    hotshot.is_ready().await;

    // Run random transactions
    let mut num_failed_views = 0;
    for view in 0..num_views {
        println!("Beginning view {}", view);
        if (own_id % num_views as u64) == view as u64 {
            println!("Generating txn for view {}", view);
            let state = hotshot.get_state().await.unwrap().unwrap();

            let txn = <State as TestableState<H_256>>::create_random_transaction(&state);
            println!("Submitting txn on view {}", view);
            hotshot.submit_transaction(txn).await.unwrap();
        }
        println!("Running the view {}", view);
        hotshot.run_one_round().await;
        println!("Collection for view {}", view);
        let result = hotshot.collect_round_events().await;
        match result {
            Ok(state) => {
                println!("View {:?}: successful with {:?}", view, state);
            }
            Err(e) => {
                num_failed_views += 1;
                println!("View: {:?}, failed with : {:?}", view, e);
            }
        }
    }

    println!(
        "All rounds completed, {} rounds with {} failures",
        num_views, num_failed_views
    );
}
