use std::{
    cmp,
    collections::VecDeque,
    mem,
    net::{IpAddr, SocketAddr},
    num::NonZeroUsize,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

use async_compatibility_layer::{
    art::{async_sleep, TcpStream},
    logging::{setup_backtrace, setup_logging},
};
use async_lock::RwLock;
use async_trait::async_trait;
use clap::Parser;
use hotshot::{
    demos::vdemo::VDemoTypes,
    traits::{
        implementations::{
            CentralizedCommChannel, CentralizedServerNetwork, Libp2pCommChannel, Libp2pNetwork,
            MemoryStorage,
        },
        NetworkError, NodeImplementation, Storage,
    },
    types::{HotShotHandle, SignatureKey},
    HotShot, ViewRunner,
};
use hotshot_centralized_server::{
    FromServer, NetworkConfig, Run, RunResults, TcpStreamUtil, TcpStreamUtilWithRecv,
    TcpStreamUtilWithSend, ToServer,
};
use hotshot_types::{
    data::{TestableLeaf, ValidatingLeaf, ValidatingProposal},
    message::QuorumVote,
    traits::{
        election::Membership,
        metrics::NoMetrics,
        network::CommunicationChannel,
        node_implementation::NodeType,
        state::{TestableBlock, TestableState},
    },
    HotShotConfig,
};
use libp2p::{
    identity::{
        ed25519::{Keypair as EdKeypair, SecretKey},
        Keypair,
    },
    multiaddr::{self, Protocol},
    Multiaddr, PeerId,
};
use libp2p_networking::network::{MeshParams, NetworkNodeConfigBuilder, NetworkNodeType};
#[allow(deprecated)]
use nll::nll_todo::nll_todo;
use tracing::{debug, error};

/// yeesh maybe we should just implement SignatureKey for this...
pub fn libp2p_generate_indexed_identity(seed: [u8; 32], index: u64) -> Keypair {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&seed);
    hasher.update(&index.to_le_bytes());
    let new_seed = *hasher.finalize().as_bytes();
    let sk_bytes = SecretKey::from_bytes(new_seed).unwrap();
    let ed_kp = <EdKeypair as From<SecretKey>>::from(sk_bytes);
    Keypair::Ed25519(ed_kp)
}

/// libp2p helper function
/// convert node string into multi addr
/// node string of the form: "$IP:$PORT"
pub fn parse_dns(s: &str) -> Result<Multiaddr, multiaddr::Error> {
    let mut i = s.split(':');
    let ip = i.next().ok_or(multiaddr::Error::InvalidMultiaddr)?;
    let port = i.next().ok_or(multiaddr::Error::InvalidMultiaddr)?;
    Multiaddr::from_str(&format!("/dns/{ip}/tcp/{port}"))
}

/// libp2p helper function
pub fn parse_ip(s: &str) -> Result<Multiaddr, multiaddr::Error> {
    let mut i = s.split(':');
    let ip = i.next().ok_or(multiaddr::Error::InvalidMultiaddr)?;
    let port = i.next().ok_or(multiaddr::Error::InvalidMultiaddr)?;
    Multiaddr::from_str(&format!("/ip4/{ip}/tcp/{port}"))
}

pub const LIBP2P_BOOTSTRAPS_LOCAL_IPS: &[&str] = &[
    "127.0.0.1:9100",
    "127.0.0.1:9101",
    "127.0.0.1:9102",
    "127.0.0.1:9103",
    "127.0.0.1:9104",
    "127.0.0.1:9105",
    "127.0.0.1:9106",
];

pub const LIBP2P_BOOTSTRAPS_REMOTE_IPS: &[&str] = &[
    "0.ap-south-1.cluster.aws.espresso.network:9000",
    "1.ap-south-1.cluster.aws.espresso.network:9000",
    "0.us-east-2.cluster.aws.espresso.network:9000",
    "1.us-east-2.cluster.aws.espresso.network:9000",
    "2.us-east-2.cluster.aws.espresso.network:9000",
    "0.us-west-2.cluster.aws.espresso.network:9000",
    "1.us-west-2.cluster.aws.espresso.network:9000",
];

