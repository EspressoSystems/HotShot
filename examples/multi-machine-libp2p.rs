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
use hotshot_utils::test_util::{setup_backtrace, setup_logging};
use libp2p::{identity::Keypair, multiaddr, Multiaddr, PeerId};
use libp2p_networking::network::{MeshParams, NetworkNodeConfigBuilder, NetworkNodeType};

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    num::NonZeroUsize,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use structopt::StructOpt;

use tracing::{debug, error, info};

/// convert node string into multi addr
/// node string of the form: "$IP:$PORT"
pub fn parse_node(s: &str) -> Result<Multiaddr, multiaddr::Error> {
    let mut i = s.split(':');
    let ip = i.next().ok_or(multiaddr::Error::InvalidMultiaddr)?;
    let port = i.next().ok_or(multiaddr::Error::InvalidMultiaddr)?;
    Multiaddr::from_str(&format!("/ip4/{}/tcp/{}", ip, port))
}

// FIXME make these actual ips/ports
/// bootstrap hardcoded metadata
pub const BOOTSTRAPS: &[(&[u8], &str)] = &[
    (
        include_bytes!("../deploy/keys/private_1.pk8"),
        // "18.216.113.34:9003",
        "127.0.0.1:9100",
    ),
    (
        include_bytes!("../deploy/keys/private_2.pk8"),
        // "18.117.245.103:9003",
        "127.0.0.1:9101",
    ),
    (
        include_bytes!("../deploy/keys/private_3.pk8"),
        // "13.58.161.60:9003",
        "127.0.0.1:9102",
    ),
    (
        include_bytes!("../deploy/keys/private_4.pk8"),
        "127.0.0.1:9103",
        // "3.111.188.178:9003",
    ),
    (
        include_bytes!("../deploy/keys/private_5.pk8"),
        "127.0.0.1:9104",
        // "52.66.253.105:9003",
    ),
    (
        include_bytes!("../deploy/keys/private_6.pk8"),
        "127.0.0.1:9105",
        // "34.219.31.18:9003",
    ),
    (
        include_bytes!("../deploy/keys/private_7.pk8"),
        "127.0.0.1:9106",
        // "54.184.243.4:9003",
    ),
];

#[derive(StructOpt, Debug)]
#[structopt(
    name = "Multi-machine consensus",
    about = "Simulates consensus among multiple machines"
)]
pub struct CliOpt {
    /// num nodes
    #[structopt(long = "num_nodes")]
    pub num_nodes: usize,

    /// num bootstrap
    #[structopt(long = "num_bootstrap")]
    pub num_bootstrap: usize,

    /// num transactions to be submitted per round
    #[structopt(long = "num_txn_per_round")]
    pub num_txn_per_round: usize,

    /// Id of the current node
    #[structopt(long = "node_idx")]
    pub node_idx: usize,

    /// how long to run for
    #[structopt(long = "online_time", default_value = "60")]
    pub online_time: u64,

    /// address to bind to
    #[structopt(long = "bound_addr")]
    #[structopt(parse(try_from_str = parse_node))]
    pub bound_addr: Multiaddr,

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
        next_view_timeout: 1000,
        timeout_ratio: (11, 10),
        round_start_delay: 1,
        start_delay: 1,
        propose_min_round_time: Duration::from_millis(0),
        propose_max_round_time: Duration::from_millis(1000),
        num_bootstrap: 7,
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
    bs: Vec<(Option<PeerId>, Multiaddr)>,
    node_id: usize,
    node_type: NetworkNodeType,
    bound_addr: Multiaddr,
    num_nodes: usize,
    identity: Option<Keypair>,
) -> Result<
    Libp2pNetwork<Message<DEntryBlock, Transaction, State, Ed25519Pub, H_256>, Ed25519Pub>,
    NetworkError,
