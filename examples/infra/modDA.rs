use crate::infra::{load_config_from_file, OrchestratorArgs, OrchestratorClient, ValidatorArgs};

use async_compatibility_layer::logging::{setup_backtrace, setup_logging};
use async_trait::async_trait;
use commit::Commitment;
use either::Either;
use hotshot::{
    traits::{
        implementations::{MemoryStorage, WebCommChannel, WebServerNetwork},
        NodeImplementation,
    },
    types::{HotShotHandle, SignatureKey},
    HotShot, ViewRunner,
};
use hotshot_orchestrator::{
    self,
    config::{NetworkConfig, WebServerConfig},
};
use hotshot_types::traits::{
    election::ConsensusExchange,
    node_implementation::{CommitteeEx, QuorumEx},
};
use hotshot_types::{
    data::{DAProposal, LeafType, QuorumProposal, SequencingLeaf, TestableLeaf},
    message::SequencingMessage,
    traits::{
        consensus_type::sequencing_consensus::SequencingConsensus,
        election::Membership,
        metrics::NoMetrics,
        network::CommunicationChannel,
        node_implementation::{ExchangesType, NodeType, SequencingExchanges},
        state::{TestableBlock, TestableState},
    },
    vote::{DAVote, QuorumVote},
    HotShotConfig,
};
use hotshot_types::{
    message::Message,
    traits::election::{CommitteeExchange, QuorumExchange},
};
use hotshot_web_server::config::DEFAULT_WEB_SERVER_DA_PORT;
// use libp2p::{
//     identity::{
//         ed25519::{Keypair as EdKeypair, SecretKey},
//         Keypair,
//     },
//     multiaddr::{self, Protocol},
//     Multiaddr,
// };
// use libp2p_identity::PeerId;
// use libp2p_networking::network::{MeshParams, NetworkNodeConfigBuilder, NetworkNodeType};
use rand::SeedableRng;
use std::fmt::Debug;
use std::net::Ipv4Addr;
use std::{
    cmp,
    //collections::{BTreeSet, VecDeque},
    collections::VecDeque,
    //fs,
    mem,
    net::IpAddr,
    //num::NonZeroUsize,
    //str::FromStr,
    //sync::Arc,
    //time::{Duration, Instant},
    time::Instant,
};
//use surf_disco::error::ClientError;
//use surf_disco::Client;
#[allow(deprecated)]
use tracing::error;

/// Runs the orchestrator
pub async fn run_orchestrator_da<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    MEMBERSHIP: Membership<TYPES> + Debug,
    DANETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            DAProposal<TYPES>,
            DAVote<TYPES, SequencingLeaf<TYPES>>,
            MEMBERSHIP,
        > + Debug,
    QUORUMNETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
            QuorumVote<TYPES, SequencingLeaf<TYPES>>,
            MEMBERSHIP,
        > + Debug,
    NODE: NodeImplementation<
        TYPES,
        Leaf = SequencingLeaf<TYPES>,
        Exchanges = SequencingExchanges<
            TYPES,
            Message<TYPES, NODE>,
            QuorumExchange<
                TYPES,
                SequencingLeaf<TYPES>,
                QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
                MEMBERSHIP,
                QUORUMNETWORK,
                Message<TYPES, NODE>,
            >,
            CommitteeExchange<TYPES, MEMBERSHIP, DANETWORK, Message<TYPES, NODE>>,
        >,
        Storage = MemoryStorage<TYPES, SequencingLeaf<TYPES>>,
        ConsensusMessage = SequencingMessage<TYPES, NODE>,
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

// #[derive(Parser, Debug, Clone)]
// #[command(
//     name = "Multi-machine consensus",
//     about = "Simulates consensus among multiple machines"
// )]
/// Arguments passed to the validator
// pub struct DAValidatorArgs {
//     /// The address the orchestrator runs on
//     host: IpAddr,
//     /// The port the orchestrator runs on
//     port: u16,
//     /// This node's public IP address, for libp2p
//     /// If no IP address is passed in, it will default to 127.0.0.1
//     public_ip: Option<IpAddr>,
// }

