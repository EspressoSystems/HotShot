use crate::infra::{load_config_from_file, OrchestratorArgs};

use async_compatibility_layer::logging::{setup_backtrace, setup_logging};
use async_trait::async_trait;
use futures::StreamExt;
use hotshot::HotShotSequencingConsensusApi;
use hotshot::{
    traits::{
        implementations::{MemoryStorage, WebCommChannel, WebServerNetwork},
        NodeImplementation,
    },
    types::{SignatureKey, SystemContextHandle},
    HotShotType, SystemContext, ViewRunner,
};
use hotshot_consensus::traits::SequencingConsensusApi;
use hotshot_orchestrator::{
    self,
    client::{OrchestratorClient, ValidatorArgs},
    config::{NetworkConfig, WebServerConfig},
};
use hotshot_task::task::FilterEvent;
use hotshot_types::event::{Event, EventType};
use hotshot_types::message::DataMessage;
use hotshot_types::traits::state::ConsensusTime;
use hotshot_types::{
    certificate::ViewSyncCertificate,
    message::Message,
    traits::election::{CommitteeExchange, QuorumExchange},
};
use hotshot_types::{
    data::ViewNumber,
    traits::{
        election::{ConsensusExchange, ViewSyncExchange},
        node_implementation::{CommitteeEx, QuorumEx},
    },
};
use hotshot_types::{
    data::{DAProposal, QuorumProposal, SequencingLeaf, TestableLeaf},
    message::SequencingMessage,
    traits::{
        consensus_type::sequencing_consensus::SequencingConsensus,
        election::Membership,
        metrics::NoMetrics,
        network::CommunicationChannel,
        node_implementation::{ExchangesType, NodeType, SequencingExchanges},
        state::{TestableBlock, TestableState},
    },
    vote::{DAVote, QuorumVote, ViewSyncVote},
    HotShotConfig,
};
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
use tracing::warn;

/// Runs the orchestrator
pub async fn run_orchestrator_da<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    MEMBERSHIP: Membership<TYPES> + Debug,
    DANETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            DAProposal<TYPES>,
            DAVote<TYPES>,
            MEMBERSHIP,
        > + Debug,
    QUORUMNETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
            QuorumVote<TYPES, SequencingLeaf<TYPES>>,
            MEMBERSHIP,
        > + Debug,
    VIEWSYNCNETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            ViewSyncCertificate<TYPES>,
            ViewSyncVote<TYPES>,
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
            ViewSyncExchange<
                TYPES,
                ViewSyncCertificate<TYPES>,
                MEMBERSHIP,
                VIEWSYNCNETWORK,
                Message<TYPES, NODE>,
            >,
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

/// Defines the behavior of a "run" of the network with a given configuration
#[async_trait]
pub trait RunDA<
    TYPES: NodeType<ConsensusType = SequencingConsensus, Time = ViewNumber>,
    MEMBERSHIP: Membership<TYPES> + Debug,
    DANETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            DAProposal<TYPES>,
            DAVote<TYPES>,
            MEMBERSHIP,
        > + Debug,
    QUORUMNETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
            QuorumVote<TYPES, SequencingLeaf<TYPES>>,
            MEMBERSHIP,
        > + Debug,
    VIEWSYNCNETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            ViewSyncCertificate<TYPES>,
            ViewSyncVote<TYPES>,
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
            ViewSyncExchange<
                TYPES,
                ViewSyncCertificate<TYPES>,
                MEMBERSHIP,
                VIEWSYNCNETWORK,
                Message<TYPES, NODE>,
            >,
        >,
        Storage = MemoryStorage<TYPES, SequencingLeaf<TYPES>>,
        ConsensusMessage = SequencingMessage<TYPES, NODE>,
    >,
