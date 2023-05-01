use futures::Future;
use futures::FutureExt;
use std::net::Ipv4Addr;
use std::{
    cmp,
    collections::{BTreeSet, VecDeque},
    fs, mem,
    net::IpAddr,
    num::NonZeroUsize,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use surf_disco::Client;

use surf_disco::error::ClientError;

use async_compatibility_layer::{
    art::async_sleep,
    logging::{setup_backtrace, setup_logging},
};
use async_lock::RwLock;
use async_trait::async_trait;
use clap::Parser;
use hotshot::{
    traits::{
        implementations::{
            Libp2pCommChannel, Libp2pNetwork, MemoryStorage, WebCommChannel, WebServerNetwork,
        },
        NodeImplementation, Storage,
    },
    types::{HotShotHandle, SignatureKey},
    HotShot, ViewRunner,
};
use hotshot_orchestrator::{
    self,
    config::{NetworkConfig, NetworkConfigFile, WebServerConfig},
};
use hotshot_types::traits::election::ConsensusExchange;
use hotshot_types::{
    data::{TestableLeaf, ValidatingLeaf, ValidatingProposal},
    traits::{
        election::Membership,
        metrics::NoMetrics,
        network::CommunicationChannel,
        node_implementation::NodeType,
        state::{TestableBlock, TestableState},
    },
    vote::QuorumVote,
    HotShotConfig,
};
use hotshot_types::{message::Message, traits::election::QuorumExchange};
use libp2p::{
    identity::{
        ed25519::{Keypair as EdKeypair, SecretKey},
        Keypair,
    },
    multiaddr::{self, Protocol},
    Multiaddr,
};
use libp2p_identity::PeerId;
use libp2p_networking::network::{MeshParams, NetworkNodeConfigBuilder, NetworkNodeType};
use rand::SeedableRng;
#[allow(deprecated)]
use tracing::error;

// ORCHESTRATOR

#[derive(Parser, Debug, Clone)]
#[command(
    name = "Multi-machine consensus",
    about = "Simulates consensus among multiple machines"
)]
/// Arguments passed to the orchestrator
pub struct OrchestratorArgs {
    /// The address the orchestrator runs on
    host: IpAddr,
    /// The port the orchestrator runs on
    port: u16,
    /// The configuration file to be used for this run
    config_file: String,
}

/// Reads a network configuration from a given filepath
pub fn load_config_from_file<TYPES: NodeType>(
    config_file: String,
) -> NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType> {
    let config_file_as_string: String = fs::read_to_string(config_file.as_str())
        .unwrap_or_else(|_| panic!("Could not read config file located at {config_file}"));
    let config_toml: NetworkConfigFile =
        toml::from_str::<NetworkConfigFile>(&config_file_as_string)
            .expect("Unable to convert config file to TOML");

    let mut config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType> =
        config_toml.into();

    // Generate network's public keys
    config.config.known_nodes = (0..config.config.total_nodes.get())
        .map(|node_id| {
            TYPES::SignatureKey::generated_from_seed_indexed(
                config.seed,
                node_id.try_into().unwrap(),
            )
            .0
        })
        .collect();

    config
}

/// Runs the orchestrator
pub async fn run_orchestrator<
    TYPES: NodeType,
    MEMBERSHIP: Membership<TYPES>,
    NETWORK: CommunicationChannel<
        TYPES,
        Message<TYPES, NODE>,
        ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
        QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
        MEMBERSHIP,
    >,
    NODE: NodeImplementation<
        TYPES,
        Leaf = ValidatingLeaf<TYPES>,
        QuorumExchange = QuorumExchange<
            TYPES,
            ValidatingLeaf<TYPES>,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            MEMBERSHIP,
            NETWORK,
            Message<TYPES, NODE>,
        >,
        Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
    >,