> {
    // TODO identity
    let mut config_builder = NetworkNodeConfigBuilder::default();
    // NOTE we may need to change this as we scale
    config_builder.replication_factor(NonZeroUsize::new(num_nodes - 2).unwrap());
    config_builder.to_connect_addrs(HashSet::new());
    config_builder.node_type(node_type);
    config_builder.bound_addr(Some(bound_addr));

    if let Some(identity) = identity {
        config_builder.identity(identity);
    }

    let mesh_params =
        // NOTE I'm arbitrarily choosing these.
        match node_type {
            NetworkNodeType::Bootstrap => MeshParams {
                mesh_n_high: 50,
                mesh_n_low: 10,
                mesh_outbound_min: 5,
                mesh_n: 15,
            },
            NetworkNodeType::Regular => MeshParams {
                mesh_n_high: 15,
                mesh_n_low: 8,
                mesh_outbound_min: 4,
                mesh_n: 12,
            },
            NetworkNodeType::Conductor => unreachable!(),
        };

    config_builder.mesh_params(Some(mesh_params));

    let config = config_builder.build().unwrap();

    // TODO add in mesh parameters based on node type
    Libp2pNetwork::new(
        config,
        pubkey,
        Arc::new(RwLock::new(bs.clone())),
        bs.len(),
        node_id as usize,
    )
    .await
}

#[async_std::main]
async fn main() {
    setup_logging();
    setup_backtrace();

    let args = CliOpt::from_args();

    let bootstrap_priv: Vec<_> = BOOTSTRAPS
        .iter()
        .map(|(key_bytes, addr_str)| {
            let mut key_bytes = <&[u8]>::clone(key_bytes).to_vec();
            // TODO better error handling
            let key = Keypair::rsa_from_pkcs8(&mut key_bytes).unwrap();
            let multiaddr = parse_node(addr_str).unwrap();
            (key, multiaddr)
        })
        .take(args.num_bootstrap)
        .collect();

    let to_connect_addrs: Vec<_> = bootstrap_priv
        .clone()
        .into_iter()
        .map(|(kp, ma)| (Some(PeerId::from_public_key(&kp.public())), ma))
        .collect();

    let own_id = args.node_idx;
    let num_nodes = args.num_nodes;
    let bound_addr = args.bound_addr;
    let (node_type, own_identity) = if own_id < args.num_bootstrap {
        (
            NetworkNodeType::Bootstrap,
            Some(bootstrap_priv[own_id].0.clone()),
        )
    } else {
        (NetworkNodeType::Regular, None)
    };

    let threshold = ((num_nodes * 2) / 3) + 1;

    let own_priv_key = Ed25519Pub::generate_test_key(own_id as u64);
    let own_pub_key = Ed25519Pub::from_private(&own_priv_key);

    error!("Done with keygen");
    let own_network = new_libp2p_network(
        own_pub_key,
        to_connect_addrs,
        own_id as usize,
        node_type,
        bound_addr,
        num_nodes,
        own_identity,
    )
    .await
    .unwrap();

    error!("Done with network creation");

    // Initialize the state and hotshot
    let (_own_state, mut hotshot) =
        init_state_and_hotshot(num_nodes, threshold, own_id as u64, own_network).await;

    error!("Finished init, starting hotshot!");
    hotshot.start().await;

    error!("waiting for connections to hotshot!");
    hotshot.is_ready().await;
    error!("We are ready!");

    let start_time = Instant::now();

    let mut view = 0;

    // Run random transactions until failure
    let mut num_failed_views = 0;

    let online_time = Duration::from_secs(60 * args.online_time);

    let mut total_successful_txns = 0;
    let mut total_txns = 0;

    while start_time + online_time > Instant::now() {
        error!("Beginning view {}", view);
        let num_submitted = {
            if own_id == (view % num_nodes) {
                println!("Generating txn for view {}", view);
                let state = hotshot.get_state().await.unwrap().unwrap();

                for _ in 0..10 {
                    let txn = <State as TestableState<H_256>>::create_random_transaction(&state);
                    info!("Submitting txn on view {}", view);
                    hotshot.submit_transaction(txn).await.unwrap();
                }
                total_txns += 10;
                10
            } else {
                0
            }
        };
        error!("Running the view {}", view);
        hotshot.run_one_round().await;
        error!("Collection for view {}", view);
        let result = hotshot.collect_round_events().await;
        match result {
            Ok(state) => {
                total_successful_txns += num_submitted;
                error!(
                    "View {:?}: successful with {:?}, and total successful txns {:?}",
                    view, state, total_successful_txns
                );
            }
            Err(e) => {
                num_failed_views += 1;
                error!("View: {:?}, failed with : {:?}", view, e);
            }
        }
        view += 1;
    }

    error!(
        "All rounds completed, {} views with {} failures. This node (id: {:?}) submitted {:?} txns, and {:?} were successful",
        view,
        num_failed_views,
        own_id,
        total_txns,
        total_successful_txns
    );
}
