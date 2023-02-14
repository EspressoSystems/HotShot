use async_compatibility_layer::logging::{setup_backtrace, setup_logging};
// TODO this should really be moved into the utils crate.
use async_lock::RwLock;
use clap::Parser;
use hotshot::{
    demos::dentry::*,
    traits::{
        election::static_committee::{GeneralStaticCommittee, StaticElectionConfig},
        implementations::{Libp2pCommChannel, Libp2pNetwork, MemoryStorage},
        NetworkError, Storage,
    },
    types::{ed25519::Ed25519Priv, HotShotHandle},
    HotShot,
};
use hotshot_centralized_server::{
    Run, RunResults, TcpStreamUtil, TcpStreamUtilWithRecv, TcpStreamUtilWithSend,
};
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal},
    message::QuorumVote,
    traits::{
        metrics::NoMetrics,
        network::CommunicationChannel,
        node_implementation::NodeType,
        signature_key::{ed25519::Ed25519Pub, SignatureKey, TestableSignatureKey},
        state::TestableState,
    },
    ExecutionType, HotShotConfig,
};
use libp2p::{
    identity::Keypair,
    multiaddr::{self, Protocol},
    Multiaddr, PeerId,
};
use libp2p_networking::network::{MeshParams, NetworkNodeConfigBuilder, NetworkNodeType};
#[allow(deprecated)]
use nll::nll_todo::nll_todo;
use std::{
    collections::HashSet,
    num::NonZeroUsize,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::{debug, error, info, instrument};

#[cfg(feature = "async-std-executor")]
use async_std::net::TcpStream;
#[cfg(feature = "tokio-executor")]
use tokio::net::TcpStream;
#[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
std::compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}

type FromServer = hotshot_centralized_server::FromServer<Ed25519Pub, StaticElectionConfig>;
type ToServer = hotshot_centralized_server::ToServer<Ed25519Pub>;

/// convert node string into multi addr
/// node string of the form: "$IP:$PORT"
pub fn parse_dns(s: &str) -> Result<Multiaddr, multiaddr::Error> {
    let mut i = s.split(':');
    let ip = i.next().ok_or(multiaddr::Error::InvalidMultiaddr)?;
    let port = i.next().ok_or(multiaddr::Error::InvalidMultiaddr)?;
    Multiaddr::from_str(&format!("/dns/{ip}/tcp/{port}"))
}

pub fn parse_ip(s: &str) -> Result<Multiaddr, multiaddr::Error> {
    let mut i = s.split(':');
    let ip = i.next().ok_or(multiaddr::Error::InvalidMultiaddr)?;
    let port = i.next().ok_or(multiaddr::Error::InvalidMultiaddr)?;
    Multiaddr::from_str(&format!("/ip4/{ip}/tcp/{port}"))
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

#[derive(clap::Args, Debug)]
struct CliOrchestrated {
    /// The orchestrator host to connect to
    #[clap(env)]
    addr: String,
}

#[derive(clap::Args, Debug)]
struct CliStandalone {
    /// num nodes
    #[arg(long, env)]
    num_nodes: u64,

    /// num bootstrap
    #[arg(long, env)]
    num_bootstrap: u64,

    /// num transactions to be submitted per round
    #[arg(long, env)]
    num_txn_per_round: u64,

    /// Id of the current node
    #[arg(long, env)]
    node_idx: u64,

    /// how long to run for
    #[arg(long, default_value = "60", env)]
    online_time: u64,

    /// address to bind to
    #[arg(long, env, value_parser = parse_ip)]
    bound_addr: Multiaddr,

    /// seed used to generate ids
    #[arg(long, env)]
    seed: u64,

    /// bootstrap node mesh high
    #[arg(long, env)]
    bootstrap_mesh_n_high: usize,

    /// bootstrap node mesh low
    #[arg(long, env)]
    bootstrap_mesh_n_low: usize,

    /// bootstrap node outbound min
    #[arg(long, env)]
    bootstrap_mesh_outbound_min: usize,

    /// bootstrap node mesh n
    #[arg(long, env)]
    bootstrap_mesh_n: usize,

    /// bootstrap node mesh high
    #[arg(long, env)]
    mesh_n_high: usize,

    /// bootstrap node mesh low
    #[arg(long, env)]
    mesh_n_low: usize,

    /// bootstrap node outbound min
    #[arg(long, env)]
    mesh_outbound_min: usize,

    /// bootstrap node mesh n
    #[arg(long, env)]
    mesh_n: usize,

    /// max round time
    #[arg(long, env)]
    propose_max_round_time: u64,

    /// min round time
    #[arg(long, env)]
    propose_min_round_time: u64,

    /// next view timeout
    #[arg(long, env)]
    next_view_timeout: u64,
}

#[derive(Parser, Debug)]
#[command(
    name = "Multi-machine consensus",
    about = "Simulates consensus among multiple machines"
)]
enum CliOpt {
    Orchestrated(CliOrchestrated),
    Standalone(CliStandalone),
}