#[derive(Parser, Debug, Clone)]
#[command(
    name = "Multi-machine consensus",
    about = "Simulates consensus among multiple machines"
)]
pub struct CliOrchestrated {
    /// The address to connect to
    host: IpAddr,

    /// The port to connect to
    port: u16,
}

#[async_trait]
impl<
        TYPES: NodeType,
        MEMBERSHIP: Membership<TYPES>,
        NODE: NodeImplementation<
            TYPES,
            Leaf = ValidatingLeaf<TYPES>,
            Proposal = ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            Election = MEMBERSHIP,
            Networking = Libp2pCommChannel<
                TYPES,
                ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
                MEMBERSHIP,
            >,
            Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
        >,
    >
    CliConfig<
        TYPES,
        MEMBERSHIP,
        Libp2pCommChannel<
            TYPES,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
            MEMBERSHIP,
        >,
        NODE,
    > for Libp2pClientConfig<TYPES, MEMBERSHIP>
where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    ValidatingLeaf<TYPES>: TestableLeaf,
    HotShot<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
    Self: Sync,
{
    async fn init(args: CliOrchestrated) -> Result<Box<Self>, NetworkError> {
        let stream = TcpStream::connect(format!("{}:{}", args.host, args.port))
            .await
            .expect("Could not reach server");
        let mut stream = TcpStreamUtil::new(stream);
        stream
            .send(ToServer::<<VDemoTypes as NodeType>::SignatureKey>::GetConfig)
            .await
            .unwrap();
        error!("Waiting for server config...");
        let (mut config, run) = match stream.recv().await.expect("Could not get Libp2pConfig") {
            FromServer::<
                <TYPES as NodeType>::SignatureKey,
                <TYPES as NodeType>::ElectionConfigType,
            >::Config {
                config,
                run,
            } => (config, run),
            x => panic!("Expected Libp2pConfig, got {x:?}"),
        };
        error!("Received server config: {config:?}");
        let (pubkey, _privkey) =
            <<TYPES as NodeType>::SignatureKey as SignatureKey>::generated_from_seed_indexed(
                config.seed,
                config.node_index,
            );

        stream
            .send(ToServer::Identify {
                key: pubkey.clone(),
            })
            .await
            .expect("Could not identify with server");

        let libp2p_config = config
            .libp2p_config
            .take()
            .expect("Server is not configured as a libp2p server");
        let bs_len = libp2p_config.bootstrap_nodes.len();
        let bootstrap_nodes: Vec<(PeerId, Multiaddr)> = libp2p_config
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
        let identity = libp2p_generate_indexed_identity(config.seed, config.node_index);
        let node_type = if (config.node_index as usize) < bs_len {
            NetworkNodeType::Bootstrap
        } else {
            NetworkNodeType::Regular
        };
        let node_index = config.node_index;
        let bound_addr = format!(
            "/{}/{}/tcp/{}",
            if libp2p_config.public_ip.is_ipv4() {
                "ip4"
            } else {
                "ip6"
            },
            libp2p_config.public_ip,
            libp2p_config.base_port + node_index as u16
        )
        .parse()
        .unwrap();
        // generate network
        let mut config_builder = NetworkNodeConfigBuilder::default();
        assert!(config.config.total_nodes.get() > 2);
        let replicated_nodes = NonZeroUsize::new(config.config.total_nodes.get() - 2).unwrap();
        config_builder.replication_factor(replicated_nodes);
        config_builder.identity(identity.clone());
        let mesh_params =
            // NOTE I'm arbitrarily choosing these.
            match node_type {
                NetworkNodeType::Bootstrap => MeshParams {
                    mesh_n_high: libp2p_config.bootstrap_mesh_n_high,
                    mesh_n_low: libp2p_config.bootstrap_mesh_n_low,
                    mesh_outbound_min: libp2p_config.bootstrap_mesh_outbound_min,
                    mesh_n: libp2p_config.bootstrap_mesh_n,
                },
                NetworkNodeType::Regular => MeshParams {
                    mesh_n_high: libp2p_config.mesh_n_high,
                    mesh_n_low: libp2p_config.mesh_n_low,
                    mesh_outbound_min: libp2p_config.mesh_outbound_min,
                    mesh_n: libp2p_config.mesh_n,
                },
                NetworkNodeType::Conductor => unreachable!(),
            };
        config_builder.mesh_params(Some(mesh_params));

        let node_config = config_builder.build().unwrap();
        let network = Libp2pNetwork::new(
            NoMetrics::new(),
            node_config,
            pubkey.clone(),
            Arc::new(RwLock::new(
                bootstrap_nodes
                    .iter()
                    .map(|(peer_id, addr)| (Some(*peer_id), addr.clone()))
                    .collect(),
            )),
            bs_len,
            config.node_index as usize,
            // FIXME unsure what to do here.
            #[allow(deprecated)]
            nll_todo(),
        )
        .await
        .map(
            Libp2pCommChannel::<
                TYPES,
                ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
                MEMBERSHIP,
            >::new,
        )
        .unwrap();

        config.libp2p_config = Some(libp2p_config);
        // TODO do we want base ports to be the same?? This breaks it for local testing.
        // Maybe that's ok?
        Ok(Box::new(Libp2pClientConfig {
            config: *config,
            //FIXME do we need this
            _run: run,
            _bootstrap_nodes: bootstrap_nodes,
            _node_type: node_type,
            _identity: identity,
            _bound_addr: bound_addr,
            _socket: stream,
            network,
        }))
    }

    fn get_config(
        &self,
    ) -> NetworkConfig<<TYPES as NodeType>::SignatureKey, <TYPES as NodeType>::ElectionConfigType>
    {
        self.config.clone()
    }

    fn get_network(
        &self,
    ) -> Libp2pCommChannel<
        TYPES,
        ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
        QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
        MEMBERSHIP,
    > {
        self.network.clone()
    }
}