>(
    OrchestratorArgs {
        host,
        port,
        config_file,
    }: OrchestratorArgs,
) {
    error!("Starting orchestrator",);
    let run_config = load_config_from_file::<TYPES>(config_file);
    let _result = hotshot_orchestrator::run_orchestrator::<
        TYPES::SignatureKey,
        TYPES::ElectionConfigType,
    >(run_config, host, port)
    .await;
}

// VALIDATOR

#[derive(Parser, Debug, Clone)]
#[command(
    name = "Multi-machine consensus",
    about = "Simulates consensus among multiple machines"
)]
/// Arguments passed to the validator
pub struct ValidatorArgs {
    /// The address the orchestrator runs on
    host: IpAddr,
    /// The port the orchestrator runs on
    port: u16,
    /// This node's public IP address, for libp2p
    /// If no IP address is passed in, it will default to 127.0.0.1
    public_ip: Option<IpAddr>,
}

/// Defines the behavior of a "run" of the network with a given configuration
#[async_trait]
pub trait Run<
    TYPES: NodeType,
    MEMBERSHIP: Membership<TYPES>,
    NETWORK: CommunicationChannel<
        TYPES,
        Message<TYPES, NODE>,
        ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
        QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
        MEMBERSHIP,
    >,
    NODE: NodeImplementation<
        TYPES,
        Leaf = ValidatingLeaf<TYPES>,
        QuorumExchange = QuorumExchange<
            TYPES,
            ValidatingLeaf<TYPES>,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            MEMBERSHIP,
            NETWORK,
            Message<TYPES, NODE>,
        >,
        CommitteeExchange = QuorumExchange<
            TYPES,
            ValidatingLeaf<TYPES>,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            MEMBERSHIP,
            NETWORK,
            Message<TYPES, NODE>,
        >,
        Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
    >,
