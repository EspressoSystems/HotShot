use futures::FutureExt;
use hotshot_orchestrator::config::CentralizedWebServerConfig;

use async_std::task::sleep;
use futures::Future;
use std::{
    cmp,
    collections::{BTreeSet, VecDeque},
    fs,
    marker::PhantomData,
    mem,
    net::{IpAddr, SocketAddr},
    num::NonZeroUsize,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use surf_disco::Client;

use surf_disco::{error::ClientError, Error};

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
            CentralizedWebCommChannel, CentralizedWebServerNetwork, Libp2pCommChannel,
            Libp2pNetwork, MemoryStorage,
        },
        NetworkError, NodeImplementation, Storage,
    },
    types::{HotShotHandle, SignatureKey},
    HotShot, ViewRunner,
};
use hotshot_orchestrator::{
    self,
    config::{NetworkConfig, NetworkConfigFile},
};
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
use libp2p::{
    identity::{
        ed25519::{Keypair as EdKeypair, SecretKey},
        Keypair,
    },
    multiaddr::{self, Protocol},
    Multiaddr, PeerId,
};
use libp2p_networking::network::{MeshParams, NetworkNodeConfigBuilder, NetworkNodeType};
use tracing::{debug, error};

#[derive(Parser, Debug, Clone)]
#[command(
    name = "Multi-machine consensus",
    about = "Simulates consensus among multiple machines"
)]

pub struct OrchestratorArgs {
    /// The address the orchestrator runs on
    host: IpAddr,
    /// The port the orchestrator runs on
    port: u16,
    /// The configuration file to be used for this run
    config_file: String,
}