#[async_trait]
impl<
        TYPES: NodeType,
        MEMBERSHIP: Membership<TYPES>,
        NODE: NodeImplementation<
            TYPES,
            Leaf = ValidatingLeaf<TYPES>,
            Proposal = ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            Vote = QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
            Election = MEMBERSHIP,
            Networking = CentralizedCommChannel<
                TYPES,
                ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
                MEMBERSHIP,
            >,
            Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
        >,
    >
    CliConfig<
        TYPES,
        MEMBERSHIP,
        CentralizedCommChannel<
            TYPES,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
            MEMBERSHIP,
        >,
        NODE,
    > for CentralizedConfig<TYPES, MEMBERSHIP>
where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    ValidatingLeaf<TYPES>: TestableLeaf,
    HotShot<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
    Self: Sync,
{
    async fn init(args: CliOrchestrated) -> Result<Box<Self>, NetworkError> {
        let addr: SocketAddr = (args.host, args.port).into();
        error!("Connecting to {addr:?} to retrieve the server config");
        let (config, run, network) =
            CentralizedServerNetwork::connect_with_server_config(NoMetrics::new(), addr).await;
        let network = CentralizedCommChannel::new(network);

        error!("Run: {:?}", run);
        error!("Config: {:?}", config);

        // Get networking information

        let node_count = config.config.total_nodes;

        debug!("Waiting on connections...");
        while !network.is_ready().await {
            let connected_clients = network.get_connected_client_count().await;
            error!("{} / {}", connected_clients, node_count);
            async_sleep(Duration::from_secs(1)).await;
        }

        Ok(Box::new(CentralizedConfig {
            config,
            _run: run,
            network,
        }))
    }

    fn get_config(
        &self,
    ) -> NetworkConfig<<TYPES as NodeType>::SignatureKey, <TYPES as NodeType>::ElectionConfigType>
    {
        self.config.clone()
    }

    fn get_network(
        &self,
    ) -> CentralizedCommChannel<
        TYPES,
        ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
        QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
        MEMBERSHIP,
    > {
        self.network.clone()
    }
}

