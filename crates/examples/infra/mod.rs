#![allow(clippy::panic)]
use std::{
    fmt::Debug,
    fs,
    marker::PhantomData,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    num::NonZeroUsize,
    time::{Duration, Instant},
};

use async_compatibility_layer::{
    art::async_sleep,
    logging::{setup_backtrace, setup_logging},
};
use async_trait::async_trait;
use cdn_broker::reexports::{crypto::signature::KeyPair, message::Topic};
use chrono::Utc;
use clap::{value_parser, Arg, Command, Parser};
use futures::StreamExt;
use hotshot::{
    traits::{
        implementations::{
            derive_libp2p_peer_id, CombinedNetworks, Libp2pNetwork, PushCdnNetwork,
            WebServerNetwork, WrappedSignatureKey,
        },
        BlockPayload, NodeImplementation,
    },
    types::SystemContextHandle,
    Memberships, Networks, SystemContext,
};
use hotshot_example_types::{
    block_types::{TestBlockHeader, TestBlockPayload, TestTransaction},
    node_types::{Libp2pImpl, PushCdnImpl},
    state_types::TestInstanceState,
    storage_types::TestStorage,
};
use hotshot_orchestrator::{
    self,
    client::{BenchResults, OrchestratorClient, ValidatorArgs},
    config::{
        BuilderType, CombinedNetworkConfig, NetworkConfig, NetworkConfigFile, NetworkConfigSource,
        WebServerConfig,
    },
};
use hotshot_testing::block_builder::{
    RandomBuilderImplementation, SimpleBuilderImplementation, TestBuilderImplementation,
};
use hotshot_types::{
    consensus::ConsensusMetricsValue,
    data::{Leaf, TestableLeaf},
    event::{Event, EventType},
    message::Message,
    traits::{
        block_contents::{BlockHeader, TestableBlock},
        election::Membership,
        network::ConnectedNetwork,
        node_implementation::{ConsensusTime, NodeType},
        states::TestableState,
    },
    HotShotConfig, PeerConfig, ValidatorConfig,
};
use rand::{rngs::StdRng, SeedableRng};
use surf_disco::Url;
use tracing::{error, info, warn};
use vbs::version::StaticVersionType;

#[derive(Debug, Clone)]
/// Arguments passed to the orchestrator
pub struct OrchestratorArgs<TYPES: NodeType> {
    /// The url the orchestrator runs on; this should be in the form of `http://localhost:5555` or `http://0.0.0.0:5555`
    pub url: Url,
    /// The configuration file to be used for this run
    pub config: NetworkConfig<TYPES::SignatureKey>,
}

#[derive(Parser, Debug, Clone)]
#[command(
    name = "Multi-machine consensus",
    about = "Simulates consensus among multiple machines"
)]
/// The configuration file to be used for this run
pub struct ConfigArgs {
    /// The configuration file to be used for this run
    pub config_file: String,
}

impl Default for ConfigArgs {
    fn default() -> Self {
        Self {
            config_file: "./crates/orchestrator/run-config.toml".to_string(),
        }
    }
}

