use async_compatibility_layer::{
    art::async_sleep,
    logging::{setup_backtrace, setup_logging},
};
use async_lock::RwLock;
use async_trait::async_trait;
use clap::Parser;
use futures::Future;
use futures::StreamExt;
use hotshot::{
    traits::{
        implementations::{
            Libp2pCommChannel, Libp2pNetwork, MemoryStorage, WebCommChannel, WebServerNetwork,
        },
        NodeImplementation, Storage,
    },
    types::{SignatureKey, SystemContextHandle},
    SystemContext, ViewRunner,
};
use hotshot_orchestrator::{
    self,
    client::{OrchestratorClient, ValidatorArgs},
    config::{NetworkConfig, NetworkConfigFile, WebServerConfig},
};
use hotshot_task::task::FilterEvent;
use hotshot_types::event::{Event, EventType};
use hotshot_types::traits::election::ConsensusExchange;
use hotshot_types::traits::state::ConsensusTime;
use hotshot_types::{data::ViewNumber, vote::ViewSyncVote};
use hotshot_types::{
    data::{LeafType, TestableLeaf, ValidatingLeaf, ValidatingProposal},
    message::ValidatingMessage,
    traits::{
        consensus_type::validating_consensus::ValidatingConsensus,
        election::{Membership, ViewSyncExchange},
        metrics::NoMetrics,
        network::CommunicationChannel,
        node_implementation::{ExchangesType, NodeType, ValidatingExchanges},
        state::{TestableBlock, TestableState},
    },
    vote::QuorumVote,
    HotShotConfig,
};
use hotshot_types::{message::Message, traits::election::QuorumExchange};
use libp2p::{
    identity::{
        bn254::{Keypair as EdKeypair, SecretKey},
        Keypair,
    },
    multiaddr::{self, Protocol},
    Multiaddr,
};
use libp2p_identity::PeerId;
use libp2p_networking::network::{MeshParams, NetworkNodeConfigBuilder, NetworkNodeType};
use nll::nll_todo::nll_todo;
use rand::SeedableRng;
use std::fmt::Debug;
use std::net::Ipv4Addr;
use std::{
    cmp,
    collections::{BTreeSet, VecDeque},
    fs, mem,
    net::IpAddr,
    num::NonZeroUsize,
    str::FromStr,
    sync::Arc,
    time::Instant,
};
#[allow(deprecated)]
use tracing::error;
use rand_chacha::ChaCha20Rng;

// ORCHESTRATOR

#[derive(Parser, Debug, Clone)]
#[command(
    name = "Multi-machine consensus",
    about = "Simulates consensus among multiple machines"
)]
/// Arguments passed to the orchestrator
pub struct OrchestratorArgs {
    /// The address the orchestrator runs on
    pub host: IpAddr,
    /// The port the orchestrator runs on
    pub port: u16,
    /// The configuration file to be used for this run
    pub config_file: String,
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

    config.config.known_nodes_with_stake = (0..config.config.total_nodes.get())
    .map(|node_id| {
        config.config.known_nodes[node_id].get_stake_table_entry(1u64)
    })
    .collect();

    config
}

/// Runs the orchestrator
pub async fn run_orchestrator<
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    MEMBERSHIP: Membership<TYPES> + Debug,
    NETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
            MEMBERSHIP,
        > + Debug,
    VIEWSYNCNETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            ViewSyncVote<TYPES>,
            MEMBERSHIP,
        > + Debug,
    NODE: NodeImplementation<
        TYPES,
        Leaf = ValidatingLeaf<TYPES>,
        Exchanges = ValidatingExchanges<
            TYPES,
            Message<TYPES, NODE>,
            QuorumExchange<
                TYPES,
                ValidatingLeaf<TYPES>,
                ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                MEMBERSHIP,
                NETWORK,
                Message<TYPES, NODE>,
            >,
            ViewSyncExchange<
                TYPES,
                ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                MEMBERSHIP,
                VIEWSYNCNETWORK,
                Message<TYPES, NODE>,
            >,
        >,
        Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
        ConsensusMessage = ValidatingMessage<TYPES, NODE>,
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
pub trait Run<
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    MEMBERSHIP: Membership<TYPES> + Debug,
    NETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
            MEMBERSHIP,
        > + Debug,
    VIEWSYNCNETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            ViewSyncVote<TYPES>,
            MEMBERSHIP,
        > + Debug,
    NODE: NodeImplementation<
        TYPES,
        Leaf = ValidatingLeaf<TYPES>,
        Exchanges = ValidatingExchanges<
            TYPES,
            Message<TYPES, NODE>,
            QuorumExchange<
                TYPES,
                ValidatingLeaf<TYPES>,
                ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                MEMBERSHIP,
                NETWORK,
                Message<TYPES, NODE>,
            >,
            ViewSyncExchange<
                TYPES,
                ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                MEMBERSHIP,
                VIEWSYNCNETWORK,
                Message<TYPES, NODE>,
            >,
        >,
        Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
        ConsensusMessage = ValidatingMessage<TYPES, NODE>,
    >,