> where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    ValidatingLeaf<TYPES>: TestableLeaf,
    HotShot<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
    Self: Sync,
{
    /// Initializes networking, returns self
    async fn initialize_networking(
        config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    ) -> Self;

    /// Initializes the genesis state and HotShot instance; does not start HotShot consensus
    /// # Panics if it cannot generate a genesis block, fails to initialize HotShot, or cannot
    /// get the anchored view
    async fn initialize_state_and_hotshot(&self) -> (TYPES::StateType, HotShotHandle<TYPES, NODE>) {
        let genesis_block = TYPES::BlockType::genesis();
        let initializer =
            hotshot::HotShotInitializer::<TYPES, ValidatingLeaf<TYPES>>::from_genesis(
                genesis_block,
            )
            .expect("Couldn't generate genesis block");

        let config = self.get_config();

        let (pk, sk) =
            TYPES::SignatureKey::generated_from_seed_indexed(config.seed, config.node_index);
        let ek = jf_primitives::aead::KeyPair::generate(&mut rand_chacha::ChaChaRng::from_seed(
            config.seed
        ));
        let known_nodes = config.config.known_nodes.clone();

        let network = self.get_network();

        // Since we do not currently pass the election config type in the NetworkConfig, this will always be the default election config
        let election_config = config.config.election_config.clone().unwrap_or_else(|| {
            <QuorumExchange<
                TYPES,
                ValidatingLeaf<TYPES>,
                ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                MEMBERSHIP,
                NETWORK,
                Message<TYPES, NODE>,
            > as ConsensusExchange<TYPES, Message<TYPES, NODE>>>::Membership::default_election_config(
                config.config.total_nodes.get() as u64
            )
        });
        let quorum_exchange = NODE::QuorumExchange::create(
            known_nodes.clone(),
            election_config.clone(),
            network.clone(),
            pk.clone(),
            sk.clone(),
            ek.clone(),
        );
        let committee_exchange = NODE::CommitteeExchange::create(
            known_nodes,
            election_config,
            network,
            pk.clone(),
            sk.clone(),
            ek.clone(),
        );
        let hotshot = HotShot::init(
            pk,
            sk,
            config.node_index,
            config.config,
            MemoryStorage::empty(),
            quorum_exchange,
            committee_exchange,
            initializer,
            NoMetrics::boxed(),
        )
        .await
        .expect("Could not init hotshot");

        let state = hotshot
            .storage()
            .get_anchored_view()
            .await
            .expect("Couldn't get HotShot's anchored view")
            .state;
        (state, hotshot)
    }

    /// Starts HotShot consensus, returns when consensus has finished
    async fn run_hotshot(&self, mut hotshot: HotShotHandle<TYPES, NODE>) {
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
        // TODO ED: In the future we should have each node generate transactions every round to simulate a more realistic network
        let tx_to_gen = transactions_per_round * (cmp::max(rounds / total_nodes, 1) + 5);
        {
            let mut txn_rng = rand::thread_rng();
            for _ in 0..tx_to_gen {
                let txn =
                    <<TYPES as NodeType>::StateType as TestableState>::create_random_transaction(
                        Some(&state),
                        &mut txn_rng,
                        padding as u64,
                    );
                txns.push_back(txn);
            }
        }
        error!("Generated {} transactions", tx_to_gen);

        error!("Adjusted padding size is {:?} bytes", adjusted_padding);
        let mut timed_out_views: u64 = 0;
        let mut round = 1;
        let mut total_transactions = 0;

        let start = Instant::now();

        error!("Starting hotshot!");
        hotshot.start().await;
        while round <= rounds {
            error!("Round {}:", round);

            let num_submitted = if node_index == ((round % total_nodes) as u64) {
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
            let view_results = hotshot.collect_round_events().await;

            match view_results {
                Ok((state, blocks)) => {
                    if let Some(_state) = state.get(0) {}
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
        let total_size = total_transactions * (padding as u64);

        // This assumes all transactions that were submitted made it through consensus, and does not account for the genesis block
        error!("All {rounds} rounds completed in {total_time_elapsed:?}. {timed_out_views} rounds timed out. {total_size} total bytes submitted");
    }

    /// Returns the network for this run
    fn get_network(&self) -> NETWORK;

    /// Returns the config for this run
    fn get_config(&self) -> NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>;
}

type Proposal<T> = ValidatingProposal<T, ValidatingLeaf<T>>;

// LIBP2P

/// Represents a libp2p-based run
pub struct Libp2pRun<TYPES: NodeType, I: NodeImplementation<TYPES>, MEMBERSHIP: Membership<TYPES>> {
    _bootstrap_nodes: Vec<(PeerId, Multiaddr)>,
    _node_type: NetworkNodeType,
    _bound_addr: Multiaddr,
    /// for libp2p layer
    _identity: Keypair,

    network: Libp2pCommChannel<
        TYPES,
        I,
        Proposal<TYPES>,
        QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
        MEMBERSHIP,
    >,
    config:
        NetworkConfig<<TYPES as NodeType>::SignatureKey, <TYPES as NodeType>::ElectionConfigType>,
}

/// yeesh maybe we should just implement SignatureKey for this...
pub fn libp2p_generate_indexed_identity(seed: [u8; 32], index: u64) -> Keypair {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&seed);
    hasher.update(&index.to_le_bytes());
    let new_seed = *hasher.finalize().as_bytes();
    let sk_bytes = SecretKey::try_from_bytes(new_seed).unwrap();
    let ed_kp = <EdKeypair as From<SecretKey>>::from(sk_bytes);
    #[allow(deprecated)]
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

#[async_trait]
impl<
        TYPES: NodeType,
        MEMBERSHIP: Membership<TYPES>,
        NODE: NodeImplementation<
            TYPES,
            Leaf = ValidatingLeaf<TYPES>,
            QuorumExchange = QuorumExchange<
                TYPES,
                ValidatingLeaf<TYPES>,
                ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                MEMBERSHIP,
                Libp2pCommChannel<
                    TYPES,
                    NODE,
                    ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                    QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
                    MEMBERSHIP,
                >,
                Message<TYPES, NODE>,
            >,
            CommitteeExchange = QuorumExchange<
                TYPES,
                ValidatingLeaf<TYPES>,
                ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                MEMBERSHIP,
                Libp2pCommChannel<
                    TYPES,
                    NODE,
                    ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                    QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
                    MEMBERSHIP,
                >,
                Message<TYPES, NODE>,
            >,
            Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
        >,
    >
    Run<
        TYPES,
        MEMBERSHIP,
        Libp2pCommChannel<
            TYPES,
            NODE,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
            MEMBERSHIP,
        >,
        NODE,
    > for Libp2pRun<TYPES, NODE, MEMBERSHIP>
where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    ValidatingLeaf<TYPES>: TestableLeaf,
    HotShot<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
    Self: Sync,
{
    async fn initialize_networking(
        config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    ) -> Libp2pRun<TYPES, NODE, MEMBERSHIP> {
        let (pubkey, _privkey) =
            <<TYPES as NodeType>::SignatureKey as SignatureKey>::generated_from_seed_indexed(
                config.seed,
                config.node_index,
            );
        let mut config = config;
        let libp2p_config = config
            .libp2p_config
            .take()
            .expect("Configuration is not for a Libp2p network");
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
        let port_index = match libp2p_config.index_ports {
            true => node_index,
            false => 0,
        };
        let bound_addr: Multiaddr = format!(
            "/{}/{}/tcp/{}",
            if libp2p_config.public_ip.is_ipv4() {
                "ip4"
            } else {
                "ip6"
            },
            libp2p_config.public_ip,
            libp2p_config.base_port as u64 + port_index
        )
        .parse()
        .unwrap();

        // generate network
        let mut config_builder = NetworkNodeConfigBuilder::default();
        assert!(config.config.total_nodes.get() > 2);
        let replicated_nodes = NonZeroUsize::new(config.config.total_nodes.get() - 2).unwrap();
        config_builder.replication_factor(replicated_nodes);
        config_builder.identity(identity.clone());

        config_builder.bound_addr(Some(bound_addr.clone()));

        let to_connect_addrs = bootstrap_nodes
            .iter()
            .map(|(peer_id, multiaddr)| (Some(*peer_id), multiaddr.clone()))
            .collect();

        config_builder.to_connect_addrs(to_connect_addrs);

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
        let network = Arc::new(
            Libp2pNetwork::new(
                NoMetrics::boxed(),
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
                // NOTE: this introduces an invariant that the keys are assigned using this indexed
                // function
                {
                    let mut keys = BTreeSet::new();
                    for i in 0..config.config.total_nodes.get() {
                        let pk =
                            <TYPES::SignatureKey as SignatureKey>::generated_from_seed_indexed(
                                config.seed,
                                i as u64,
                            )
                            .0;
                        keys.insert(pk);
                    }
                    keys
                },
            )
            .await
            .unwrap(),
        );

        let comm_channel = Libp2pCommChannel::<
            TYPES,
            NODE,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
            MEMBERSHIP,
        >::new(network);

        comm_channel.wait_for_ready().await;

        Libp2pRun {
            config,

            _bootstrap_nodes: bootstrap_nodes,
            _node_type: node_type,
            _identity: identity,
            _bound_addr: bound_addr,
            // _socket: stream,
            network: comm_channel,
        }
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
        NODE,
        ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
        QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
        MEMBERSHIP,
    > {
        self.network.clone()
    }
}

// WEB SERVER

/// Represents a web server-based run
pub struct WebServerRun<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    MEMBERSHIP: Membership<TYPES>,
> {
    config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    network: WebCommChannel<
        TYPES,
        I,
        Proposal<TYPES>,
        QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
        MEMBERSHIP,
    >,
}

#[async_trait]
impl<
        TYPES: NodeType,
        MEMBERSHIP: Membership<TYPES>,
        NODE: NodeImplementation<
            TYPES,
            Leaf = ValidatingLeaf<TYPES>,
            QuorumExchange = QuorumExchange<
                TYPES,
                ValidatingLeaf<TYPES>,
                ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                MEMBERSHIP,
                WebCommChannel<
                    TYPES,
                    NODE,
                    ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                    QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
                    MEMBERSHIP,
                >,
                Message<TYPES, NODE>,
            >,
            CommitteeExchange = QuorumExchange<
                TYPES,
                ValidatingLeaf<TYPES>,
                ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                MEMBERSHIP,
                WebCommChannel<
                    TYPES,
                    NODE,
                    ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                    QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
                    MEMBERSHIP,
                >,
                Message<TYPES, NODE>,
            >,
            Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
        >,
    >
    Run<
        TYPES,
        MEMBERSHIP,
        WebCommChannel<
            TYPES,
            NODE,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
            MEMBERSHIP,
        >,
        NODE,
    > for WebServerRun<TYPES, NODE, MEMBERSHIP>
where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    ValidatingLeaf<TYPES>: TestableLeaf,
    HotShot<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
    Self: Sync,
{
    async fn initialize_networking(
        config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    ) -> WebServerRun<TYPES, NODE, MEMBERSHIP> {
        // Generate our own key
        let (pub_key, _priv_key) =
            <<TYPES as NodeType>::SignatureKey as SignatureKey>::generated_from_seed_indexed(
                config.seed,
                config.node_index,
            );

        // Get the configuration for the web server
        let WebServerConfig {
            host,
            port,
            wait_between_polls,
        }: WebServerConfig = config.clone().web_server_config.unwrap();

        // Create the network
        let network: WebCommChannel<
            TYPES,
            NODE,
            Proposal<TYPES>,
            QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
            MEMBERSHIP,
        > = WebCommChannel::new(
            WebServerNetwork::create(&host.to_string(), port, wait_between_polls, pub_key).into(),
        );
        WebServerRun { config, network }
    }

    fn get_network(
        &self,
    ) -> WebCommChannel<
        TYPES,
        NODE,
        Proposal<TYPES>,
        QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
        MEMBERSHIP,
    > {
        self.network.clone()
    }

    fn get_config(&self) -> NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType> {
        self.config.clone()
    }
}

/// Holds the client connection to the orchestrator
pub struct OrchestratorClient {
    client: surf_disco::Client<ClientError>,
}

impl OrchestratorClient {
    /// Creates the client that connects to the orchestrator
    async fn connect_to_orchestrator(args: ValidatorArgs) -> Self {
        let base_url = format!("{0}:{1}", args.host, args.port);
        let base_url = format!("http://{base_url}").parse().unwrap();
        let client = surf_disco::Client::<ClientError>::new(base_url);
        // TODO ED: Add healthcheck wait here
        OrchestratorClient { client }
    }

    /// Sends an identify message to the server
    /// Returns this validator's node_index in the network
    async fn identify_with_orchestrator(&self, identity: String) -> u16 {
        let identity = identity.as_str();
        let f = |client: Client<ClientError>| {
            async move {
                let node_index: Result<u16, ClientError> = client
                    .post(&format!("api/identity/{identity}"))
                    .send()
                    .await;
                node_index
            }
            .boxed()
        };
        self.wait_for_fn_from_orchestrator(f).await
    }

    /// Returns the run configuration from the orchestrator
    /// Will block until the configuration is returned
    async fn get_config_from_orchestrator<TYPES: NodeType>(
        &self,
        node_index: u16,
    ) -> NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType> {
        let f = |client: Client<ClientError>| {
            async move {
                let config: Result<
                    NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
                    ClientError,
                > = client
                    .post(&format!("api/config/{node_index}"))
                    .send()
                    .await;
                config
            }
            .boxed()
        };
        self.wait_for_fn_from_orchestrator(f).await
    }

    /// Tells the orchestrator this validator is ready to start
    /// Blocks until the orchestrator indicates all nodes are ready to start
    async fn wait_for_all_nodes_ready(&self, node_index: u64) -> bool {
        let send_ready_f = |client: Client<ClientError>| {
            async move {
                let result: Result<_, ClientError> = client
                    .post("api/ready")
                    .body_json(&node_index)
                    .unwrap()
                    .send()
                    .await;
                result
            }
            .boxed()
        };
        self.wait_for_fn_from_orchestrator::<_, _, ()>(send_ready_f)
            .await;

        let wait_for_all_nodes_ready_f = |client: Client<ClientError>| {
            async move { client.get("api/start").send().await }.boxed()
        };
        self.wait_for_fn_from_orchestrator(wait_for_all_nodes_ready_f)
            .await
    }

    /// Generic function that waits for the orchestrator to return a non-error
    /// Returns whatever type the given function returns
    async fn wait_for_fn_from_orchestrator<F, Fut, GEN>(&self, f: F) -> GEN
    where
        F: Fn(Client<ClientError>) -> Fut,
        Fut: Future<Output = Result<GEN, ClientError>>,
    {
        loop {
            let client = self.client.clone();
            let res = f(client).await;
            match res {
                Ok(x) => break x,
                Err(_x) => {
                    async_sleep(Duration::from_millis(250)).await;
                }
            }
        }
    }
}

/// Main entry point for validators
pub async fn main_entry_point<
    TYPES: NodeType,
    MEMBERSHIP: Membership<TYPES>,
    NETWORK: CommunicationChannel<
        TYPES,
        Message<TYPES, NODE>,
        ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
        QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
        MEMBERSHIP,
    >,
    NODE: NodeImplementation<
        TYPES,
        Leaf = ValidatingLeaf<TYPES>,
        QuorumExchange = QuorumExchange<
            TYPES,
            ValidatingLeaf<TYPES>,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            MEMBERSHIP,
            NETWORK,
            Message<TYPES, NODE>,
        >,
        CommitteeExchange = QuorumExchange<
            TYPES,
            ValidatingLeaf<TYPES>,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            MEMBERSHIP,
            NETWORK,
            Message<TYPES, NODE>,
        >,
        Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
    >,
    RUN: Run<TYPES, MEMBERSHIP, NETWORK, NODE>,
>(
    args: ValidatorArgs,
) where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    ValidatingLeaf<TYPES>: TestableLeaf,
    HotShot<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
{
    setup_logging();
    setup_backtrace();

    error!("Starting validator");

    let orchestrator_client: OrchestratorClient =
        OrchestratorClient::connect_to_orchestrator(args.clone()).await;

    // Identify with the orchestrator
    let public_ip = match args.public_ip {
        Some(ip) => ip,
        None => IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
    };
    error!(
        "Identifying with orchestrator using IP address {}",
        public_ip.to_string()
    );
    let node_index: u16 = orchestrator_client
        .identify_with_orchestrator(public_ip.to_string())
        .await;
    error!("Finished identifying; our node index is {node_index}");
    error!("Getting config from orchestrator");

    let mut run_config = orchestrator_client
        .get_config_from_orchestrator::<TYPES>(node_index)
        .await;

    run_config.node_index = node_index.into();
    run_config.libp2p_config.as_mut().unwrap().public_ip = args.public_ip.unwrap();

    error!("Initializing networking");
    let run = RUN::initialize_networking(run_config.clone()).await;
    let (_state, hotshot) = run.initialize_state_and_hotshot().await;

    error!("Waiting for start command from orchestrator");
    orchestrator_client
        .wait_for_all_nodes_ready(run_config.clone().node_index)
        .await;

    error!("All nodes are ready!  Starting HotShot");
    run.run_hotshot(hotshot).await;
}