/// Reads the orchestrator initialization config from the command line
/// # Panics
/// If unable to read the config file from the command line
#[allow(clippy::too_many_lines)]
pub fn read_orchestrator_init_config<TYPES: NodeType>() -> (NetworkConfig<TYPES::SignatureKey>, Url)
{
    // assign default setting
    let mut orchestrator_url = Url::parse("http://localhost:4444").unwrap();
    let mut args = ConfigArgs::default();
    // start reading from command line
    let matches = Command::new("orchestrator")
        .arg(
            Arg::new("config_file")
                .short('c')
                .long("config_file")
                .value_name("FILE")
                .help("Sets a custom config file with default values, some might be changed if they are set manually in the command line")
                .required(true),
        )
        .arg(
            Arg::new("total_nodes")
                .short('n')
                .long("total_nodes")
                .value_name("NUM")
                .help("Sets the total number of nodes")
                .required(false),
        )
        .arg(
            Arg::new("da_committee_size")
                .short('d')
                .long("da_committee_size")
                .value_name("NUM")
                .help("Sets the size of the data availability committee")
                .required(false),
        )
        .arg(
            Arg::new("transactions_per_round")
                .short('t')
                .long("transactions_per_round")
                .value_name("NUM")
                .help("Sets the number of transactions per round")
                .required(false),
        )
        .arg(
            Arg::new("transaction_size")
                .short('s')
                .long("transaction_size")
                .value_name("NUM")
                .help("Sets the size of each transaction in bytes")
                .required(false),
        )
        .arg(
            Arg::new("rounds")
                .short('r')
                .long("rounds")
                .value_name("NUM")
                .help("Sets the number of rounds to run")
                .required(false),
        )
        .arg(
            Arg::new("commit_sha")
                .short('m')
                .long("commit_sha")
                .value_name("SHA")
                .help("Sets the commit sha to output in the results")
                .required(false),
        )
        .arg(
            Arg::new("orchestrator_url")
                .short('u')
                .long("orchestrator_url")
                .value_name("URL")
                .help("Sets the url of the orchestrator")
                .required(false),
        )
        .arg(
            Arg::new("webserver_url")
                .short('w')
                .long("webserver_url")
                .value_name("URL")
                .help("Sets the url of the webserver")
                .required(false),
        )
        .arg(
            Arg::new("da_webserver_url")
                .short('a')
                .long("da_webserver_url")
                .value_name("URL")
                .help("Sets the url of the da webserver")
                .required(false),
        )
        .arg(
            Arg::new("fixed_leader_for_gpuvid")
                .short('f')
                .long("fixed_leader_for_gpuvid")
                .value_name("BOOL")
                .help("Sets the number of fixed leader for gpu vid, only be used when leaders running on gpu")
                .required(false),
        )
        .arg(
            Arg::new("builder")
                .short('b')
                .long("builder")
                .value_name("BUILDER_TYPE")
                .value_parser(value_parser!(BuilderType))
                .help("Sets type of builder. `simple` or `random` to run corresponding integrated builder, `external` to use the one specified by `[config.builder_url]` in config")
                .required(false),
        )
        .get_matches();

    if let Some(config_file_string) = matches.get_one::<String>("config_file") {
        args = ConfigArgs {
            config_file: config_file_string.clone(),
        };
    } else {
        error!("No config file provided, we'll use the default one.");
    }
    let mut config: NetworkConfig<TYPES::SignatureKey> =
        load_config_from_file::<TYPES>(&args.config_file);

    if let Some(total_nodes_string) = matches.get_one::<String>("total_nodes") {
        config.config.num_nodes_with_stake = total_nodes_string.parse::<NonZeroUsize>().unwrap();
        config.config.known_nodes_with_stake =
            vec![PeerConfig::default(); config.config.num_nodes_with_stake.get() as usize];
        error!(
            "config.config.total_nodes: {:?}",
            config.config.num_nodes_with_stake
        );
    }
    if let Some(da_committee_size_string) = matches.get_one::<String>("da_committee_size") {
        config.config.da_staked_committee_size = da_committee_size_string.parse::<usize>().unwrap();
    }
    if let Some(fixed_leader_for_gpuvid_string) =
        matches.get_one::<String>("fixed_leader_for_gpuvid")
    {
        config.config.fixed_leader_for_gpuvid =
            fixed_leader_for_gpuvid_string.parse::<usize>().unwrap();
    }
    if let Some(transactions_per_round_string) = matches.get_one::<String>("transactions_per_round")
    {
        config.transactions_per_round = transactions_per_round_string.parse::<usize>().unwrap();
    }
    if let Some(transaction_size_string) = matches.get_one::<String>("transaction_size") {
        config.transaction_size = transaction_size_string.parse::<usize>().unwrap();
    }
    if let Some(rounds_string) = matches.get_one::<String>("rounds") {
        config.rounds = rounds_string.parse::<usize>().unwrap();
    }
    if let Some(commit_sha_string) = matches.get_one::<String>("commit_sha") {
        config.commit_sha = commit_sha_string.to_string();
    }
    if let Some(orchestrator_url_string) = matches.get_one::<String>("orchestrator_url") {
        orchestrator_url = Url::parse(orchestrator_url_string).unwrap();
    }
    if let Some(webserver_url_string) = matches.get_one::<String>("webserver_url") {
        let updated_web_server_config = WebServerConfig {
            url: Url::parse(webserver_url_string).unwrap(),
            wait_between_polls: config.web_server_config.unwrap().wait_between_polls,
        };
        config.web_server_config = Some(updated_web_server_config);
    }
    if let Some(da_webserver_url_string) = matches.get_one::<String>("da_webserver_url") {
        let updated_da_web_server_config = WebServerConfig {
            url: Url::parse(da_webserver_url_string).unwrap(),
            wait_between_polls: config.da_web_server_config.unwrap().wait_between_polls,
        };
        config.da_web_server_config = Some(updated_da_web_server_config);
    }
    if let Some(builder_type) = matches.get_one::<BuilderType>("builder") {
        config.builder = *builder_type;
    }

    (config, orchestrator_url)
}