type Proposal<T> = ValidatingProposal<T, ValidatingLeaf<T>>;

pub struct Libp2pClientConfig<TYPES: NodeType, MEMBERSHIP: Membership<TYPES>> {
    _bootstrap_nodes: Vec<(PeerId, Multiaddr)>,
    _node_type: NetworkNodeType,
    _bound_addr: Multiaddr,
    /// for libp2p layer
    _identity: Keypair,

    _socket: TcpStreamUtil,
    network: Libp2pCommChannel<
        TYPES,
        Proposal<TYPES>,
        QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
        MEMBERSHIP,
    >,
    //TODO do we need this? I don't think so
    _run: Run,
    config:
        NetworkConfig<<TYPES as NodeType>::SignatureKey, <TYPES as NodeType>::ElectionConfigType>,
}

pub enum Config<TYPES: NodeType, MEMBERSHIP: Membership<TYPES>> {
    Libp2pConfig(Libp2pClientConfig<TYPES, MEMBERSHIP>),
    CentralizedConfig(CentralizedConfig<TYPES, MEMBERSHIP>),
}

pub struct CentralizedConfig<TYPES: NodeType, MEMBERSHIP: Membership<TYPES>> {
    config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    network: CentralizedCommChannel<
        TYPES,
        Proposal<TYPES>,
        QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
        MEMBERSHIP,
    >,
    _run: Run,
}

#[async_trait]
pub trait CliConfig<
    TYPES: NodeType,
    MEMBERSHIP: Membership<TYPES>,
    NETWORK: CommunicationChannel<
        TYPES,
        ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
        QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
        MEMBERSHIP,
    >,
    NODE: NodeImplementation<
        TYPES,
        Leaf = ValidatingLeaf<TYPES>,
        Proposal = ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
        Election = MEMBERSHIP,
        Networking = NETWORK,
        Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
    >,