// TODO ED Does this need to actually return a result? Doesn't seem like it
// This only reads one file, unlike the old server that read multiple.  We didn't use that featuree anyway
pub fn load_config_from_file<TYPES: NodeType>(
    config_file: String,
) -> NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType> {
    let config_file_as_string: String = fs::read_to_string(config_file.as_str())
        .expect(format!("Could not read config file located at {config_file}").as_str());
    let config_toml: NetworkConfigFile =
        toml::from_str::<NetworkConfigFile>(&config_file_as_string)
            .expect("Unable to convert config file to TOML");

    let mut config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType> =
        config_toml.into();

    // Generate keys
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

pub async fn run_orchestrator<
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
        Membership = MEMBERSHIP,
        Networking = NETWORK,
        Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
    >,
>(
    OrchestratorArgs {
        host,
        port,
        config_file,
    }: OrchestratorArgs,
) {
    println!("Starting orchestrator",);
    let run_config = load_config_from_file::<TYPES>(config_file);

    println!("{:?}", run_config);
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

pub struct ValidatorArgs {
    /// The address the orchestrator runs on
    host: IpAddr,
    /// The port the orchestrator runs on
    port: u16,
}

#[async_trait]
pub trait Run<
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
        Membership = MEMBERSHIP,
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
    async fn initialize_networking(
        config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    ) -> Self;

    async fn initialize_state_and_hotshot(&self) -> (TYPES::StateType, HotShotHandle<TYPES, NODE>) {
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
        // TODO ED This will always be default
        let election_config = config.config.election_config.clone().unwrap_or_else(|| {
            NODE::Membership::default_election_config(config.config.total_nodes.get() as u64)
        });
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
        let tx_to_gen = transactions_per_round * (cmp::max(rounds / total_nodes, 1) + 5);
        error!("Generated {} transactions", tx_to_gen);
        {
            let mut txn_rng = rand::thread_rng();
            for _ in 0..tx_to_gen {
                // TODO make this u64...
                let txn =
                    <<TYPES as NodeType>::StateType as TestableState>::create_random_transaction(
                        Some(&state),
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
        let _expected_transactions = transactions_per_round * rounds;
        let total_size = total_transactions * (padding as u64);
        error!("All {rounds} rounds completed in {total_time_elapsed:?}");
        error!("{timed_out_views} rounds timed out");

        // This assumes all submitted transactions make it through consensus:
        error!(
            "{} total bytes submitted in {:?}",
            total_size, total_time_elapsed
        );
        debug!("All rounds completed");
    }

    fn get_network(&self) -> NETWORK;

    fn get_config(&self) -> NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>;
}

// TODO ED Perhaps reinstate in future
// pub enum RunType<TYPES: NodeType, MEMBERSHIP: Membership<TYPES>> {
//     Libp2pRun(Libp2pRun<TYPES, MEMBERSHIP>),
//     WebServerRun(WebServerRun<TYPES, MEMBERSHIP>),
// }

type Proposal<T> = ValidatingProposal<T, ValidatingLeaf<T>>;

// LIBP2P
pub struct Libp2pRun<TYPES: NodeType, MEMBERSHIP: Membership<TYPES>> {
    _bootstrap_nodes: Vec<(PeerId, Multiaddr)>,
    _node_type: NetworkNodeType,
    _bound_addr: Multiaddr,
    /// for libp2p layer
    _identity: Keypair,

    // _socket: TcpStreamUtil,
    network: Libp2pCommChannel<
        TYPES,
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

#[async_trait]
impl<
        TYPES: NodeType,
        MEMBERSHIP: Membership<TYPES>,
        NODE: NodeImplementation<
            TYPES,
            Leaf = ValidatingLeaf<TYPES>,
            Proposal = ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            Membership = MEMBERSHIP,
            Networking = Libp2pCommChannel<
                TYPES,
                ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
                MEMBERSHIP,
            >,
            Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
        >,
    >
    Run<
        TYPES,
        MEMBERSHIP,
        Libp2pCommChannel<
            TYPES,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
            MEMBERSHIP,
        >,
        NODE,
    > for Libp2pRun<TYPES, MEMBERSHIP>
where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    ValidatingLeaf<TYPES>: TestableLeaf,
    HotShot<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
    Self: Sync,
{
    async fn initialize_networking(
        config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    ) -> Libp2pRun<TYPES, MEMBERSHIP> {
        let (pubkey, _privkey) =
            <<TYPES as NodeType>::SignatureKey as SignatureKey>::generated_from_seed_indexed(
                config.seed,
                config.node_index,
            );
        let mut config = config;
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
            // NOTE: this introduces an invariant that the keys are assigned using this indexed
            // function
            {
                let mut keys = BTreeSet::new();
                for i in 0..config.config.total_nodes.get() {
                    let pk = <TYPES::SignatureKey as SignatureKey>::generated_from_seed_indexed(
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
        .map(
            Libp2pCommChannel::<
                TYPES,
                ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
                MEMBERSHIP,
            >::new,
        )
        .unwrap();

        Libp2pRun {
            config,

            _bootstrap_nodes: bootstrap_nodes,
            _node_type: node_type,
            _identity: identity,
            _bound_addr: bound_addr,
            // _socket: stream,
            network,
        }
        // network
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

pub struct WebServerRun<TYPES: NodeType, MEMBERSHIP: Membership<TYPES>> {
    config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    network: CentralizedWebCommChannel<
        TYPES,
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
            Proposal = ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            Vote = QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
            Membership = MEMBERSHIP,
            Networking = CentralizedWebCommChannel<
                TYPES,
                ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
                MEMBERSHIP,
            >,
            Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
        >,
    >
    Run<
        TYPES,
        MEMBERSHIP,
        CentralizedWebCommChannel<
            TYPES,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
            MEMBERSHIP,
        >,
        NODE,
    > for WebServerRun<TYPES, MEMBERSHIP>
where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    ValidatingLeaf<TYPES>: TestableLeaf,
    HotShot<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
    Self: Sync,
{
    async fn initialize_networking(
        config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    ) -> WebServerRun<TYPES, MEMBERSHIP> {
        // TODO ED Retrun networking

        // Generate our own key
        let (pub_key, _priv_key) =
            <<TYPES as NodeType>::SignatureKey as SignatureKey>::generated_from_seed_indexed(
                config.seed,
                config.node_index,
            );

        let CentralizedWebServerConfig {
            host,
            port,
            wait_between_polls,
        }: CentralizedWebServerConfig = config.clone().centralized_web_server_config.unwrap();

        let network: CentralizedWebCommChannel<
            TYPES,
            Proposal<TYPES>,
            QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
            MEMBERSHIP,
        > = CentralizedWebCommChannel::new(CentralizedWebServerNetwork::create(
            &host.to_string(),
            port,
            wait_between_polls,
            pub_key,
        ));
        WebServerRun { config, network }
    }

    fn get_network(
        &self,
    ) -> CentralizedWebCommChannel<
        TYPES,
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

pub struct OrchestratorClient {
    client: surf_disco::Client<ClientError>,
    // membership: PhantomData<(TYPES)>,
}

impl OrchestratorClient {
    async fn connect_to_orchestrator(args: ValidatorArgs) -> Self {
        let base_url = format!("{0}:{1}", args.host, args.port);
        let base_url = format!("http://{base_url}").parse().unwrap();
        let client = surf_disco::Client::<ClientError>::new(base_url);
        // TODO ED insert healthcheck here
        OrchestratorClient {
            client,
            // membership: PhantomData<(TYPES)>::default()
        }
    }

    // Will block until the config is returned --> Could make this a function as arg to a generic wait function
    async fn get_config_from_orchestrator<TYPES: NodeType>(
        &self,
    ) -> NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType> {
        let f = |client: Client<ClientError>| {
            async move {
                let config: Result<
                    NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
                    ClientError,
                > = client.post("api/config").send().await;
                config
            }
            .boxed()
        };
        self.wait_for_fn_from_orchestrator(f).await
    }

    async fn wait_for_all_nodes_ready(&self, node_index: u64) -> bool {
        // let result: Result<(), ServerError> = client
        // .post("api/ready")
        // .body_json(&node_index)
        // .unwrap()
        // .send()
        // .await;

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
        let _result: () = self.wait_for_fn_from_orchestrator(send_ready_f).await;

        let wait_for_all_nodes_ready_f = |client: Client<ClientError>| {
            async move { client.get("api/start").send().await }.boxed()
        };
        self.wait_for_fn_from_orchestrator(wait_for_all_nodes_ready_f)
            .await
    }

    async fn wait_for_fn_from_orchestrator<F, Fut, GEN>(&self, mut f: F) -> GEN
    where
        F: Fn(Client<ClientError>) -> Fut,
        Fut: Future<Output = Result<GEN, ClientError>>,
    {
        let result = loop {
            // TODO ED Move this outside the loop
            let client = self.client.clone();
            let res = f(client).await;
            match res {
                Ok(x) => break x,
                Err(_x) => ({async_sleep(Duration::from_millis(250)).await;} ),
            }
        };
        result
    }
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
        Membership = MEMBERSHIP,
        Networking = NETWORK,
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

    print!("Running validator");

    let orchestrator_client: OrchestratorClient =
        OrchestratorClient::connect_to_orchestrator(args).await;
    let run_config = orchestrator_client
        .get_config_from_orchestrator::<TYPES>()
        .await;

    let run = RUN::initialize_networking(run_config.clone()).await;

    // let run = RUN::new(run_config.clone(), network);

    let (_state, hotshot) = run.initialize_state_and_hotshot().await;

    orchestrator_client
        .wait_for_all_nodes_ready(run_config.node_index)
        .await;

    run.run_hotshot(hotshot).await;
}