/// Reads a network configuration from a given filepath
/// # Panics
/// if unable to convert the config file into toml
/// # Note
/// This derived config is used for initialization of orchestrator,
/// therefore `known_nodes_with_stake` will be an initialized
/// vector full of the node's own config.
/// `my_own_validator_config` will be generated from seed here
/// for loading config from orchestrator,
/// or else it will be loaded from file.
#[must_use]
pub fn load_config_from_file<TYPES: NodeType>(
    config_file: &str,
) -> NetworkConfig<TYPES::SignatureKey> {
    let config_file_as_string: String = fs::read_to_string(config_file)
        .unwrap_or_else(|_| panic!("Could not read config file located at {config_file}"));
    let config_toml: NetworkConfigFile<TYPES::SignatureKey> =
        toml::from_str::<NetworkConfigFile<TYPES::SignatureKey>>(&config_file_as_string)
            .expect("Unable to convert config file to TOML");

    let mut config: NetworkConfig<TYPES::SignatureKey> = config_toml.into();

    // my_own_validator_config would be best to load from file,
    // but its type is too complex to load so we'll generate it from seed now.
    // Also this function is only used for orchestrator initialization now, so this value doesn't matter
    config.config.my_own_validator_config =
        ValidatorConfig::generated_from_seed_indexed(config.seed, config.node_index, 1, true);
    // initialize it with size for better assignment of peers' config
    config.config.known_nodes_with_stake =
        vec![PeerConfig::default(); config.config.num_nodes_with_stake.get() as usize];

    config
}

/// Runs the orchestrator
pub async fn run_orchestrator<
    TYPES: NodeType,
    DACHANNEL: ConnectedNetwork<Message<TYPES>, TYPES::SignatureKey>,
    QUORUMCHANNEL: ConnectedNetwork<Message<TYPES>, TYPES::SignatureKey>,
    NODE: NodeImplementation<TYPES>,
>(
    OrchestratorArgs { url, config }: OrchestratorArgs<TYPES>,
) {
    println!("Starting orchestrator",);
    let _result = hotshot_orchestrator::run_orchestrator::<TYPES::SignatureKey>(config, url).await;
}

/// Helper function to calculate the nuymber of transactions to send per node per round
#[allow(clippy::cast_possible_truncation)]
fn calculate_num_tx_per_round(
    node_index: u64,
    total_num_nodes: usize,
    transactions_per_round: usize,
) -> usize {
    transactions_per_round / total_num_nodes
        + usize::from(
            (total_num_nodes - 1 - node_index as usize)
                < (transactions_per_round % total_num_nodes),
        )
}

/// create a web server network from a config file + public key
/// # Panics
/// Panics if the web server config doesn't exist in `config`
fn webserver_network_from_config<TYPES: NodeType, NetworkVersion: StaticVersionType + 'static>(
    config: NetworkConfig<TYPES::SignatureKey>,
    pub_key: TYPES::SignatureKey,
) -> WebServerNetwork<TYPES, NetworkVersion> {
    // Get the configuration for the web server
    let WebServerConfig {
        url,
        wait_between_polls,
    }: WebServerConfig = config.web_server_config.unwrap();

    WebServerNetwork::create(url, wait_between_polls, pub_key, false)
}

/// Defines the behavior of a "run" of the network with a given configuration
#[async_trait]
pub trait RunDA<
    TYPES: NodeType<InstanceState = TestInstanceState>,
    DANET: ConnectedNetwork<Message<TYPES>, TYPES::SignatureKey>,
    QUORUMNET: ConnectedNetwork<Message<TYPES>, TYPES::SignatureKey>,
    NODE: NodeImplementation<
        TYPES,
        QuorumNetwork = QUORUMNET,
        CommitteeNetwork = DANET,
        Storage = TestStorage<TYPES>,
    >,