> where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    SequencingLeaf<TYPES>: TestableLeaf,
    SystemContext<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
    Self: Sync,
    SystemContext<SequencingConsensus, TYPES, NODE>: HotShotType<TYPES, NODE>,
{
    /// Initializes networking, returns self
    async fn initialize_networking(
        config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    ) -> Self;

    /// Initializes the genesis state and HotShot instance; does not start HotShot consensus
    /// # Panics if it cannot generate a genesis block, fails to initialize HotShot, or cannot
    /// get the anchored view
    /// Note: sequencing leaf does not have state, so does not return state
    async fn initialize_state_and_hotshot(&self) -> SystemContextHandle<TYPES, NODE> {
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
        let view_sync_network = self.get_view_sync_network();

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
            config.config.da_committee_size.try_into().unwrap(),
        );

        let exchanges = NODE::Exchanges::create(
            known_nodes.clone(),
            (quorum_election_config, committee_election_config),
            (
                quorum_network.clone(),
                view_sync_network.clone(),
                da_network.clone(),
            ),
            pk.clone(),
            sk.clone(),
            ek.clone(),
        );

        SystemContext::init(
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
        .expect("Could not init hotshot")
    }

    /// Starts HotShot consensus, returns when consensus has finished
    async fn run_hotshot(&self, mut context: SystemContextHandle<TYPES, NODE>) {
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

        // TODO ED: In the future we should have each node generate transactions every round to simulate a more realistic network
        let tx_to_gen = transactions_per_round * rounds * 3;
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
        let mut round = 0;
        let mut total_transactions = 0;

        let start = Instant::now();

        error!("Starting hotshot!");
        let (mut event_stream, _streamid) = context.get_event_stream(FilterEvent::default()).await;
        let mut anchor_view: TYPES::Time = <TYPES::Time as ConsensusTime>::genesis();
        let mut num_successful_commits = 0;

        let total_nodes_u64 = total_nodes.get() as u64;

        let api = HotShotSequencingConsensusApi {
            inner: context.hotshot.inner.clone(),
        };

        context.hotshot.start_consensus().await;

        loop {
            match event_stream.next().await {
                None => {
                    panic!("Error! Event stream completed before consensus ended.");
                }
                Some(Event { event, .. }) => {
                    match event {
                        EventType::Error { error } => {
                            error!("Error in consensus: {:?}", error);
                            // TODO what to do here
                        }
                        EventType::Decide {
                            leaf_chain,
                            qc: _,
                            block_size,
                        } => {
                            // this might be a obob
                            if let Some(leaf) = leaf_chain.get(0) {
                                warn!("Decide event for leaf: {}", *leaf.view_number);

                                let new_anchor = leaf.view_number;
                                if new_anchor >= anchor_view {
                                    anchor_view = leaf.view_number;
                                }
                            }

                            if let Some(size) = block_size {
                                total_transactions += size;
                            }

                            num_successful_commits += leaf_chain.len();
                            if num_successful_commits >= rounds {
                                break;
                            }

                            if leaf_chain.len() > 1 {
                                error!(
                                    "Leaf chain is greater than 1 with len {}",
                                    leaf_chain.len()
                                );
                            }
                            // when we make progress, submit new events
                        }
                        EventType::ReplicaViewTimeout { view_number } => {
                            error!("Timed out as a replicas in view {:?}", view_number);
                        }
                        EventType::NextLeaderViewTimeout { view_number } => {
                            error!("Timed out as the next leader in view {:?}", view_number);
                        }
                        EventType::ViewFinished { view_number } => {
                            if *view_number > round {
                                round = *view_number;
                                tracing::error!("view finished: {:?}", view_number);
                                for _ in 0..transactions_per_round {
                                    if node_index >= total_nodes_u64 - 10 {
                                        let txn = txns.pop_front().unwrap();

                                        tracing::warn!("Submitting txn on round {}", round);

                                        let result = api
                                            .send_transaction(DataMessage::SubmitTransaction(
                                                txn.clone(),
                                                TYPES::Time::new(0),
                                            ))
                                            .await;

                                        if result.is_err() {
                                            error! (
                                            "Could not send transaction to web server on round {}",
                                            round
                                        )
                                        }
                                    }
                                }
                            }
                        }
                        _ => unimplemented!(),
                    }
                }
            }

            round += 1;
        }

        // Output run results
        let total_time_elapsed = start.elapsed();
        error!("{rounds} rounds completed in {total_time_elapsed:?} - Total transactions committed: {total_transactions} - Total commitments: {num_successful_commits}");
    }

    /// Returns the da network for this run
    fn get_da_network(&self) -> DANETWORK;

    /// Returns the quorum network for this run
    fn get_quorum_network(&self) -> QUORUMNETWORK;

    ///Returns view sync network for this run
    fn get_view_sync_network(&self) -> VIEWSYNCNETWORK;

    /// Returns the config for this run
    fn get_config(&self) -> NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>;
}

// WEB SERVER

/// Alias for the [`WebCommChannel`] for sequencing consensus.
type StaticDAComm<TYPES, I, MEMBERSHIP> =
    WebCommChannel<TYPES, I, DAProposal<TYPES>, DAVote<TYPES>, MEMBERSHIP>;

/// Alias for the ['WebCommChannel'] for validating consensus
type StaticQuorumComm<TYPES, I, MEMBERSHIP> = WebCommChannel<
    TYPES,
    I,
    QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
    QuorumVote<TYPES, SequencingLeaf<TYPES>>,
    MEMBERSHIP,
>;

