use async_std::sync::RwLock;
use clap::Parser;
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

use tracing::{debug, error, info};

/// convert node string into multi addr
/// node string of the form: "$IP:$PORT"
pub fn parse_dns(s: &str) -> Result<Multiaddr, multiaddr::Error> {
    let mut i = s.split(':');
    let ip = i.next().ok_or(multiaddr::Error::InvalidMultiaddr)?;
    let port = i.next().ok_or(multiaddr::Error::InvalidMultiaddr)?;
    Multiaddr::from_str(&format!("/dns/{}/tcp/{}", ip, port))
}

pub fn parse_ip(s: &str) -> Result<Multiaddr, multiaddr::Error> {
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
        // "127.0.0.1:9100",
        // "18.216.113.34:9000",
        "0.ap-south-1.cluster.aws.espresso.network:9000",
    ),
    (
        include_bytes!("../deploy/keys/private_2.pk8"),
        // "127.0.0.1:9101",
        // "18.117.245.103:9000",
        "1.ap-south-1.cluster.aws.espresso.network:9000",
    ),
    (
        include_bytes!("../deploy/keys/private_3.pk8"),
        // "127.0.0.1:9102",
        // "13.58.161.60:9000",
        "0.us-east-2.cluster.aws.espresso.network:9000",
    ),
    (
        include_bytes!("../deploy/keys/private_4.pk8"),
        // "127.0.0.1:9103",
        // "3.111.188.178:9000",
        "1.us-east-2.cluster.aws.espresso.network:9000",
    ),
    (
        include_bytes!("../deploy/keys/private_5.pk8"),
        // "127.0.0.1:9104",
        // "52.66.253.105:9000",
        "2.us-east-2.cluster.aws.espresso.network:9000",
    ),
    (
        include_bytes!("../deploy/keys/private_6.pk8"),
        // "127.0.0.1:9105",
        // "34.219.31.18:9000",
        "0.us-west-2.cluster.aws.espresso.network:9000",
    ),
    (
        include_bytes!("../deploy/keys/private_7.pk8"),
        // "127.0.0.1:9106",
        // "54.184.243.4:9000",
        "1.us-west-2.cluster.aws.espresso.network:9000",
    ),
];

#[derive(Parser, Debug)]
#[clap(
    name = "Multi-machine consensus",
    about = "Simulates consensus among multiple machines"
)]
pub struct CliOpt {
    /// num nodes
    #[clap(long = "num_nodes", env)]
    pub num_nodes: usize,

    /// num bootstrap
    #[clap(long = "num_bootstrap", env)]
    pub num_bootstrap: usize,

    /// num transactions to be submitted per round
    #[clap(long = "num_txn_per_round", env)]
    pub num_txn_per_round: usize,

    /// Id of the current node
    #[clap(long = "node_idx", env)]
    pub node_idx: usize,

    /// how long to run for
    #[clap(long = "online_time", default_value = "60", env)]
    pub online_time: u64,

    /// address to bind to
    #[clap(long = "bound_addr", env)]
    #[clap(parse(try_from_str = parse_ip))]
    pub bound_addr: Multiaddr,

    /// seed used to generate ids
    #[clap(long = "seed", env)]
    pub seed: u64,

    /// bootstrap node mesh high
    #[clap(long = "bootstrap_mesh_n_high", env)]
    pub bootstrap_mesh_n_high: usize,

    /// bootstrap node mesh low
    #[clap(long = "bootstrap_mesh_n_low", env)]
    pub bootstrap_mesh_n_low: usize,

    /// bootstrap node outbound min
    #[clap(long = "bootstrap_mesh_outbound_min", env)]
    pub bootstrap_mesh_outbound_min: usize,

    /// bootstrap node mesh n
    #[clap(long = "bootstrap_mesh_n", env)]
    pub bootstrap_mesh_n: usize,

    /// bootstrap node mesh high
    #[clap(long = "mesh_n_high", env)]
    pub mesh_n_high: usize,

    /// bootstrap node mesh low
    #[clap(long = "mesh_n_low", env)]
    pub mesh_n_low: usize,

    /// bootstrap node outbound min
    #[clap(long = "mesh_outbound_min", env)]
    pub mesh_outbound_min: usize,

    /// bootstrap node mesh n
    #[clap(long = "mesh_n", env)]
    pub mesh_n: usize,

    /// max round time
    #[clap(long = "propose_max_round_time", env)]
    pub propose_max_round_time: u64,

    /// min round time
    #[clap(long = "propose_min_round_time", env)]
    pub propose_min_round_time: u64,

    /// next view timeout
    #[clap(long = "next_view_timeout", env)]
    pub next_view_timeout: u64,
}