> where
    <TYPES as NodeType>::ValidatedState: TestableState<TYPES>,
    <TYPES as NodeType>::BlockPayload: TestableBlock,
    TYPES: NodeType<Transaction = TestTransaction>,
    Leaf<TYPES>: TestableLeaf,
    Self: Sync,
{
    /// Initializes networking, returns self
    async fn initialize_networking(
        config: NetworkConfig<TYPES::SignatureKey>,
        libp2p_advertise_address: Option<SocketAddr>,
    ) -> Self;

    /// Initializes the genesis state and HotShot instance; does not start HotShot consensus
    /// # Panics if it cannot generate a genesis block, fails to initialize HotShot, or cannot
    /// get the anchored view
    /// Note: sequencing leaf does not have state, so does not return state
    async fn initialize_state_and_hotshot(&self) -> SystemContextHandle<TYPES, NODE> {
        let initializer = hotshot::HotShotInitializer::<TYPES>::from_genesis(TestInstanceState {})
            .expect("Couldn't generate genesis block");

        let config = self.get_config();

        // Get KeyPair for certificate Aggregation
        let pk = config.config.my_own_validator_config.public_key.clone();
        let sk = config.config.my_own_validator_config.private_key.clone();
        let known_nodes_with_stake = config.config.known_nodes_with_stake.clone();

        let da_network = self.get_da_channel();
        let quorum_network = self.get_quorum_channel();

        let networks_bundle = Networks {
            quorum_network: quorum_network.clone().into(),
            da_network: da_network.clone().into(),
            _pd: PhantomData,
        };

        let memberships = Memberships {
            quorum_membership: <TYPES as NodeType>::Membership::create_election(
                known_nodes_with_stake.clone(),
                known_nodes_with_stake.clone(),
                config.config.fixed_leader_for_gpuvid,
            ),
            da_membership: <TYPES as NodeType>::Membership::create_election(
                known_nodes_with_stake.clone(),
                config.config.known_da_nodes.clone().into_iter().collect(),
                config.config.fixed_leader_for_gpuvid,
            ),
            vid_membership: <TYPES as NodeType>::Membership::create_election(
                known_nodes_with_stake.clone(),
                known_nodes_with_stake.clone(),
                config.config.fixed_leader_for_gpuvid,
            ),
            view_sync_membership: <TYPES as NodeType>::Membership::create_election(
                known_nodes_with_stake.clone(),
                known_nodes_with_stake.clone(),
                config.config.fixed_leader_for_gpuvid,
            ),
        };

        SystemContext::init(
            pk,
            sk,
            config.node_index,
            config.config,
            memberships,
            networks_bundle,
            initializer,
            ConsensusMetricsValue::default(),
            TestStorage::<TYPES>::default(),
        )
        .await
        .expect("Could not init hotshot")
        .0
    }

    /// Starts HotShot consensus, returns when consensus has finished
    #[allow(clippy::too_many_lines)]
    async fn run_hotshot(
        &self,
        context: SystemContextHandle<TYPES, NODE>,
        transactions: &mut Vec<TestTransaction>,
        transactions_to_send_per_round: u64,
        transaction_size_in_bytes: u64,
    ) -> BenchResults {
        let NetworkConfig {
            rounds,
            node_index,
            start_delay_seconds,
            ..
        } = self.get_config();

        let mut total_transactions_committed = 0;
        let mut total_transactions_sent = 0;
        let mut minimum_latency = 1000;
        let mut maximum_latency = 0;
        let mut total_latency = 0;
        let mut num_latency = 0;

        info!("Sleeping for {start_delay_seconds} seconds before starting hotshot!");
        async_sleep(Duration::from_secs(start_delay_seconds)).await;

        info!("Starting HotShot example!");
        let start = Instant::now();

        let mut event_stream = context.get_event_stream();
        let mut anchor_view: TYPES::Time = <TYPES::Time as ConsensusTime>::genesis();
        let mut num_successful_commits = 0;
        let mut failed_num_views = 0;

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
                            let current_timestamp = Utc::now().timestamp();
                            // this might be a obob
                            if let Some(leaf_info) = leaf_chain.first() {
                                let leaf = &leaf_info.leaf;
                                info!("Decide event for leaf: {}", *leaf.get_view_number());

                                // iterate all the decided transactions to calculate latency
                                if let Some(block_payload) = &leaf.get_block_payload() {
                                    for tx in block_payload
                                        .get_transactions(leaf.get_block_header().metadata())
                                    {
                                        let restored_timestamp_vec =
                                            tx.0[tx.0.len() - 8..].to_vec();
                                        let restored_timestamp = i64::from_be_bytes(
                                            restored_timestamp_vec.as_slice().try_into().unwrap(),
                                        );
                                        let cur_latency = current_timestamp - restored_timestamp;
                                        total_latency += cur_latency;
                                        num_latency += 1;
                                        minimum_latency =
                                            std::cmp::min(minimum_latency, cur_latency);
                                        maximum_latency =
                                            std::cmp::max(maximum_latency, cur_latency);
                                    }
                                }

                                let new_anchor = leaf.get_view_number();
                                if new_anchor >= anchor_view {
                                    anchor_view = leaf.get_view_number();
                                }

                                // send transactions
                                for _ in 0..transactions_to_send_per_round {
                                    // append current timestamp to the tx to calc latency
                                    let timestamp = Utc::now().timestamp();
                                    let mut tx = transactions.remove(0).0;
                                    let mut timestamp_vec = timestamp.to_be_bytes().to_vec();
                                    tx.append(&mut timestamp_vec);

                                    () = context
                                        .submit_transaction(TestTransaction(tx))
                                        .await
                                        .unwrap();
                                    total_transactions_sent += 1;
                                }
                            }

                            if let Some(size) = block_size {
                                total_transactions_committed += size;
                            }

                            num_successful_commits += leaf_chain.len();
                            if num_successful_commits >= rounds {
                                break;
                            }

                            if leaf_chain.len() > 1 {
                                warn!("Leaf chain is greater than 1 with len {}", leaf_chain.len());
                            }
                            // when we make progress, submit new events
                        }
                        EventType::ReplicaViewTimeout { view_number } => {
                            failed_num_views += 1;
                            warn!("Timed out as a replicas in view {:?}", view_number);
                        }
                        EventType::ViewTimeout { view_number } => {
                            failed_num_views += 1;
                            warn!("Timed out in view {:?}", view_number);
                        }
                        _ => {} // mostly DA proposal
                    }
                }
            }
        }
        let consensus_lock = context.hotshot.get_consensus();
        let consensus = consensus_lock.read().await;
        let total_num_views = usize::try_from(consensus.locked_view.get_u64()).unwrap();
        // When posting to the orchestrator, note that the total number of views also include un-finalized views.
        println!("[{node_index}]: Total views: {total_num_views}, Failed views: {failed_num_views}, num_successful_commits: {num_successful_commits}");
        // +2 is for uncommitted views
        assert!(total_num_views <= (failed_num_views + num_successful_commits + 2));
        // Output run results
        let total_time_elapsed = start.elapsed(); // in seconds
        println!("[{node_index}]: {rounds} rounds completed in {total_time_elapsed:?} - Total transactions sent: {total_transactions_sent} - Total transactions committed: {total_transactions_committed} - Total commitments: {num_successful_commits}");
        if total_transactions_committed != 0 {
            // extra 8 bytes for timestamp
            let throughput_bytes_per_sec = total_transactions_committed
                * (transaction_size_in_bytes + 8)
                / total_time_elapsed.as_secs();
            let avg_latency_in_sec = total_latency / num_latency;
            println!("[{node_index}]: throughput: {throughput_bytes_per_sec} bytes/sec, avg_latency: {avg_latency_in_sec} sec.");
            BenchResults {
                avg_latency_in_sec,
                num_latency,
                minimum_latency_in_sec: minimum_latency,
                maximum_latency_in_sec: maximum_latency,
                throughput_bytes_per_sec,
                total_transactions_committed,
                transaction_size_in_bytes: transaction_size_in_bytes + 8, // extra 8 bytes for timestamp
                total_time_elapsed_in_sec: total_time_elapsed.as_secs(),
                total_num_views,
                failed_num_views,
            }
        } else {
            // all values with zero
            BenchResults::default()
        }
    }

    /// Returns the da network for this run
    fn get_da_channel(&self) -> DANET;

    /// Returns the quorum network for this run
    fn get_quorum_channel(&self) -> QUORUMNET;

    /// Returns the config for this run
    fn get_config(&self) -> NetworkConfig<TYPES::SignatureKey>;
}

