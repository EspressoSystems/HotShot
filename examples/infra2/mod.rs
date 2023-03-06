#![allow(unused)]
use futures::FutureExt;

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
    // TODO ED Rename this once we get rid of old file
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
    // async fn connect_to_orchestrator() {}

    // // Will block until the config is returned --> Could make this a function as arg to a generic wait function
    // async fn get_config_from_orchestrator() {}

    // async fn wait_for_fn_from_orchestrator() {}

    // Also wait for networking to be ready using the CommChannel function (see other file for example)
    // TODO ED Make separate impls of this, should return the Run struct
    async fn initialize_networking() {}

    async fn initialize_hotshot() {}

    async fn start_hotshot() {}
}

// TODO ED Perhaps reinstate in future
// pub enum RunType<TYPES: NodeType, MEMBERSHIP: Membership<TYPES>> {
//     Libp2pRun(Libp2pRun<TYPES, MEMBERSHIP>),
//     WebServerRun(WebServerRun<TYPES, MEMBERSHIP>),
// }

type Proposal<T> = ValidatingProposal<T, ValidatingLeaf<T>>;

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
    async fn get_config_from_orchestrator<TYPES: NodeType>(self) {
        let mut f =  |client: Client<ClientError>| {async move {
            let config: Result<
                NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
                ClientError,
            > = client.post("api/config").send().await;
            config
        }.boxed()};
        self.wait_for_fn_from_orchestrator(f).await;
    }


    async fn wait_for_fn_from_orchestrator<F, Fut, GEN>(self, mut f:  F) -> GEN
    where
        F: Fn(Client<ClientError>) -> Fut,
        Fut: Future<Output = Result<GEN, ClientError>>,
    {
        // let mut result = f(self.client).await;
        // // TODO ED Add while loop here, change bool to GEN type, and return GEN Type
        // while result.is_err() {
        //     println!("Sleeping"); 
        //     async_sleep(Duration::from_millis(250));
        //     result = f(self.client).await;
        // }
        let result = loop {
            // TODO ED Move this outside the loop
            let client = self.client.clone();
            let res = f(client).await;
            match res {
                Ok(x) => break x,
                Err(x) => (), 
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
    CONFIG: Run<TYPES, MEMBERSHIP, NETWORK, NODE>,
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
    orchestrator_client
        .get_config_from_orchestrator::<TYPES>()
        .await;
}