> where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    ValidatingLeaf<TYPES>: TestableLeaf,
    HotShot<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
    Self: Sync,
{
    async fn init(args: CliOrchestrated) -> Result<Box<Self>, NetworkError>;

    async fn wait_for_ready(&self) {
        CommunicationChannel::wait_for_ready(&self.get_network()).await;
    }

    // TODO check that the orchestrator does this properly.
    // TODO no more config.config.clone()
    async fn init_state_and_hotshot(&self) -> (TYPES::StateType, HotShotHandle<TYPES, NODE>) {
        let genesis_block = TYPES::BlockType::genesis();
        let initializer =
            hotshot::HotShotInitializer::<TYPES, ValidatingLeaf<TYPES>>::from_genesis(
                genesis_block,
            )
            .unwrap();

        let config = self.get_config();

        let (pk, sk) =
            TYPES::SignatureKey::generated_from_seed_indexed(config.seed, config.node_index);
        let known_nodes = config.config.known_nodes.clone();

        let network = self.get_network();
        let election_config = config.config.election_config.clone().unwrap();

        let hotshot = HotShot::init(
            pk,
            sk,
            config.node_index,
            config.config,
            network,
            MemoryStorage::new(),
            MEMBERSHIP::create_election(known_nodes, election_config),
            initializer,
            NoMetrics::new(),
        )
        .await
        .expect("Could not init hotshot");

        let state = hotshot.storage().get_anchored_view().await.unwrap().state;
        (state, hotshot)
    }

    async fn run_consensus(&self, mut hotshot: HotShotHandle<TYPES, NODE>) -> RunResults {
        let NetworkConfig {
            padding,
            rounds,
            transactions_per_round,
            node_index,
            config: HotShotConfig { total_nodes, .. },
            ..
        } = self.get_config();

        let size = mem::size_of::<TYPES::Transaction>();
        let adjusted_padding = if padding < size { 0 } else { padding - size };
        let mut txns: VecDeque<TYPES::Transaction> = VecDeque::new();
        let state = hotshot.get_state().await;

        // This assumes that no node will be a leader more than 5x the expected number of times they should be the leader
        // FIXME  is this a reasonable assumption when we start doing DA?
        let tx_to_gen = transactions_per_round * (cmp::max(rounds / total_nodes, 1) + 5);
        error!("Generated {} transactions", tx_to_gen);
        {
            let mut txn_rng = rand::thread_rng();
            for _ in 0..tx_to_gen {
                // TODO make this u64...
                let txn =
                    <<TYPES as NodeType>::StateType as TestableState>::create_random_transaction(
                        &state,
                        &mut txn_rng,
                        padding as u64,
                    );
                txns.push_back(txn);
            }
        }

        error!("Adjusted padding size is = {:?}", adjusted_padding);
        let mut timed_out_views: u64 = 0;
        let mut round = 1;
        let mut total_transactions = 0;

        let start = Instant::now();

        error!("Starting hotshot!");
        hotshot.start().await;
        while round <= rounds {
            debug!(?round);
            error!("Round {}:", round);

            let num_submitted = if node_index == ((round % total_nodes) as u64) {
                tracing::info!("Generating txn for round {}", round);

                for _ in 0..transactions_per_round {
                    let txn = txns.pop_front().unwrap();
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
                        debug!("  - State: {state:?}");
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

        let total_time_elapsed = start.elapsed();
        let expected_transactions = transactions_per_round * rounds;
        let total_size = total_transactions * (padding as u64);
        error!("All {rounds} rounds completed in {total_time_elapsed:?}");
        error!("{timed_out_views} rounds timed out");

        // This assumes all submitted transactions make it through consensus:
        error!(
            "{} total bytes submitted in {:?}",
            total_size, total_time_elapsed
        );
        debug!("All rounds completed");

        RunResults {
            // FIXME nuke this field since we're not doing this anymore.
            run: Run(0),
            node_index,

            transactions_submitted: total_transactions as usize,
            transactions_rejected: expected_transactions - (total_transactions as usize),
            transaction_size_bytes: (total_size as usize),

            rounds_succeeded: rounds as u64 - timed_out_views,
            rounds_timed_out: timed_out_views,
            total_time_in_seconds: total_time_elapsed.as_secs_f64(),
        }
    }

    fn get_config(
        &self,
    ) -> NetworkConfig<<TYPES as NodeType>::SignatureKey, <TYPES as NodeType>::ElectionConfigType>;

    fn get_network(&self) -> NETWORK;
}

pub async fn main_entry_point<
    TYPES: NodeType,
    MEMBERSHIP: Membership<TYPES>,
    NETWORK: CommunicationChannel<
        TYPES,
        ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
        QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
        MEMBERSHIP,
    >,
    NODE: NodeImplementation<
        TYPES,
        Leaf = ValidatingLeaf<TYPES>,
        Proposal = ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
        Election = MEMBERSHIP,
        Networking = NETWORK,
        Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
    >,
    CONFIG: CliConfig<TYPES, MEMBERSHIP, NETWORK, NODE>,
>(
    args: CliOrchestrated,
) where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    ValidatingLeaf<TYPES>: TestableLeaf,
    HotShot<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
    CONFIG: Sync,
{
    setup_logging();
    setup_backtrace();

    let config = CONFIG::init(args).await.unwrap();

    config.wait_for_ready().await;

    let (_state, hotshot_handle) = config.init_state_and_hotshot().await;

    config.run_consensus(hotshot_handle).await;
}