// WEB SERVER

/// Represents a web server-based run
pub struct WebServerDARun<TYPES: NodeType, NetworkVersion: StaticVersionType> {
    /// the network configuration
    config: NetworkConfig<TYPES::SignatureKey>,
    /// quorum channel
    quorum_channel: WebServerNetwork<TYPES, NetworkVersion>,
    /// data availability channel
    da_channel: WebServerNetwork<TYPES, NetworkVersion>,
}

#[async_trait]
impl<
        TYPES: NodeType<
            Transaction = TestTransaction,
            BlockPayload = TestBlockPayload,
            BlockHeader = TestBlockHeader,
            InstanceState = TestInstanceState,
        >,
        NODE: NodeImplementation<
            TYPES,
            QuorumNetwork = WebServerNetwork<TYPES, NetworkVersion>,
            CommitteeNetwork = WebServerNetwork<TYPES, NetworkVersion>,
            Storage = TestStorage<TYPES>,
        >,
        NetworkVersion: StaticVersionType,
    >
    RunDA<
        TYPES,
        WebServerNetwork<TYPES, NetworkVersion>,
        WebServerNetwork<TYPES, NetworkVersion>,
        NODE,
    > for WebServerDARun<TYPES, NetworkVersion>
where
    <TYPES as NodeType>::ValidatedState: TestableState<TYPES>,
    <TYPES as NodeType>::BlockPayload: TestableBlock,
    Leaf<TYPES>: TestableLeaf,
    Self: Sync,
    NetworkVersion: 'static,
{
    async fn initialize_networking(
        config: NetworkConfig<TYPES::SignatureKey>,
        _libp2p_advertise_address: Option<SocketAddr>,
    ) -> WebServerDARun<TYPES, NetworkVersion> {
        // Get our own key
        let pub_key = config.config.my_own_validator_config.public_key.clone();

        // extract values from config (for DA network)
        let WebServerConfig {
            url,
            wait_between_polls,
        }: WebServerConfig = config.clone().da_web_server_config.unwrap();

        // create and wait for underlying network
        let underlying_quorum_network =
            webserver_network_from_config::<TYPES, NetworkVersion>(config.clone(), pub_key.clone());

        underlying_quorum_network.wait_for_ready().await;

        let da_channel: WebServerNetwork<TYPES, NetworkVersion> =
            WebServerNetwork::create(url.clone(), wait_between_polls, pub_key.clone(), true);

        WebServerDARun {
            config,
            quorum_channel: underlying_quorum_network,
            da_channel,
        }
    }

    fn get_da_channel(&self) -> WebServerNetwork<TYPES, NetworkVersion> {
        self.da_channel.clone()
    }

    fn get_quorum_channel(&self) -> WebServerNetwork<TYPES, NetworkVersion> {
        self.quorum_channel.clone()
    }

    fn get_config(&self) -> NetworkConfig<TYPES::SignatureKey> {
        self.config.clone()
    }
}