/// Alias for the ['WebCommChannel'] for view sync consensus
type StaticViewSyncComm<TYPES, I, MEMBERSHIP> =
    WebCommChannel<TYPES, I, ViewSyncCertificate<TYPES>, ViewSyncVote<TYPES>, MEMBERSHIP>;

/// Represents a web server-based run
pub struct WebServerDARun<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<TYPES>,
    MEMBERSHIP: Membership<TYPES>,
> {
    config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    quorum_network: StaticQuorumComm<TYPES, I, MEMBERSHIP>,
    da_network: StaticDAComm<TYPES, I, MEMBERSHIP>,
    view_sync_network: StaticViewSyncComm<TYPES, I, MEMBERSHIP>,
}

#[async_trait]
impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus, Time = ViewNumber>,
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
                    WebCommChannel<TYPES, NODE, DAProposal<TYPES>, DAVote<TYPES>, MEMBERSHIP>,
                    Message<TYPES, NODE>,
                >,
                ViewSyncExchange<
                    TYPES,
                    ViewSyncCertificate<TYPES>,
                    MEMBERSHIP,
                    WebCommChannel<
                        TYPES,
                        NODE,
                        ViewSyncCertificate<TYPES>,
                        ViewSyncVote<TYPES>,
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
        StaticViewSyncComm<TYPES, NODE, MEMBERSHIP>,
        NODE,
    > for WebServerDARun<TYPES, NODE, MEMBERSHIP>
where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    SequencingLeaf<TYPES>: TestableLeaf,
    SystemContext<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
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

        let underlying_quorum_network = WebServerNetwork::create(
            &host.to_string(),
            port,
            wait_between_polls,
            pub_key.clone(),
            false,
        );

        // Create the network
        let quorum_network: WebCommChannel<
            TYPES,
            NODE,
            QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
            QuorumVote<TYPES, SequencingLeaf<TYPES>>,
            MEMBERSHIP,
        > = WebCommChannel::new(underlying_quorum_network.clone().into());

        let view_sync_network: WebCommChannel<
            TYPES,
            NODE,
            ViewSyncCertificate<TYPES>,
            ViewSyncVote<TYPES>,
            MEMBERSHIP,
        > = WebCommChannel::new(underlying_quorum_network.into());

        let WebServerConfig {
            host,
            port,
            wait_between_polls,
        }: WebServerConfig = config.clone().da_web_server_config.unwrap();

        // Each node runs the DA network so that leaders have access to transactions and DA votes
        let da_network: WebCommChannel<TYPES, NODE, DAProposal<TYPES>, DAVote<TYPES>, MEMBERSHIP> =
            WebCommChannel::new(
                WebServerNetwork::create(
                    &host.to_string(),
                    port,
                    wait_between_polls,
                    pub_key,
                    true,
                )
                .into(),
            );

        WebServerDARun {
            config,
            quorum_network,
            da_network,
            view_sync_network,
        }
    }

    fn get_da_network(
        &self,
    ) -> WebCommChannel<TYPES, NODE, DAProposal<TYPES>, DAVote<TYPES>, MEMBERSHIP> {
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

    fn get_view_sync_network(
        &self,
    ) -> WebCommChannel<TYPES, NODE, ViewSyncCertificate<TYPES>, ViewSyncVote<TYPES>, MEMBERSHIP>
    {
        self.view_sync_network.clone()
    }

    fn get_config(&self) -> NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType> {
        self.config.clone()
    }
}

/// Main entry point for validators
pub async fn main_entry_point<
    TYPES: NodeType<ConsensusType = SequencingConsensus, Time = ViewNumber>,
    MEMBERSHIP: Membership<TYPES> + Debug,
    DANETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            DAProposal<TYPES>,
            DAVote<TYPES>,
            MEMBERSHIP,
        > + Debug,
    QUORUMNETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
            QuorumVote<TYPES, SequencingLeaf<TYPES>>,
            MEMBERSHIP,
        > + Debug,
    VIEWSYNCNETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            ViewSyncCertificate<TYPES>,
            ViewSyncVote<TYPES>,
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
            ViewSyncExchange<
                TYPES,
                ViewSyncCertificate<TYPES>,
                MEMBERSHIP,
                VIEWSYNCNETWORK,
                Message<TYPES, NODE>,
            >,
        >,
        Storage = MemoryStorage<TYPES, SequencingLeaf<TYPES>>,
        ConsensusMessage = SequencingMessage<TYPES, NODE>,
    >,
    RUNDA: RunDA<TYPES, MEMBERSHIP, DANETWORK, QUORUMNETWORK, VIEWSYNCNETWORK, NODE>,
>(
    args: ValidatorArgs,
) where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    SequencingLeaf<TYPES>: TestableLeaf,
    SystemContext<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
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