> where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    ValidatingLeaf<TYPES>: TestableLeaf,
    SystemContext<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
    Self: Sync,
{
    /// Initializes networking, returns self
    async fn initialize_networking(
        config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    ) -> Self;

    /// Initializes the genesis state and HotShot instance; does not start HotShot consensus
    /// # Panics if it cannot generate a genesis block, fails to initialize HotShot, or cannot
    /// get the anchored view
    async fn initialize_state_and_hotshot(
        &self,
    ) -> (TYPES::StateType, SystemContextHandle<TYPES, NODE>) {
        let genesis_block = TYPES::BlockType::genesis();
        let initializer =
            hotshot::HotShotInitializer::<TYPES, ValidatingLeaf<TYPES>>::from_genesis(
                genesis_block,
            )
            .expect("Couldn't generate genesis block");

        let config = self.get_config();

        // Get KeyPair for certificate Aggregation
        let (pk, sk) =
            TYPES::SignatureKey::generated_from_seed_indexed(config.seed, config.node_index);
        let entry = pk.get_stake_table_entry(1u64);
        let known_nodes = config.config.known_nodes.clone(); // PUBKEY
        let known_nodes_with_stake = config.config.known_nodes_with_stake.clone(); // PUBKEY + StakeValue

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

        let exchanges = NODE::Exchanges::create(
            known_nodes_with_stake.clone(),
            known_nodes.clone(),
            (election_config.clone(), ()),
            //Kaley todo: add view sync network
            (network.clone(), nll_todo(), ()),
            pk.clone(),
            entry.clone(),
            sk.clone(),
        );
        let hotshot = SystemContext::init(
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

        let state = hotshot
            .storage()
            .get_anchored_view()
            .await
            .expect("Couldn't get HotShot's anchored view")
            .state;
        (state, hotshot)
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
        let state = context.get_state().await;

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
        context.hotshot.start_consensus().await;
        let (mut event_stream, _streamid) = context.get_event_stream(FilterEvent::default()).await;
        let mut anchor_view: TYPES::Time = <TYPES::Time as ConsensusTime>::genesis();
        let mut num_successful_commits = 0;

        let total_nodes_u64 = total_nodes.get() as u64;

        let mut should_submit_txns = node_index == (*anchor_view % total_nodes_u64);

        loop {
            if should_submit_txns {
                for _ in 0..transactions_per_round {
                    let txn = txns.pop_front().unwrap();
                    tracing::info!("Submitting txn on round {}", round);
                    context.submit_transaction(txn).await.unwrap();
                }
                should_submit_txns = false;
            }

            match event_stream.next().await {
                None => {
                    panic!("Error! Event stream completed before consensus ended.");
                }
                Some(Event { view_number, event }) => {
                    match event {
                        EventType::Error { error } => {
                            error!("Error in consensus: {:?}", error);
                            // TODO what to do here
                        }
                        EventType::Decide { leaf_chain, qc, _ } => {
                            // this might be a obob
                            if let Some(leaf) = leaf_chain.get(0) {
                                let new_anchor = leaf.view_number;
                                if new_anchor >= anchor_view {
                                    anchor_view = leaf.view_number;
                                }
                                if (*anchor_view % total_nodes_u64) == node_index {
                                    should_submit_txns = true;
                                }
                            }
                            num_successful_commits += leaf_chain.len();
                            if num_successful_commits >= rounds {
                                break;
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
                            tracing::info!("view finished: {:?}", view_number);
                        }
                        _ => unimplemented!(),
                    }
                }
            }
        }

        // while round <= rounds {
        //     error!("Round {}:", round);
        //
        //     let num_submitted =
        //     if node_index == ((round % total_nodes) as u64) {
        //         for _ in 0..transactions_per_round {
        //             let txn = txns.pop_front().unwrap();
        //             tracing::info!("Submitting txn on round {}", round);
        //             hotshot.submit_transaction(txn).await.unwrap();
        //         }
        //         transactions_per_round
        //     } else {
        //         0
        //     };
        //     error!("Submitting {} transactions", num_submitted);
        //
        //     // Start consensus
        //     let view_results = nll_todo();
        //
        //     match view_results {
        //         Ok((leaf_chain, _qc)) => {
        //             let blocks: Vec<TYPES::BlockType> = leaf_chain
        //                 .into_iter()
        //                 .map(|leaf| leaf.get_deltas())
        //                 .collect();
        //
        //             for block in blocks {
        //                 total_transactions += block.txn_count();
        //             }
        //         }
        //         Err(e) => {
        //             timed_out_views += 1;
        //             error!("View: {:?}, failed with : {:?}", round, e);
        //         }
        //     }
        //
        //     round += 1;
        // }
        //
        let total_time_elapsed = start.elapsed();
        let total_size = total_transactions * (padding as u64);
        //
        // // This assumes all transactions that were submitted made it through consensus, and does not account for the genesis block
        error!("All {rounds} rounds completed in {total_time_elapsed:?}. {timed_out_views} rounds timed out. {total_size} total bytes submitted");
    }

    /// Returns the network for this run
    fn get_network(&self) -> NETWORK;

    /// Returns view sync network for this run KALEY TODO
    //fn get_view_sync_network(&self) -> VIEWSYNCNETWORK;

    /// Returns the config for this run
    fn get_config(&self) -> NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>;
}

type Proposal<T> = ValidatingProposal<T, ValidatingLeaf<T>>;

// LIBP2P

/// Represents a libp2p-based run
pub struct Libp2pRun<
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    I: NodeImplementation<TYPES>,
    MEMBERSHIP: Membership<TYPES>,
> {
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
    // view_sync_network: Libp2pCommChannel<
    //     TYPES,
    //     I,
    //     Proposal<TYPES>,
    //     ViewSyncVote<TYPES>,
    //     MEMBERSHIP,
    // >,
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
        TYPES: NodeType<ConsensusType = ValidatingConsensus>,
        MEMBERSHIP: Membership<TYPES> + Debug,
        NODE: NodeImplementation<
            TYPES,
            Leaf = ValidatingLeaf<TYPES>,
            Exchanges = ValidatingExchanges<
                TYPES,
                Message<TYPES, NODE>,
                QuorumExchange<
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
                ViewSyncExchange<
                    TYPES,
                    ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                    MEMBERSHIP,
                    Libp2pCommChannel<
                        TYPES,
                        NODE,
                        ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                        ViewSyncVote<TYPES>,
                        MEMBERSHIP,
                    >,
                    Message<TYPES, NODE>,
                >,
            >,
            Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
            ConsensusMessage = ValidatingMessage<TYPES, NODE>,
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
        Libp2pCommChannel<
            TYPES,
            NODE,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            ViewSyncVote<TYPES>,
            MEMBERSHIP,
        >,
        NODE,
    > for Libp2pRun<TYPES, NODE, MEMBERSHIP>
where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    ValidatingLeaf<TYPES>: TestableLeaf,
    SystemContext<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
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
                BTreeSet::new(),
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

/// Alias for the [`WebCommChannel`] for validating consensus.
type ValidatingWebCommChannel<TYPES, I, MEMBERSHIP> =
    WebCommChannel<TYPES, I, Proposal<TYPES>, QuorumVote<TYPES, ValidatingLeaf<TYPES>>, MEMBERSHIP>;

/// Represents a web server-based run
pub struct WebServerRun<
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    I: NodeImplementation<TYPES>,
    MEMBERSHIP: Membership<TYPES>,
> {
    config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    network: ValidatingWebCommChannel<TYPES, I, MEMBERSHIP>,
}

#[async_trait]
impl<
        TYPES: NodeType<ConsensusType = ValidatingConsensus>,
        MEMBERSHIP: Membership<TYPES> + Debug,
        NODE: NodeImplementation<
            TYPES,
            Leaf = ValidatingLeaf<TYPES>,
            Exchanges = ValidatingExchanges<
                TYPES,
                Message<TYPES, NODE>,
                QuorumExchange<
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
                ViewSyncExchange<
                    TYPES,
                    ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                    MEMBERSHIP,
                    WebCommChannel<
                        TYPES,
                        NODE,
                        ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                        ViewSyncVote<TYPES>,
                        MEMBERSHIP,
                    >,
                    Message<TYPES, NODE>,
                >,
            >,
            Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
            ConsensusMessage = ValidatingMessage<TYPES, NODE>,
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
        WebCommChannel<
            TYPES,
            NODE,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            ViewSyncVote<TYPES>,
            MEMBERSHIP,
        >,
        NODE,
    > for WebServerRun<TYPES, NODE, MEMBERSHIP>
where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    ValidatingLeaf<TYPES>: TestableLeaf,
    SystemContext<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
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
            WebServerNetwork::create(
                &host.to_string(),
                port,
                wait_between_polls,
                pub_key,
                nll_todo(),
                false,
            )
            .into(),
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

/// Main entry point for validators
pub async fn main_entry_point<
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    MEMBERSHIP: Membership<TYPES> + Debug,
    NETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
            MEMBERSHIP,
        > + Debug,
    VIEWSYNCNETWORK: CommunicationChannel<
            TYPES,
            Message<TYPES, NODE>,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            ViewSyncVote<TYPES>,
            MEMBERSHIP,
        > + Debug,
    NODE: NodeImplementation<
        TYPES,
        Leaf = ValidatingLeaf<TYPES>,
        Exchanges = ValidatingExchanges<
            TYPES,
            Message<TYPES, NODE>,
            QuorumExchange<
                TYPES,
                ValidatingLeaf<TYPES>,
                ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                MEMBERSHIP,
                NETWORK,
                Message<TYPES, NODE>,
            >,
            ViewSyncExchange<
                TYPES,
                ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                MEMBERSHIP,
                VIEWSYNCNETWORK,
                Message<TYPES, NODE>,
            >,
        >,
        Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
        ConsensusMessage = ValidatingMessage<TYPES, NODE>,
    >,
    RUN: Run<TYPES, MEMBERSHIP, NETWORK, VIEWSYNCNETWORK, NODE>,
>(
    args: ValidatorArgs,
) where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    ValidatingLeaf<TYPES>: TestableLeaf,
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