// Push CDN

/// Represents a Push CDN-based run
pub struct PushCdnDaRun<TYPES: NodeType> {
    /// The underlying configuration
    config: NetworkConfig<TYPES::SignatureKey>,
    /// The quorum channel
    quorum_channel: PushCdnNetwork<TYPES>,
    /// The DA channel
    da_channel: PushCdnNetwork<TYPES>,
}

#[async_trait]
impl<
        TYPES: NodeType<
            Transaction = TestTransaction,
            BlockPayload = TestBlockPayload,
            BlockHeader = TestBlockHeader,
            InstanceState = TestInstanceState,
        >,
        NODE: NodeImplementation<
            TYPES,
            QuorumNetwork = PushCdnNetwork<TYPES>,
            CommitteeNetwork = PushCdnNetwork<TYPES>,
            Storage = TestStorage<TYPES>,
        >,
    > RunDA<TYPES, PushCdnNetwork<TYPES>, PushCdnNetwork<TYPES>, NODE> for PushCdnDaRun<TYPES>
where
    <TYPES as NodeType>::ValidatedState: TestableState<TYPES>,
    <TYPES as NodeType>::BlockPayload: TestableBlock,
    Leaf<TYPES>: TestableLeaf,
    Self: Sync,
{
    async fn initialize_networking(
        config: NetworkConfig<TYPES::SignatureKey>,
        _libp2p_advertise_address: Option<SocketAddr>,
    ) -> PushCdnDaRun<TYPES> {
        // Get our own key
        let key = config.config.my_own_validator_config.clone();

        // Convert to the Push-CDN-compatible type
        let keypair = KeyPair {
            public_key: WrappedSignatureKey(key.public_key),
            private_key: key.private_key,
        };

        // See if we should be DA, subscribe to the DA topic if so
        let mut topics = vec![Topic::Global];
        if config.config.my_own_validator_config.is_da {
            topics.push(Topic::DA);
        }

        // Create the network and await the initial connection
        let network = PushCdnNetwork::new(
            config
                .cdn_marshal_address
                .clone()
                .expect("`cdn_marshal_address` needs to be supplied for a push CDN run"),
            topics.iter().map(ToString::to_string).collect(),
            keypair,
        )
        .expect("failed to create network");

        // Wait for the network to be ready
        network.wait_for_ready().await;

        PushCdnDaRun {
            config,
            quorum_channel: network.clone(),
            da_channel: network,
        }
    }

    fn get_da_channel(&self) -> PushCdnNetwork<TYPES> {
        self.da_channel.clone()
    }

    fn get_quorum_channel(&self) -> PushCdnNetwork<TYPES> {
        self.quorum_channel.clone()
    }

    fn get_config(&self) -> NetworkConfig<TYPES::SignatureKey> {
        self.config.clone()
    }
}

// Libp2p

/// Represents a libp2p-based run
pub struct Libp2pDARun<TYPES: NodeType> {
    /// the network configuration
    config: NetworkConfig<TYPES::SignatureKey>,
    /// quorum channel
    quorum_channel: Libp2pNetwork<Message<TYPES>, TYPES::SignatureKey>,
    /// data availability channel
    da_channel: Libp2pNetwork<Message<TYPES>, TYPES::SignatureKey>,
}

#[async_trait]
impl<
        TYPES: NodeType<
            Transaction = TestTransaction,
            BlockPayload = TestBlockPayload,
            BlockHeader = TestBlockHeader,
            InstanceState = TestInstanceState,
        >,
        NODE: NodeImplementation<
            TYPES,
            QuorumNetwork = Libp2pNetwork<Message<TYPES>, TYPES::SignatureKey>,
            CommitteeNetwork = Libp2pNetwork<Message<TYPES>, TYPES::SignatureKey>,
            Storage = TestStorage<TYPES>,
        >,
    >
    RunDA<
        TYPES,
        Libp2pNetwork<Message<TYPES>, TYPES::SignatureKey>,
        Libp2pNetwork<Message<TYPES>, TYPES::SignatureKey>,
        NODE,
    > for Libp2pDARun<TYPES>