impl CliOrchestrated {
    async fn init(&self, server_conn: &mut Option<TcpStreamUtil>) -> Result<Config, NetworkError> {
        let stream = TcpStream::connect(&self.addr)
            .await
            .expect("Could not reach server");
        let mut stream = TcpStreamUtil::new(stream);
        stream.send(ToServer::GetConfig).await.unwrap();
        error!("Waiting for server config...");
        let (mut config, run) = match stream.recv().await.expect("Could not get Libp2pConfig") {
            FromServer::Config { config, run } => (config, run),
            x => panic!("Expected Libp2pConfig, got {x:?}"),
        };
        assert_eq!(config.key_type_name, std::any::type_name::<Ed25519Pub>());
        assert_eq!(
            config.election_config_type_name,
            std::any::type_name::<StaticElectionConfig>()
        );
        error!("Received server config: {config:?}");
        let privkey = Ed25519Priv::generated_from_seed_indexed(config.seed, config.node_index);
        let pubkey = Ed25519Pub::from_private(&privkey);

        stream
            .send(ToServer::Identify { key: pubkey })
            .await
            .expect("Could not identify with server");

        let libp2p_config = config
            .libp2p_config
            .take()
            .expect("Server is not configured as a libp2p server");
        let bs = libp2p_config
            .bootstrap_nodes
            .iter()
            .map(|(addr, pair)| {
                let kp = Keypair::from_protobuf_encoding(pair).unwrap();
                let peer_id = PeerId::from_public_key(&kp.public());
                let mut multiaddr = Multiaddr::from(addr.ip());
                multiaddr.push(Protocol::Tcp(addr.port()));
                (peer_id, multiaddr)
            })
            .collect();

        let (node_type, identity) =
            if (config.node_index as usize) < libp2p_config.bootstrap_nodes.len() {
                (
                    NetworkNodeType::Bootstrap,
                    Some(
                        Keypair::from_protobuf_encoding(
                            &libp2p_config.bootstrap_nodes[config.node_index as usize].1,
                        )
                        .unwrap(),
                    ),
                )
            } else {
                (NetworkNodeType::Regular, None)
            };

        *server_conn = Some(stream);

        Ok(Config {
            run,
            privkey,
            pubkey,
            bs,
            node_id: config.node_index,
            node_type,
            identity,
            bound_addr: format!(
                "/{}/{}/tcp/{}",
                if libp2p_config.public_ip.is_ipv4() {
                    "ip4"
                } else {
                    "ip6"
                },
                libp2p_config.public_ip,
                libp2p_config.base_port + config.node_index as u16
            )
            .parse()
            .unwrap(),
            num_nodes: config.config.total_nodes.get() as _,
            bootstrap_mesh_n_high: libp2p_config.bootstrap_mesh_n_high,
            bootstrap_mesh_n_low: libp2p_config.bootstrap_mesh_n_low,
            bootstrap_mesh_outbound_min: libp2p_config.bootstrap_mesh_outbound_min,
            bootstrap_mesh_n: libp2p_config.bootstrap_mesh_n,
            mesh_n_high: libp2p_config.mesh_n_high,
            mesh_n_low: libp2p_config.mesh_n_low,
            mesh_outbound_min: libp2p_config.mesh_outbound_min,
            mesh_n: libp2p_config.mesh_n,
            next_view_timeout: libp2p_config.next_view_timeout,
            propose_min_round_time: libp2p_config.propose_min_round_time,
            propose_max_round_time: libp2p_config.propose_max_round_time,
            online_time: libp2p_config.online_time,
            num_txn_per_round: libp2p_config.num_txn_per_round,
        })
    }
}
impl CliStandalone {
    fn init(&self) -> Config {
        let mut seed = [0u8; 32];
        seed[0..16].copy_from_slice(&self.seed.to_le_bytes());
        let privkey = Ed25519Priv::generated_from_seed_indexed(seed, self.node_idx);
        let pubkey = Ed25519Pub::from_private(&privkey);

        let bootstrap_priv: Vec<_> = BOOTSTRAPS
            .iter()
            .map(|(key_bytes, addr_str)| {
                let mut key_bytes = <&[u8]>::clone(key_bytes).to_vec();
                let key = Keypair::rsa_from_pkcs8(&mut key_bytes).unwrap();
                let multiaddr = parse_dns(addr_str).unwrap();
                (key, multiaddr)
            })
            .take(self.num_bootstrap as usize)
            .collect();

        let to_connect_addrs: Vec<_> = bootstrap_priv
            .clone()
            .into_iter()
            .map(|(kp, ma)| (PeerId::from_public_key(&kp.public()), ma))
            .collect();
        let (node_type, own_identity) = if self.node_idx < self.num_bootstrap {
            (
                NetworkNodeType::Bootstrap,
                Some(bootstrap_priv[self.node_idx as usize].0.clone()),
            )
        } else {
            (NetworkNodeType::Regular, None)
        };

        Config {
            run: Run(0),
            privkey,
            pubkey,
            bs: to_connect_addrs,
            node_id: self.node_idx,
            node_type,
            bound_addr: self.bound_addr.clone(),
            identity: own_identity,
            num_nodes: self.num_nodes,
            bootstrap_mesh_n_high: self.bootstrap_mesh_n_high,
            bootstrap_mesh_n_low: self.bootstrap_mesh_n_low,
            bootstrap_mesh_outbound_min: self.bootstrap_mesh_outbound_min,
            bootstrap_mesh_n: self.bootstrap_mesh_n,
            mesh_n_high: self.mesh_n_high,
            mesh_n_low: self.mesh_n_low,
            mesh_outbound_min: self.mesh_outbound_min,
            mesh_n: self.mesh_n,
            next_view_timeout: self.next_view_timeout,
            propose_min_round_time: self.propose_min_round_time,
            propose_max_round_time: self.propose_max_round_time,
            online_time: self.online_time,
            num_txn_per_round: self.num_txn_per_round,
        }
    }
}