type Node = DEntryNode<
    Libp2pNetwork<Message<DEntryBlock, Transaction, State, Ed25519Pub, H_256>, Ed25519Pub>,
>;

/// Creates the initial state and hotshot for simulation.
async fn init_state_and_hotshot(
    nodes: usize,
    threshold: usize,
    node_id: u64,
    config: &CliOpt,
    networking: Libp2pNetwork<
        Message<DEntryBlock, Transaction, State, Ed25519Pub, H_256>,
        Ed25519Pub,
    >,
) -> (State, HotShotHandle<Node, H_256>) {
    // Create the initial state
    // NOTE: all balances must be positive
    // so we avoid a negative balance
    let balances: BTreeMap<Account, Balance> = vec![
        ("Joe", 1_000_000),
        ("Nathan M", 500_000),
        ("John", 400_000),
        ("Nathan Y", 600_000),
        ("Ian", 5_000_000),
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
        execution_type: hotshot::ExecutionType::Continuous,
        total_nodes: NonZeroUsize::new(nodes).unwrap(),
        threshold: NonZeroUsize::new(threshold).unwrap(),
        max_transactions: NonZeroUsize::new(100).unwrap(),
        known_nodes: known_nodes.clone(),
        next_view_timeout: config.next_view_timeout * 1000,
        timeout_ratio: (11, 10),
        round_start_delay: 1,
        start_delay: 1,
        propose_min_round_time: Duration::from_secs(config.propose_min_round_time),
        propose_max_round_time: Duration::from_secs(config.propose_max_round_time),
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
    identity: Option<Keypair>,
    opts: &CliOpt,
) -> Result<
    Libp2pNetwork<Message<DEntryBlock, Transaction, State, Ed25519Pub, H_256>, Ed25519Pub>,
    NetworkError,
> {
    assert!(node_id < opts.num_nodes);
    let mut config_builder = NetworkNodeConfigBuilder::default();
    // NOTE we may need to change this as we scale
    config_builder.replication_factor(NonZeroUsize::new(opts.num_nodes - 2).unwrap());
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
                mesh_n_high: opts.bootstrap_mesh_n_high,
                mesh_n_low: opts.bootstrap_mesh_n_low,
                mesh_outbound_min: opts.bootstrap_mesh_outbound_min,
                mesh_n: opts.bootstrap_mesh_n,
            },
            NetworkNodeType::Regular => MeshParams {
                mesh_n_high: opts.mesh_n_high,
                mesh_n_low: opts.mesh_n_low,
                mesh_outbound_min: opts.mesh_outbound_min,
                mesh_n: opts.mesh_n,
            },
            NetworkNodeType::Conductor => unreachable!(),
        };

    config_builder.mesh_params(Some(mesh_params));

    let config = config_builder.build().unwrap();
    let bs_len = bs.len();

    Libp2pNetwork::new(
        config,
        pubkey,
        Arc::new(RwLock::new(bs)),
        bs_len,
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
            let key = Keypair::rsa_from_pkcs8(&mut key_bytes).unwrap();
            let multiaddr = parse_dns(addr_str).unwrap();
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
    let bound_addr = args.bound_addr.clone();
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
        own_identity,
        &args,
    )
    .await
    .unwrap();

    error!("Done with network creation");

    // Initialize the state and hotshot
    let (_own_state, mut hotshot) =
        init_state_and_hotshot(num_nodes, threshold, own_id as u64, &args, own_network).await;

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

    let mut total_txns = 0;

    while start_time + online_time > Instant::now() {
        info!("Beginning view {}", view);
        let _num_submitted = {
            if own_id == (view % num_nodes) {
                info!("Generating txn for view {}", view);
                let state = hotshot.get_state().await;

                for _ in 0..10 {
                    let txn = <State as TestableState<H_256>>::create_random_transaction(&state);
                    info!("Submitting txn on view {}", view);
                    hotshot.submit_transaction(txn).await.unwrap();
                }
                10
            } else {
                0
            }
        };
        info!("Running the view {}", view);
        hotshot.start().await;
        info!("Collection for view {}", view);
        let result = hotshot.collect_round_events().await;
        match result {
            Ok((state, blocks)) => {
                let mut num_tnxs = 0;
                for block in blocks {
                    num_tnxs += block.transactions.len();
                }
                total_txns += num_tnxs;
                error!(
                    "View {:?}: successful with {:?}, and total successful txns {:?}",
                    view, state, total_txns
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
        "All rounds completed, {} views with {} failures. This node {:?} has {:?} total txns successfully committed overall. Ran for {:?} from {:?} to {:?}",
        view,
        num_failed_views,
        own_id,
        total_txns,
        online_time,
        start_time,
        Instant::now()
    );
}