where
    <TYPES as NodeType>::ValidatedState: TestableState<TYPES>,
    <TYPES as NodeType>::BlockPayload: TestableBlock,
    Leaf<TYPES>: TestableLeaf,
    Self: Sync,
{
    async fn initialize_networking(
        config: NetworkConfig<TYPES::SignatureKey>,
        libp2p_advertise_address: Option<SocketAddr>,
    ) -> Libp2pDARun<TYPES> {
        // Extrapolate keys for ease of use
        let keys = config.clone().config.my_own_validator_config;
        let public_key = keys.public_key;
        let private_key = keys.private_key;

        // In an example, we can calculate the libp2p bind address as a function
        // of the advertise address.
        let bind_address = if let Some(libp2p_advertise_address) = libp2p_advertise_address {
            // If we have supplied one, use it
            SocketAddr::new(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                libp2p_advertise_address.port(),
            )
        } else {
            // If not, index a base port with our node index
            SocketAddr::new(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                8000 + (u16::try_from(config.node_index)
                    .expect("failed to create advertise address")),
            )
        };

        // Create the Libp2p network
        let libp2p_network = Libp2pNetwork::from_config::<TYPES>(
            config.clone(),
            bind_address,
            &public_key,
            &private_key,
        )
        .await
        .expect("failed to create libp2p network");

        // Wait for the network to be ready
        libp2p_network.wait_for_ready().await;

        Libp2pDARun {
            config,
            quorum_channel: libp2p_network.clone(),
            da_channel: libp2p_network,
        }
    }

    fn get_da_channel(&self) -> Libp2pNetwork<Message<TYPES>, TYPES::SignatureKey> {
        self.da_channel.clone()
    }

    fn get_quorum_channel(&self) -> Libp2pNetwork<Message<TYPES>, TYPES::SignatureKey> {
        self.quorum_channel.clone()
    }

    fn get_config(&self) -> NetworkConfig<TYPES::SignatureKey> {
        self.config.clone()
    }
}

// Combined network

/// Represents a combined-network-based run
pub struct CombinedDARun<TYPES: NodeType> {
    /// the network configuration
    config: NetworkConfig<TYPES::SignatureKey>,
    /// quorum channel
    quorum_channel: CombinedNetworks<TYPES>,
    /// data availability channel
    da_channel: CombinedNetworks<TYPES>,
}

#[async_trait]
impl<
        TYPES: NodeType<
            Transaction = TestTransaction,
            BlockPayload = TestBlockPayload,
            BlockHeader = TestBlockHeader,
            InstanceState = TestInstanceState,
        >,
        NODE: NodeImplementation<
            TYPES,
            QuorumNetwork = CombinedNetworks<TYPES>,
            CommitteeNetwork = CombinedNetworks<TYPES>,
            Storage = TestStorage<TYPES>,
        >,
    > RunDA<TYPES, CombinedNetworks<TYPES>, CombinedNetworks<TYPES>, NODE> for CombinedDARun<TYPES>
where
    <TYPES as NodeType>::ValidatedState: TestableState<TYPES>,
    <TYPES as NodeType>::BlockPayload: TestableBlock,
    Leaf<TYPES>: TestableLeaf,
    Self: Sync,
{
    async fn initialize_networking(
        config: NetworkConfig<TYPES::SignatureKey>,
        libp2p_advertise_address: Option<SocketAddr>,
    ) -> CombinedDARun<TYPES> {
        // Initialize our Libp2p network
        let libp2p_da_run: Libp2pDARun<TYPES> =
            <Libp2pDARun<TYPES> as RunDA<
                TYPES,
                Libp2pNetwork<Message<TYPES>, TYPES::SignatureKey>,
                Libp2pNetwork<Message<TYPES>, TYPES::SignatureKey>,
                Libp2pImpl,
            >>::initialize_networking(config.clone(), libp2p_advertise_address)
            .await;

        // Initialize our CDN network
        let cdn_da_run: PushCdnDaRun<TYPES> =
            <PushCdnDaRun<TYPES> as RunDA<
                TYPES,
                PushCdnNetwork<TYPES>,
                PushCdnNetwork<TYPES>,
                PushCdnImpl,
            >>::initialize_networking(config.clone(), libp2p_advertise_address)
            .await;

        // Create our combined network config
        let CombinedNetworkConfig { delay_duration }: CombinedNetworkConfig = config
            .clone()
            .combined_network_config
            .expect("combined network config not specified");

        // Combine the two communication channels
        let da_channel = CombinedNetworks::new(
            cdn_da_run.da_channel,
            libp2p_da_run.da_channel,
            delay_duration,
        );
        let quorum_channel = CombinedNetworks::new(
            cdn_da_run.quorum_channel,
            libp2p_da_run.quorum_channel,
            delay_duration,
        );

        // Return the run configuration
        CombinedDARun {
            config,
            quorum_channel,
            da_channel,
        }
    }

    fn get_da_channel(&self) -> CombinedNetworks<TYPES> {
        self.da_channel.clone()
    }

    fn get_quorum_channel(&self) -> CombinedNetworks<TYPES> {
        self.quorum_channel.clone()
    }

    fn get_config(&self) -> NetworkConfig<TYPES::SignatureKey> {
        self.config.clone()
    }
}