type ThisLeaf = ValidatingLeaf<DEntryTypes>;
type ThisElection =
    GeneralStaticCommittee<DEntryTypes, ThisLeaf, <DEntryTypes as NodeType>::SignatureKey>;
type ThisNetworking = Libp2pCommChannel<DEntryTypes, ThisProposal, ThisVote>;
type ThisProposal = ValidatingProposal<DEntryTypes, ThisElection>;
type ThisVote = QuorumVote<DEntryTypes, ThisLeaf>;
type Node = DEntryNode<ThisNetworking, ThisElection>;

struct Config {
    run: Run,
    privkey: Ed25519Priv,
    pubkey: Ed25519Pub,
    bs: Vec<(PeerId, Multiaddr)>,
    node_id: u64,
    node_type: NetworkNodeType,
    bound_addr: Multiaddr,
    identity: Option<Keypair>,
    num_nodes: u64,
    bootstrap_mesh_n_high: usize,
    bootstrap_mesh_n_low: usize,
    bootstrap_mesh_outbound_min: usize,
    bootstrap_mesh_n: usize,
    mesh_n_high: usize,
    mesh_n_low: usize,
    mesh_outbound_min: usize,
    mesh_n: usize,
    next_view_timeout: u64,
    propose_min_round_time: u64,
    propose_max_round_time: u64,
    online_time: u64,
    num_txn_per_round: u64,
}