/// Defines the behavior of a "run" of the network with a given configuration
#[async_trait]
pub trait RunDA<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    MEMBERSHIP: Membership<TYPES> + Debug,
    DANETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            DAProposal<TYPES>,
            DAVote<TYPES, SequencingLeaf<TYPES>>,
            MEMBERSHIP,
        > + Debug,
    QUORUMNETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
            QuorumVote<TYPES, SequencingLeaf<TYPES>>,
            MEMBERSHIP,
        > + Debug,
    NODE: NodeImplementation<
        TYPES,
        Leaf = SequencingLeaf<TYPES>,
        Exchanges = SequencingExchanges<
            TYPES,
            Message<TYPES, NODE>,
            QuorumExchange<
                TYPES,
                SequencingLeaf<TYPES>,
                QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
                MEMBERSHIP,
                QUORUMNETWORK,
                Message<TYPES, NODE>,
            >,
            CommitteeExchange<TYPES, MEMBERSHIP, DANETWORK, Message<TYPES, NODE>>,
        >,
        Storage = MemoryStorage<TYPES, SequencingLeaf<TYPES>>,
        ConsensusMessage = SequencingMessage<TYPES, NODE>,
    >,
> where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    SequencingLeaf<TYPES>: TestableLeaf,
    HotShot<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
    //NODE: TestableNodeImplementation<TYPES::ConsensusType, TYPES>,
    Self: Sync,
{
    /// Initializes networking, returns self
    async fn initialize_networking(
        config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    ) -> Self;

    /// Initializes the genesis state and HotShot instance; does not start HotShot consensus
    /// # Panics if it cannot generate a genesis block, fails to initialize HotShot, or cannot
    /// get the anchored view
    /// Note: sequencing leaf does not have state, so does not return state
    async fn initialize_state_and_hotshot(&self) -> HotShotHandle<TYPES, NODE> {
        let genesis_block = TYPES::BlockType::genesis();
        let initializer =
            hotshot::HotShotInitializer::<TYPES, SequencingLeaf<TYPES>>::from_genesis(
                genesis_block,
            )
            .expect("Couldn't generate genesis block");

        let config = self.get_config();

        let (pk, sk) =
            TYPES::SignatureKey::generated_from_seed_indexed(config.seed, config.node_index);
        let ek = jf_primitives::aead::KeyPair::generate(&mut rand_chacha::ChaChaRng::from_seed(
            config.seed,
        ));
        let known_nodes = config.config.known_nodes.clone();

        let da_network = self.get_da_network();
        let quorum_network = self.get_quorum_network();

        // Since we do not currently pass the election config type in the NetworkConfig, this will always be the default election config
        let quorum_election_config = config.config.election_config.clone().unwrap_or_else(|| {
            <QuorumEx<TYPES,NODE> as ConsensusExchange<
                TYPES,
                Message<TYPES, NODE>,
            >>::Membership::default_election_config(config.config.total_nodes.get() as u64)
        });

        let committee_election_config = <CommitteeEx<TYPES, NODE> as ConsensusExchange<
            TYPES,
            Message<TYPES, NODE>,
        >>::Membership::default_election_config(
            config.config.total_nodes.get() as u64
        );

        // let election_config = config.config.election_config.clone().unwrap_or_else(|| {
        //     <QuorumExchange<
        //         TYPES,
        //         SequencingLeaf<TYPES>,
        //         QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
        //         MEMBERSHIP,
        //         QUORUMNETWORK,
        //         Message<TYPES, NODE>,
        //     > as ConsensusExchange<TYPES, Message<TYPES, NODE>>>::Membership::default_election_config(
        //         config.config.total_nodes.get() as u64
        //     )
        // });

        let exchanges = NODE::Exchanges::create(
            known_nodes.clone(),
            (quorum_election_config, committee_election_config),
            (quorum_network.clone(), da_network.clone()),
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
            exchanges,
            initializer,
            NoMetrics::boxed(),
        )
        .await
        .expect("Could not init hotshot");

        // let state = hotshot
        //     .storage()
        //     .get_anchored_view()
        //     .await
        //     .expect("Couldn't get HotShot's anchored view")
        //     .state;
        // (state, hotshot)
        hotshot
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
        let _state = hotshot.get_state().await;

        // This assumes that no node will be a leader more than 5x the expected number of times they should be the leader
        // FIXME  is this a reasonable assumption when we start doing DA?
        // TODO ED: In the future we should have each node generate transactions every round to simulate a more realistic network
        let tx_to_gen = transactions_per_round * (cmp::max(rounds / total_nodes, 1) + 5);
        {
            let mut txn_rng = rand::thread_rng();
            for _ in 0..tx_to_gen {
                let txn =
                    <<TYPES as NodeType>::StateType as TestableState>::create_random_transaction(
                        None,
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
        let mut total_commitments = 0;

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
                Ok((leaf_chain, _qc)) => {
                    let blocks: Vec<Either<TYPES::BlockType, Commitment<TYPES::BlockType>>> =
                        leaf_chain
                            .into_iter()
                            .map(|leaf| leaf.get_deltas())
                            .collect();
                    for b in blocks.into_iter() {
                        b.either(
                            |block| total_transactions += block.txn_count(),
                            |_| total_commitments += 1,
                        );
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
        error!("Total commitments: {total_commitments}");
    }

    /// Returns the da network for this run
    fn get_da_network(&self) -> DANETWORK;

    /// Returns the quorum network for this run
    fn get_quorum_network(&self) -> QUORUMNETWORK;

    /// Returns the config for this run
    fn get_config(&self) -> NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>;
}

//type Proposal<T> = DAProposal<T>;

// WEB SERVER

/// Alias for the [`WebCommChannel`] for sequencing consensus.
type StaticDAComm<TYPES, I, MEMBERSHIP> =
    WebCommChannel<TYPES, I, DAProposal<TYPES>, DAVote<TYPES, SequencingLeaf<TYPES>>, MEMBERSHIP>;

/// Alias for the ['WebCommChannel'] for validating consensus
type StaticQuorumComm<TYPES, I, MEMBERSHIP> = WebCommChannel<
    TYPES,
    I,
    QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
    QuorumVote<TYPES, SequencingLeaf<TYPES>>,
    MEMBERSHIP,
>;

/// Represents a web server-based run
pub struct WebServerDARun<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<TYPES>,
    MEMBERSHIP: Membership<TYPES>,
> {
    config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    quorum_network: StaticQuorumComm<TYPES, I, MEMBERSHIP>,
    da_network: StaticDAComm<TYPES, I, MEMBERSHIP>,
}

#[async_trait]
impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        MEMBERSHIP: Membership<TYPES> + Debug,
        NODE: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            Exchanges = SequencingExchanges<
                TYPES,
                Message<TYPES, NODE>,
                QuorumExchange<
                    TYPES,
                    SequencingLeaf<TYPES>,
                    QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
                    MEMBERSHIP,
                    WebCommChannel<
                        TYPES,
                        NODE,
                        QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
                        QuorumVote<TYPES, SequencingLeaf<TYPES>>,
                        MEMBERSHIP,
                    >,
                    Message<TYPES, NODE>,
                >,
                CommitteeExchange<
                    TYPES,
                    MEMBERSHIP,
                    WebCommChannel<
                        TYPES,
                        NODE,
                        DAProposal<TYPES>,
                        DAVote<TYPES, SequencingLeaf<TYPES>>,
                        MEMBERSHIP,
                    >,
                    Message<TYPES, NODE>,
                >,
            >,
            Storage = MemoryStorage<TYPES, SequencingLeaf<TYPES>>,
            ConsensusMessage = SequencingMessage<TYPES, NODE>,
        >,
    >
    RunDA<
        TYPES,
        MEMBERSHIP,
        StaticDAComm<TYPES, NODE, MEMBERSHIP>,
        StaticQuorumComm<TYPES, NODE, MEMBERSHIP>,
        NODE,
    > for WebServerDARun<TYPES, NODE, MEMBERSHIP>
where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    SequencingLeaf<TYPES>: TestableLeaf,
    HotShot<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
    Self: Sync,
{
    async fn initialize_networking(
        config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    ) -> WebServerDARun<TYPES, NODE, MEMBERSHIP> {
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

        let known_nodes = config.config.known_nodes.clone();

        let mut committee_nodes = known_nodes.clone();
        committee_nodes.truncate(config.config.da_committee_size.into());

        // Create the network
        let quorum_network: WebCommChannel<
            TYPES,
            NODE,
            QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
            QuorumVote<TYPES, SequencingLeaf<TYPES>>,
            MEMBERSHIP,
        > = WebCommChannel::new(
            WebServerNetwork::create(
                &host.to_string(),
                port,
                wait_between_polls,
                pub_key.clone(),
                known_nodes,
            )
            .into(),
        );

        let da_network: WebCommChannel<
            TYPES,
            NODE,
            DAProposal<TYPES>,
            DAVote<TYPES, SequencingLeaf<TYPES>>,
            MEMBERSHIP,
        > = WebCommChannel::new(
            WebServerNetwork::create(
                &host.to_string(),
                DEFAULT_WEB_SERVER_DA_PORT,
                wait_between_polls,
                pub_key,
                committee_nodes,
            )
            .into(),
        );

        WebServerDARun {
            config,
            quorum_network,
            da_network,
        }
    }

    fn get_da_network(
        &self,
    ) -> WebCommChannel<
        TYPES,
        NODE,
        DAProposal<TYPES>,
        DAVote<TYPES, SequencingLeaf<TYPES>>,
        MEMBERSHIP,
    > {
        self.da_network.clone()
    }

    fn get_quorum_network(
        &self,
    ) -> WebCommChannel<
        TYPES,
        NODE,
        QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
        QuorumVote<TYPES, SequencingLeaf<TYPES>>,
        MEMBERSHIP,
    > {
        self.quorum_network.clone()
    }

    fn get_config(&self) -> NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType> {
        self.config.clone()
    }
}

/// Holds the client connection to the orchestrator
// pub struct OrchestratorClient {
//     client: surf_disco::Client<ClientError>,
// }

// impl OrchestratorClient {
//     /// Creates the client that connects to the orchestrator
//     async fn connect_to_orchestrator(args: ValidatorArgs) -> Self {
//         let base_url = format!("{0}:{1}", args.host, args.port);
//         let base_url = format!("http://{base_url}").parse().unwrap();
//         let client = surf_disco::Client::<ClientError>::new(base_url);
//         assert!(client.connect(None).await);
//         OrchestratorClient { client }
//     }

//     /// Sends an identify message to the server
//     /// Returns this validator's node_index in the network
//     async fn identify_with_orchestrator(&self, identity: String) -> u16 {
//         let identity = identity.as_str();
//         let f = |client: Client<ClientError>| {
//             async move {
//                 let node_index: Result<u16, ClientError> = client
//                     .post(&format!("api/identity/{identity}"))
//                     .send()
//                     .await;
//                 node_index
//             }
//             .boxed()
//         };
//         self.wait_for_fn_from_orchestrator(f).await
//     }

//     /// Returns the run configuration from the orchestrator
//     /// Will block until the configuration is returned
//     async fn get_config_from_orchestrator<TYPES: NodeType>(
//         &self,
//         node_index: u16,
//     ) -> NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType> {
//         let f = |client: Client<ClientError>| {
//             async move {
//                 let config: Result<
//                     NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
//                     ClientError,
//                 > = client
//                     .post(&format!("api/config/{node_index}"))
//                     .send()
//                     .await;
//                 config
//             }
//             .boxed()
//         };
//         self.wait_for_fn_from_orchestrator(f).await
//     }

//     /// Tells the orchestrator this validator is ready to start
//     /// Blocks until the orchestrator indicates all nodes are ready to start
//     async fn wait_for_all_nodes_ready(&self, node_index: u64) -> bool {
//         let send_ready_f = |client: Client<ClientError>| {
//             async move {
//                 let result: Result<_, ClientError> = client
//                     .post("api/ready")
//                     .body_json(&node_index)
//                     .unwrap()
//                     .send()
//                     .await;
//                 result
//             }
//             .boxed()
//         };
//         self.wait_for_fn_from_orchestrator::<_, _, ()>(send_ready_f)
//             .await;

//         let wait_for_all_nodes_ready_f = |client: Client<ClientError>| {
//             async move { client.get("api/start").send().await }.boxed()
//         };
//         self.wait_for_fn_from_orchestrator(wait_for_all_nodes_ready_f)
//             .await
//     }

//     /// Generic function that waits for the orchestrator to return a non-error
//     /// Returns whatever type the given function returns
//     async fn wait_for_fn_from_orchestrator<F, Fut, GEN>(&self, f: F) -> GEN
//     where
//         F: Fn(Client<ClientError>) -> Fut,
//         Fut: Future<Output = Result<GEN, ClientError>>,
//     {
//         loop {
//             let client = self.client.clone();
//             let res = f(client).await;
//             match res {
//                 Ok(x) => break x,
//                 Err(_x) => {
//                     async_sleep(Duration::from_millis(250)).await;
//                 }
//             }
//         }
//     }
// }

/// Main entry point for validators
pub async fn main_entry_point<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    MEMBERSHIP: Membership<TYPES> + Debug,
    DANETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            DAProposal<TYPES>,
            DAVote<TYPES, SequencingLeaf<TYPES>>,
            MEMBERSHIP,
        > + Debug,
    QUORUMNETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
            QuorumVote<TYPES, SequencingLeaf<TYPES>>,
            MEMBERSHIP,
        > + Debug,
    NODE: NodeImplementation<
        TYPES,
        Leaf = SequencingLeaf<TYPES>,
        Exchanges = SequencingExchanges<
            TYPES,
            Message<TYPES, NODE>,
            QuorumExchange<
                TYPES,
                SequencingLeaf<TYPES>,
                QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
                MEMBERSHIP,
                QUORUMNETWORK,
                Message<TYPES, NODE>,
            >,
            CommitteeExchange<TYPES, MEMBERSHIP, DANETWORK, Message<TYPES, NODE>>,
        >,
        Storage = MemoryStorage<TYPES, SequencingLeaf<TYPES>>,
        ConsensusMessage = SequencingMessage<TYPES, NODE>,
    >,
    RUNDA: RunDA<TYPES, MEMBERSHIP, DANETWORK, QUORUMNETWORK, NODE>,
>(
    args: ValidatorArgs,
) where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    SequencingLeaf<TYPES>: TestableLeaf,
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
    //run_config.libp2p_config.as_mut().unwrap().public_ip = args.public_ip.unwrap();

    error!("Initializing networking");
    let run = RUNDA::initialize_networking(run_config.clone()).await;
    let hotshot = run.initialize_state_and_hotshot().await;

    error!("Waiting for start command from orchestrator");
    orchestrator_client
        .wait_for_all_nodes_ready(run_config.clone().node_index)
        .await;

    error!("All nodes are ready!  Starting HotShot");
    run.run_hotshot(hotshot).await;
}