/// Main entry point for validators
/// # Panics
/// if unable to get the local ip address
pub async fn main_entry_point<
    TYPES: NodeType<
        Transaction = TestTransaction,
        BlockPayload = TestBlockPayload,
        BlockHeader = TestBlockHeader,
        InstanceState = TestInstanceState,
    >,
    DACHANNEL: ConnectedNetwork<Message<TYPES>, TYPES::SignatureKey>,
    QUORUMCHANNEL: ConnectedNetwork<Message<TYPES>, TYPES::SignatureKey>,
    NODE: NodeImplementation<
        TYPES,
        QuorumNetwork = QUORUMCHANNEL,
        CommitteeNetwork = DACHANNEL,
        Storage = TestStorage<TYPES>,
    >,
    RUNDA: RunDA<TYPES, DACHANNEL, QUORUMCHANNEL, NODE>,
>(
    args: ValidatorArgs,
) where
    <TYPES as NodeType>::ValidatedState: TestableState<TYPES>,
    <TYPES as NodeType>::BlockPayload: TestableBlock,
    Leaf<TYPES>: TestableLeaf,
{
    setup_logging();
    setup_backtrace();

    info!("Starting validator");

    let orchestrator_client: OrchestratorClient = OrchestratorClient::new(args.clone());

    // We assume one node will not call this twice to generate two validator_config-s with same identity.
    let my_own_validator_config =
        NetworkConfig::<TYPES::SignatureKey>::generate_init_validator_config(
            &orchestrator_client,
            // This is false for now, we only use it to generate the keypair
            false,
        )
        .await;

    // Derives our Libp2p private key from our private key, and then returns the public key of that key
    let libp2p_public_key =
        derive_libp2p_peer_id::<TYPES::SignatureKey>(&my_own_validator_config.private_key)
            .expect("failed to derive Libp2p keypair");

    // conditionally save/load config from file or orchestrator
    // This is a function that will return correct complete config from orchestrator.
    // It takes in a valid args.network_config_file when loading from file, or valid validator_config when loading from orchestrator, the invalid one will be ignored.
    // It returns the complete config which also includes peer's public key and public config.
    // This function will be taken solely by sequencer right after OrchestratorClient::new,
    // which means the previous `generate_validator_config_when_init` will not be taken by sequencer, it's only for key pair generation for testing in hotshot.
    let (mut run_config, source) = NetworkConfig::<TYPES::SignatureKey>::get_complete_config(
        &orchestrator_client,
        args.clone().network_config_file,
        my_own_validator_config,
        args.advertise_address,
        Some(libp2p_public_key),
        // If `indexed_da` is true: use the node index to determine if we are a DA node.
        true,
    )
    .await
    .expect("failed to get config");

    let builder_task = match run_config.builder {
        BuilderType::External => None,
        BuilderType::Random => {
            let (builder_task, builder_url) =
                <RandomBuilderImplementation as TestBuilderImplementation<TYPES>>::start(
                    run_config.config.num_nodes_with_stake.into(),
                    run_config.random_builder.clone().unwrap_or_default(),
                )
                .await;

            run_config.config.builder_url = builder_url;

            builder_task
        }
        BuilderType::Simple => {
            let (builder_task, builder_url) =
                <SimpleBuilderImplementation as TestBuilderImplementation<TYPES>>::start(
                    run_config.config.num_nodes_with_stake.into(),
                    (),
                )
                .await;

            run_config.config.builder_url = builder_url;

            builder_task
        }
    };

    info!("Initializing networking");
    let run = RUNDA::initialize_networking(run_config.clone(), args.advertise_address).await;
    let hotshot = run.initialize_state_and_hotshot().await;

    if let Some(task) = builder_task {
        task.start(Box::new(hotshot.get_event_stream()));
    }

    // pre-generate transactions
    let NetworkConfig {
        transaction_size,
        rounds,
        transactions_per_round,
        node_index,
        config: HotShotConfig {
            num_nodes_with_stake,
            ..
        },
        ..
    } = run_config;

    let mut txn_rng = StdRng::seed_from_u64(node_index);
    let transactions_to_send_per_round = calculate_num_tx_per_round(
        node_index,
        num_nodes_with_stake.get(),
        transactions_per_round,
    );
    let mut transactions = Vec::new();

    for round in 0..rounds {
        for _ in 0..transactions_to_send_per_round {
            let mut txn = <TYPES::ValidatedState>::create_random_transaction(
                None,
                &mut txn_rng,
                transaction_size as u64,
            );

            // prepend destined view number to transaction
            let view_execute_number: u64 = round as u64 + 4;
            txn.0[0..8].copy_from_slice(&view_execute_number.to_be_bytes());

            transactions.push(txn);
        }
    }
    if let NetworkConfigSource::Orchestrator = source {
        info!("Waiting for the start command from orchestrator");
        orchestrator_client
            .wait_for_all_nodes_ready(run_config.clone().node_index)
            .await;
    }

    info!("Starting HotShot");
    let bench_results = run
        .run_hotshot(
            hotshot,
            &mut transactions,
            transactions_to_send_per_round as u64,
            (transaction_size + 8) as u64, // extra 8 bytes for transaction base, see `create_random_transaction`.
        )
        .await;
    orchestrator_client.post_bench_results(bench_results).await;
}