impl Config {
    /// Creates the initial state and hotshot for simulation.
    async fn init_state_and_hotshot(
        &self,
        networking: ThisNetworking,
    ) -> (DEntryState, HotShotHandle<DEntryTypes, Node>) {
        let genesis_block = DEntryBlock::genesis();
        let initializer = hotshot::HotShotInitializer::from_genesis(genesis_block).unwrap();

        // Create the initial hotshot
        let known_nodes: Vec<_> = (0..self.num_nodes)
            .map(|x| {
                let priv_key = Ed25519Pub::generate_test_key(x);
                Ed25519Pub::from_private(&priv_key)
            })
            .collect();

        let config = HotShotConfig {
            execution_type: ExecutionType::Continuous,
            total_nodes: NonZeroUsize::new(self.num_nodes as usize).unwrap(),
            max_transactions: NonZeroUsize::new(100).unwrap(),
            min_transactions: 0,
            known_nodes: known_nodes.clone(),
            next_view_timeout: self.next_view_timeout * 1000,
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            propose_min_round_time: Duration::from_secs(self.propose_min_round_time),
            propose_max_round_time: Duration::from_secs(self.propose_max_round_time),
            num_bootstrap: 7,
            election_config: Some(StaticElectionConfig {}),
        };
        debug!(?config);
        let hotshot = HotShot::init(
            self.pubkey,
            self.privkey.clone(),
            self.node_id,
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

    async fn new_libp2p_network(&self) -> Result<ThisNetworking, NetworkError> {
        assert!(self.node_id < self.num_nodes);
        let mut config_builder = NetworkNodeConfigBuilder::default();
        // NOTE we may need to change this as we scale
        config_builder.replication_factor(NonZeroUsize::new(self.num_nodes as usize - 2).unwrap());
        config_builder.to_connect_addrs(HashSet::new());
        config_builder.node_type(self.node_type);
        error!("Binding to {:?}", self.bound_addr);
        config_builder.bound_addr(Some(self.bound_addr.clone()));

        if let Some(identity) = self.identity.clone() {
            config_builder.identity(identity);
        }

        let mesh_params =
        // NOTE I'm arbitrarily choosing these.
        match self.node_type {
            NetworkNodeType::Bootstrap => MeshParams {
                mesh_n_high: self.bootstrap_mesh_n_high,
                mesh_n_low: self.bootstrap_mesh_n_low,
                mesh_outbound_min: self.bootstrap_mesh_outbound_min,
                mesh_n: self.bootstrap_mesh_n,
            },
            NetworkNodeType::Regular => MeshParams {
                mesh_n_high: self.mesh_n_high,
                mesh_n_low: self.mesh_n_low,
                mesh_outbound_min: self.mesh_outbound_min,
                mesh_n: self.mesh_n,
            },
            NetworkNodeType::Conductor => unreachable!(),
        };

        config_builder.mesh_params(Some(mesh_params));

        let node_config = config_builder.build().unwrap();
        let bs_len = self.bs.len();

        Libp2pNetwork::new(
            NoMetrics::new(),
            node_config,
            self.pubkey,
            Arc::new(RwLock::new(
                self.bs
                    .iter()
                    .map(|(peer_id, addr)| (Some(*peer_id), addr.clone()))
                    .collect(),
            )),
            bs_len,
            self.node_id as usize,
            #[allow(deprecated)]
            nll_todo(),
        )
        .await
        .map(Libp2pCommChannel::new)
    }
}

#[cfg_attr(
    feature = "tokio-executor",
    tokio::main(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::main)]
#[instrument]
async fn main() {
    setup_logging();
    setup_backtrace();

    let mut rng = rand::thread_rng();

    let args = CliOpt::parse();
    let mut server_conn = None;
    let config = match args {
        CliOpt::Standalone(args) => args.init(),
        CliOpt::Orchestrated(args) => args
            .init(&mut server_conn)
            .await
            .expect("Could not create Config"),
    };
    let own_id = config.node_id;
    let num_nodes = config.num_nodes;
    error!("Done with keygen");
    let own_network = config.new_libp2p_network().await.unwrap();

    error!("Done with network creation");

    // Initialize the state and hotshot
    let (_own_state, mut hotshot) = config.init_state_and_hotshot(own_network).await;

    error!("waiting for connections to hotshot!");
    let network = hotshot.networking();
    <ThisNetworking as CommunicationChannel<
        DEntryTypes,
        ThisLeaf,
        ThisProposal,
        ThisVote,
        ThisElection,
    >>::ready(network)
    .await;

    if let Some(server) = &mut server_conn {
        error!("Waiting for server to start us up");
        loop {
            match server
                .recv::<FromServer>()
                .await
                .expect("Lost connection to server")
            {
                FromServer::Start => {
                    error!("Starting!");
                    break;
                }
                x => error!("Unexpected server message: {x:?}"),
            }
        }
    } else {
        error!("We are ready!");
    }
    error!("Finished init, starting hotshot!");
    hotshot.start().await;

    let start_time = Instant::now();

    let mut view = 0;

    // Run random transactions until failure
    let mut num_failed_views = 0;
    let mut num_succeeded_views = 0;

    let online_time = Duration::from_secs(60 * config.online_time);

    let mut total_txns = 0;

    while start_time + online_time > Instant::now() {
        error!("Beginning view {}", view);
        if own_id == (view % num_nodes) {
            info!("Generating txn for view {}", view);
            let state = hotshot.get_state().await;

            for _ in 0..config.num_txn_per_round {
                let txn =
                    <DEntryState as TestableState>::create_random_transaction(&state, &mut rng);
                info!("Submitting txn on view {}", view);
                hotshot.submit_transaction(txn).await.unwrap();
            }
        }
        info!("Running the view {}", view);
        hotshot.start().await;
        info!("Collection for view {}", view);
        let result = hotshot.collect_round_events().await;
        match result {
            Ok((_state, blocks)) => {
                let mut num_tnxs = 0;
                for block in blocks {
                    num_tnxs += block.txn_count();
                }
                total_txns += num_tnxs;
                num_succeeded_views += 1;
                error!(
                    "View {:?}: successful with total successful txns {:?}",
                    view, total_txns
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

    if let Some(mut server) = server_conn {
        server
            .send(ToServer::Results(RunResults {
                node_index: own_id,
                rounds_succeeded: num_succeeded_views,
                rounds_timed_out: num_failed_views,
                run: config.run,
                total_time_in_seconds: online_time.as_secs_f64(),
                transaction_size_bytes: 0,
                transactions_rejected: 0,
                transactions_submitted: total_txns,
            }))
            .await
            .expect("Could not report results to the server");
    }
}
