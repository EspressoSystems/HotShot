use hotshot_types::{
    traits::{election::ElectionConfig, signature_key::SignatureKey},
    ExecutionType, HotShotConfig, PeerConfig, ValidatorConfig,
};
use serde_inline_default::serde_inline_default;
use std::{
    env,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    num::NonZeroUsize,
    path::PathBuf,
    time::Duration,
    vec,
};
use std::{fs, path::Path};
use surf_disco::Url;
use thiserror::Error;
use toml;
use tracing::error;

use crate::client::OrchestratorClient;

/// Configuration describing a libp2p node
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Libp2pConfig {
    /// bootstrap nodes (socket, serialized public key)
    pub bootstrap_nodes: Vec<(SocketAddr, Vec<u8>)>,
    /// number of bootstrap nodes
    pub num_bootstrap_nodes: usize,
    /// public ip of this node
    pub public_ip: IpAddr,
    /// port to run libp2p on
    pub base_port: u16,
    /// global index of node (for testing purposes a uid)
    pub node_index: u64,
    /// whether or not to index ports
    pub index_ports: bool,
    /// corresponds to libp2p DHT parameter of the same name for bootstrap nodes
    pub bootstrap_mesh_n_high: usize,
    /// corresponds to libp2p DHT parameter of the same name for bootstrap nodes
    pub bootstrap_mesh_n_low: usize,
    /// corresponds to libp2p DHT parameter of the same name for bootstrap nodes
    pub bootstrap_mesh_outbound_min: usize,
    /// corresponds to libp2p DHT parameter of the same name for bootstrap nodes
    pub bootstrap_mesh_n: usize,
    /// corresponds to libp2p DHT parameter of the same name
    pub mesh_n_high: usize,
    /// corresponds to libp2p DHT parameter of the same name
    pub mesh_n_low: usize,
    /// corresponds to libp2p DHT parameter of the same name
    pub mesh_outbound_min: usize,
    /// corresponds to libp2p DHT parameter of the same name
    pub mesh_n: usize,
    /// timeout before starting the next view
    pub next_view_timeout: u64,
    /// minimum time to wait for a view
    pub propose_min_round_time: Duration,
    /// maximum time to wait for a view
    pub propose_max_round_time: Duration,
    /// time node has been running
    pub online_time: u64,
    /// number of transactions per view
    pub num_txn_per_round: usize,
}

/// configuration serialized into a file
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Libp2pConfigFile {
    /// whether or not to index ports
    pub index_ports: bool,
    /// corresponds to libp2p DHT parameter of the same name for bootstrap nodes
    pub bootstrap_mesh_n_high: usize,
    /// corresponds to libp2p DHT parameter of the same name for bootstrap nodes
    pub bootstrap_mesh_n_low: usize,
    /// corresponds to libp2p DHT parameter of the same name for bootstrap nodes
    pub bootstrap_mesh_outbound_min: usize,
    /// corresponds to libp2p DHT parameter of the same name for bootstrap nodes
    pub bootstrap_mesh_n: usize,
    /// corresponds to libp2p DHT parameter of the same name
    pub mesh_n_high: usize,
    /// corresponds to libp2p DHT parameter of the same name
    pub mesh_n_low: usize,
    /// corresponds to libp2p DHT parameter of the same name
    pub mesh_outbound_min: usize,
    /// corresponds to libp2p DHT parameter of the same name
    pub mesh_n: usize,
    /// time node has been running
    pub online_time: u64,
    /// port to run libp2p on
    pub base_port: u16,
}

/// configuration for a web server
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct WebServerConfig {
    /// the url to run on
    pub url: Url,
    /// the time to wait between polls
    pub wait_between_polls: Duration,
}

/// configuration for combined network
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct CombinedNetworkConfig {
    /// delay duration before sending a message through the secondary network
    pub delay_duration: Duration,
}

/// a network configuration error
#[derive(Error, Debug)]
pub enum NetworkConfigError {
    /// Failed to read NetworkConfig from file
    #[error("Failed to read NetworkConfig from file")]
    ReadFromFileError(std::io::Error),
    /// Failed to deserialize loaded NetworkConfig
    #[error("Failed to deserialize loaded NetworkConfig")]
    DeserializeError(serde_json::Error),
    /// Failed to write NetworkConfig to file
    #[error("Failed to write NetworkConfig to file")]
    WriteToFileError(std::io::Error),
    /// Failed to serialize NetworkConfig
    #[error("Failed to serialize NetworkConfig")]
    SerializeError(serde_json::Error),
    /// Failed to recursively create path to NetworkConfig
    #[error("Failed to recursively create path to NetworkConfig")]
    FailedToCreatePath(std::io::Error),
}

/// a network configuration
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(bound(deserialize = ""))]
pub struct NetworkConfig<KEY: SignatureKey, ELECTIONCONFIG: ElectionConfig> {
    /// number of views to run
    pub rounds: usize,
    /// number of transactions per view
    pub transactions_per_round: usize,
    /// number of bootstrap nodes
    pub num_bootrap: usize,
    /// timeout before starting the next view
    pub next_view_timeout: u64,
    /// timeout before starting next view sync round
    pub view_sync_timeout: Duration,
    /// minimum time to wait for a view
    pub propose_min_round_time: Duration,
    /// maximum time to wait for a view
    pub propose_max_round_time: Duration,
    /// global index of node (for testing purposes a uid)
    pub node_index: u64,
    /// unique seed (for randomness? TODO)
    pub seed: [u8; 32],
    /// size of transactions
    pub transaction_size: usize,
    /// delay before beginning consensus
    pub start_delay_seconds: u64,
    /// name of the key type (for debugging)
    pub key_type_name: String,
    /// election config type (for debugging)
    pub election_config_type_name: String,
    /// the libp2p config
    pub libp2p_config: Option<Libp2pConfig>,
    /// the hotshot config
    pub config: HotShotConfig<KEY, ELECTIONCONFIG>,
    /// the webserver config
    pub web_server_config: Option<WebServerConfig>,
    /// the data availability web server config
    pub da_web_server_config: Option<WebServerConfig>,
    /// The address for the Push CDN's "marshal", A.K.A. load balancer
    pub cdn_marshal_address: Option<String>,
    /// combined network config
    pub combined_network_config: Option<CombinedNetworkConfig>,
    /// the commit this run is based on
    pub commit_sha: String,
}

/// the source of the network config
pub enum NetworkConfigSource {
    /// we source the network configuration from the orchestrator
    Orchestrator,
    /// we source the network configuration from a config file on disk
    File,
}

impl<K: SignatureKey, E: ElectionConfig> NetworkConfig<K, E> {
    /// Asynchronously retrieves a `NetworkConfig` either from a file or from an orchestrator.
    ///
    /// This function takes an `OrchestratorClient`, an identity string, and an optional file path.
    ///
    /// If a file path is provided, the function will first attempt to load the `NetworkConfig` from the file.
    /// If the file does not exist or cannot be read, the function will fall back to retrieving the `NetworkConfig` from the orchestrator.
    /// In this case, if the path to the file does not exist, it will be created.
    /// The retrieved `NetworkConfig` is then saved back to the file for future use.
    ///
    /// If no file path is provided, the function will directly retrieve the `NetworkConfig` from the orchestrator.
    ///
    /// # Arguments
    ///
    /// * `client` - An `OrchestratorClient` used to retrieve the `NetworkConfig` from the orchestrator.
    /// * `identity` - A string representing the identity for which to retrieve the `NetworkConfig`.
    /// * `file` - An optional string representing the path to the file from which to load the `NetworkConfig`.
    ///
    /// # Returns
    ///
    /// This function returns a tuple containing a `NetworkConfig` and a `NetworkConfigSource`. The `NetworkConfigSource` indicates whether the `NetworkConfig` was loaded from a file or retrieved from the orchestrator.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use hotshot_orchestrator::config::NetworkConfig;
    /// # use hotshot_orchestrator::client::OrchestratorClient;
    /// let client = OrchestratorClient::new();
    /// let identity = "my_identity".to_string();
    /// let file = Some("/path/to/my/config".to_string());
    /// let (config, source) = NetworkConfig::from_file_or_orchestrator(client, file);
    /// ```
    pub async fn from_file_or_orchestrator(
        client: &OrchestratorClient,
        file: Option<String>,
    ) -> (NetworkConfig<K, E>, NetworkConfigSource) {
        if let Some(file) = file {
            error!("Retrieving config from the file");
            // if we pass in file, try there first
            match Self::from_file(file.clone()) {
                Ok(config) => (config, NetworkConfigSource::File),
                Err(e) => {
                    // fallback to orchestrator
                    error!("{e}, falling back to orchestrator");

                    let config = client
                        .get_config_without_peer(client.identity.clone())
                        .await;

                    // save to file if we fell back
                    if let Err(e) = config.to_file(file) {
                        error!("{e}");
                    };

                    (config, NetworkConfigSource::File)
                }
            }
        } else {
            error!("Retrieving config from the orchestrator");

            // otherwise just get from orchestrator
            (
                client
                    .get_config_without_peer(client.identity.clone())
                    .await,
                NetworkConfigSource::Orchestrator,
            )
        }
    }

    /// Get a temporary node index for generating a validator config
    pub async fn generate_init_validator_config(client: &OrchestratorClient) -> ValidatorConfig<K> {
        // This cur_node_index is only used for key pair generation, it's not bound with the node,
        // lather the node with the generated key pair will get a new node_index from orchestrator.
        let cur_node_index = client.get_node_index_for_init_validator_config().await;
        ValidatorConfig::generated_from_seed_indexed([0u8; 32], cur_node_index.into(), 1)
    }

    /// Asynchronously retrieves a `NetworkConfig` from an orchestrator.
    /// The retrieved one includes correct `node_index` and peer's public config.
    pub async fn get_complete_config(
        client: &OrchestratorClient,
        my_own_validator_config: ValidatorConfig<K>,
        file: Option<String>,
    ) -> (NetworkConfig<K, E>, NetworkConfigSource) {
        let (mut run_config, source) = Self::from_file_or_orchestrator(client, file).await;
        let node_index = run_config.node_index;

        // Assign my_own_validator_config to the run_config if not loading from file
        match source {
            NetworkConfigSource::Orchestrator => {
                run_config.config.my_own_validator_config = my_own_validator_config;
            }
            NetworkConfigSource::File => {
                // do nothing, my_own_validator_config has already been loaded from file
            }
        }

        // one more round of orchestrator here to get peer's public key/config
        let updated_config: NetworkConfig<K, E> = client
            .post_and_wait_all_public_keys::<K, E>(
                run_config.node_index,
                run_config
                    .config
                    .my_own_validator_config
                    .get_public_config(),
            )
            .await;
        run_config.config.known_nodes_with_stake = updated_config.config.known_nodes_with_stake;

        error!("Retrieved config; our node index is {node_index}.");
        (run_config, source)
    }

    /// Loads a `NetworkConfig` from a file.
    ///
    /// This function takes a file path as a string, reads the file, and then deserializes the contents into a `NetworkConfig`.
    ///
    /// # Arguments
    ///
    /// * `file` - A string representing the path to the file from which to load the `NetworkConfig`.
    ///
    /// # Returns
    ///
    /// This function returns a `Result` that contains a `NetworkConfig` if the file was successfully read and deserialized, or a `NetworkConfigError` if an error occurred.
    ///
    /// # Errors
    ///
    /// This function will return an error if the file cannot be read or if the contents cannot be deserialized into a `NetworkConfig`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use hotshot_orchestrator::config::NetworkConfig;
    /// # use hotshot_types::signature_key::BLSPubKey;
    /// // # use hotshot::traits::election::static_committee::StaticElectionConfig;
    /// let file = "/path/to/my/config".to_string();
    /// // NOTE: broken due to staticelectionconfig not being importable
    /// // cannot import staticelectionconfig from hotshot without creating circular dependency
    /// // making this work probably involves the `types` crate implementing a dummy
    /// // electionconfigtype just ot make this example work
    /// let config = NetworkConfig::<BLSPubKey, StaticElectionConfig>::from_file(file).unwrap();
    /// ```
    pub fn from_file(file: String) -> Result<Self, NetworkConfigError> {
        // read from file
        let data = match fs::read(file) {
            Ok(data) => data,
            Err(e) => {
                return Err(NetworkConfigError::ReadFromFileError(e));
            }
        };

        // deserialize
        match serde_json::from_slice(&data) {
            Ok(data) => Ok(data),
            Err(e) => Err(NetworkConfigError::DeserializeError(e)),
        }
    }

    /// Serializes the `NetworkConfig` and writes it to a file.
    ///
    /// This function takes a file path as a string, serializes the `NetworkConfig` into JSON format using `serde_json` and then writes the serialized data to the file.
    ///
    /// # Arguments
    ///
    /// * `file` - A string representing the path to the file where the `NetworkConfig` should be saved.
    ///
    /// # Returns
    ///
    /// This function returns a `Result` that contains `()` if the `NetworkConfig` was successfully serialized and written to the file, or a `NetworkConfigError` if an error occurred.
    ///
    /// # Errors
    ///
    /// This function will return an error if the `NetworkConfig` cannot be serialized or if the file cannot be written.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use hotshot_orchestrator::config::NetworkConfig;
    /// let file = "/path/to/my/config".to_string();
    /// let config = NetworkConfig::from_file(file);
    /// config.to_file(file).unwrap();
    /// ```
    pub fn to_file(&self, file: String) -> Result<(), NetworkConfigError> {
        // ensure the directory containing the config file exists
        if let Some(dir) = Path::new(&file).parent() {
            if let Err(e) = fs::create_dir_all(dir) {
                return Err(NetworkConfigError::FailedToCreatePath(e));
            }
        }

        // serialize
        let serialized = match serde_json::to_string_pretty(self) {
            Ok(data) => data,
            Err(e) => {
                return Err(NetworkConfigError::SerializeError(e));
            }
        };

        // write to file
        match fs::write(file, serialized) {
            Ok(()) => Ok(()),
            Err(e) => Err(NetworkConfigError::WriteToFileError(e)),
        }
    }
}

impl<K: SignatureKey, E: ElectionConfig> Default for NetworkConfig<K, E> {
    fn default() -> Self {
        Self {
            rounds: ORCHESTRATOR_DEFAULT_NUM_ROUNDS,
            transactions_per_round: ORCHESTRATOR_DEFAULT_TRANSACTIONS_PER_ROUND,
            node_index: 0,
            seed: [0u8; 32],
            transaction_size: ORCHESTRATOR_DEFAULT_TRANSACTION_SIZE,
            libp2p_config: None,
            config: HotShotConfigFile::default().into(),
            start_delay_seconds: 60,
            key_type_name: std::any::type_name::<K>().to_string(),
            election_config_type_name: std::any::type_name::<E>().to_string(),
            web_server_config: None,
            da_web_server_config: None,
            cdn_marshal_address: None,
            combined_network_config: None,
            next_view_timeout: 10,
            view_sync_timeout: Duration::from_secs(2),
            num_bootrap: 5,
            propose_min_round_time: Duration::from_secs(0),
            propose_max_round_time: Duration::from_secs(10),
            commit_sha: String::new(),
        }
    }
}

/// a network config stored in a file
#[serde_inline_default]
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(bound(deserialize = ""))]
pub struct NetworkConfigFile<KEY: SignatureKey> {
    /// number of views to run
    #[serde_inline_default(ORCHESTRATOR_DEFAULT_NUM_ROUNDS)]
    pub rounds: usize,
    /// number of transactions per view
    #[serde_inline_default(ORCHESTRATOR_DEFAULT_TRANSACTIONS_PER_ROUND)]
    pub transactions_per_round: usize,
    /// global index of node (for testing purposes a uid)
    #[serde(default)]
    pub node_index: u64,
    /// unique seed (for randomness? TODO)
    #[serde(default)]
    pub seed: [u8; 32],
    /// size of transactions
    #[serde_inline_default(ORCHESTRATOR_DEFAULT_TRANSACTION_SIZE)]
    pub transaction_size: usize,
    /// delay before beginning consensus
    #[serde_inline_default(ORCHESTRATOR_DEFAULT_START_DELAY_SECONDS)]
    pub start_delay_seconds: u64,
    /// the libp2p config
    #[serde(default)]
    pub libp2p_config: Option<Libp2pConfigFile>,
    /// the hotshot config file
    #[serde(default)]
    pub config: HotShotConfigFile<KEY>,
    /// The address of the Push CDN's "marshal", A.K.A. load balancer
    #[serde(default)]
    pub cdn_marshal_address: Option<String>,
    /// the webserver config
    #[serde(default)]
    pub web_server_config: Option<WebServerConfig>,
    /// the data availability web server config
    #[serde(default)]
    pub da_web_server_config: Option<WebServerConfig>,
    /// combined network config
    #[serde(default)]
    pub combined_network_config: Option<CombinedNetworkConfig>,
}

impl<K: SignatureKey, E: ElectionConfig> From<NetworkConfigFile<K>> for NetworkConfig<K, E> {
    fn from(val: NetworkConfigFile<K>) -> Self {
        NetworkConfig {
            rounds: val.rounds,
            transactions_per_round: val.transactions_per_round,
            node_index: 0,
            num_bootrap: val.config.num_bootstrap,
            next_view_timeout: val.config.next_view_timeout,
            view_sync_timeout: val.config.view_sync_timeout,
            propose_max_round_time: val.config.propose_max_round_time,
            propose_min_round_time: val.config.propose_min_round_time,
            seed: val.seed,
            transaction_size: val.transaction_size,
            libp2p_config: val.libp2p_config.map(|libp2p_config| Libp2pConfig {
                num_bootstrap_nodes: val.config.num_bootstrap,
                index_ports: libp2p_config.index_ports,
                bootstrap_nodes: Vec::new(),
                public_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                base_port: libp2p_config.base_port,
                node_index: 0,
                bootstrap_mesh_n_high: libp2p_config.bootstrap_mesh_n_high,
                bootstrap_mesh_n_low: libp2p_config.bootstrap_mesh_n_low,
                bootstrap_mesh_outbound_min: libp2p_config.bootstrap_mesh_outbound_min,
                bootstrap_mesh_n: libp2p_config.bootstrap_mesh_n,
                mesh_n_high: libp2p_config.mesh_n_high,
                mesh_n_low: libp2p_config.mesh_n_low,
                mesh_outbound_min: libp2p_config.mesh_outbound_min,
                mesh_n: libp2p_config.mesh_n,
                next_view_timeout: val.config.next_view_timeout,
                propose_min_round_time: val.config.propose_min_round_time,
                propose_max_round_time: val.config.propose_max_round_time,
                online_time: libp2p_config.online_time,
                num_txn_per_round: val.transactions_per_round,
            }),
            config: val.config.into(),
            key_type_name: std::any::type_name::<K>().to_string(),
            election_config_type_name: std::any::type_name::<E>().to_string(),
            start_delay_seconds: val.start_delay_seconds,
            cdn_marshal_address: val.cdn_marshal_address,
            web_server_config: val.web_server_config,
            da_web_server_config: val.da_web_server_config,
            combined_network_config: val.combined_network_config,
            commit_sha: String::new(),
        }
    }
}

/// Holds configuration for a `HotShot`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(bound(deserialize = ""))]
pub struct HotShotConfigFile<KEY: SignatureKey> {
    /// Total number of staked nodes in the network
    pub num_nodes_with_stake: NonZeroUsize,
    /// Total number of non-staked nodes in the network
    pub num_nodes_without_stake: usize,
    #[serde(skip)]
    /// My own public key, secret key, stake value
    pub my_own_validator_config: ValidatorConfig<KEY>,
    #[serde(skip)]
    /// The known nodes' public key and stake value
    pub known_nodes_with_stake: Vec<PeerConfig<KEY>>,
    #[serde(skip)]
    /// The known non-staking nodes'
    pub known_nodes_without_stake: Vec<KEY>,
    /// Number of staking committee nodes
    pub staked_committee_nodes: usize,
    /// Number of non-staking committee nodes
    pub non_staked_committee_nodes: usize,
    /// Maximum transactions per block
    pub max_transactions: NonZeroUsize,
    /// Minimum transactions per block
    pub min_transactions: usize,
    /// Base duration for next-view timeout, in milliseconds
    pub next_view_timeout: u64,
    /// Duration for view sync round timeout
    pub view_sync_timeout: Duration,
    /// The exponential backoff ration for the next-view timeout
    pub timeout_ratio: (u64, u64),
    /// The delay a leader inserts before starting pre-commit, in milliseconds
    pub round_start_delay: u64,
    /// Delay after init before starting consensus, in milliseconds
    pub start_delay: u64,
    /// Number of network bootstrap nodes
    pub num_bootstrap: usize,
    /// The minimum amount of time a leader has to wait to start a round
    pub propose_min_round_time: Duration,
    /// The maximum amount of time a leader can wait to start a round
    pub propose_max_round_time: Duration,
    /// Time to wait until we request data associated with a proposal
    pub data_request_delay: Duration,
}

/// Holds configuration for a validator node
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(bound(deserialize = ""))]
pub struct ValidatorConfigFile {
    /// The validator's seed
    pub seed: [u8; 32],
    /// The validator's index, which can be treated as another input to the seed
    pub node_id: u64,
    // The validator's stake, commented for now
    // pub stake_value: u64,
}

impl ValidatorConfigFile {
    /// read the validator config from a file
    /// # Panics
    /// Panics if unable to get the current working directory
    pub fn from_file(dir_str: &str) -> Self {
        let current_working_dir = match env::current_dir() {
            Ok(dir) => dir,
            Err(e) => {
                error!("get_current_working_dir error: {:?}", e);
                PathBuf::from("")
            }
        };
        let filename =
            current_working_dir.into_os_string().into_string().unwrap() + "/../../" + dir_str;
        match fs::read_to_string(filename.clone()) {
            // If successful return the files text as `contents`.
            Ok(contents) => {
                let data: ValidatorConfigFile = match toml::from_str(&contents) {
                    // If successful, return data as `Data` struct.
                    // `d` is a local variable.
                    Ok(d) => d,
                    // Handle the `error` case.
                    Err(e) => {
                        // Write `msg` to `stderr`.
                        error!("Unable to load data from `{}`: {}", filename, e);
                        ValidatorConfigFile::default()
                    }
                };
                data
            }
            // Handle the `error` case.
            Err(e) => {
                // Write `msg` to `stderr`.
                error!("Could not read file `{}`: {}", filename, e);
                ValidatorConfigFile::default()
            }
        }
    }
}

impl<KEY: SignatureKey, E: ElectionConfig> From<HotShotConfigFile<KEY>> for HotShotConfig<KEY, E> {
    fn from(val: HotShotConfigFile<KEY>) -> Self {
        HotShotConfig {
            execution_type: ExecutionType::Continuous,
            num_nodes_with_stake: val.num_nodes_with_stake,
            num_nodes_without_stake: val.num_nodes_without_stake,
            max_transactions: val.max_transactions,
            min_transactions: val.min_transactions,
            known_nodes_with_stake: val.known_nodes_with_stake,
            known_nodes_without_stake: val.known_nodes_without_stake,
            my_own_validator_config: val.my_own_validator_config,
            da_staked_committee_size: val.staked_committee_nodes,
            da_non_staked_committee_size: val.non_staked_committee_nodes,
            next_view_timeout: val.next_view_timeout,
            view_sync_timeout: val.view_sync_timeout,
            timeout_ratio: val.timeout_ratio,
            round_start_delay: val.round_start_delay,
            start_delay: val.start_delay,
            num_bootstrap: val.num_bootstrap,
            propose_min_round_time: val.propose_min_round_time,
            propose_max_round_time: val.propose_max_round_time,
            data_request_delay: val.data_request_delay,
            election_config: None,
        }
    }
}
/// default number of rounds to run
pub const ORCHESTRATOR_DEFAULT_NUM_ROUNDS: usize = 10;
/// default number of transactions per round
pub const ORCHESTRATOR_DEFAULT_TRANSACTIONS_PER_ROUND: usize = 10;
/// default size of transactions
pub const ORCHESTRATOR_DEFAULT_TRANSACTION_SIZE: usize = 100;
/// default delay before beginning consensus
pub const ORCHESTRATOR_DEFAULT_START_DELAY_SECONDS: u64 = 60;

impl<K: SignatureKey> From<ValidatorConfigFile> for ValidatorConfig<K> {
    fn from(val: ValidatorConfigFile) -> Self {
        // here stake_value is set to 1, since we don't input stake_value from ValidatorConfigFile for now
        ValidatorConfig::generated_from_seed_indexed(val.seed, val.node_id, 1)
    }
}
impl<KEY: SignatureKey, E: ElectionConfig> From<ValidatorConfigFile> for HotShotConfig<KEY, E> {
    fn from(value: ValidatorConfigFile) -> Self {
        let mut config: HotShotConfig<KEY, E> = HotShotConfigFile::default().into();
        config.my_own_validator_config = value.into();
        config
    }
}

impl<KEY: SignatureKey> Default for HotShotConfigFile<KEY> {
    fn default() -> Self {
        let gen_known_nodes_with_stake = (0..10)
            .map(|node_id| {
                let cur_validator_config: ValidatorConfig<KEY> =
                    ValidatorConfig::generated_from_seed_indexed([0u8; 32], node_id, 1);
                cur_validator_config.get_public_config()
            })
            .collect();
        Self {
            num_nodes_with_stake: NonZeroUsize::new(10).unwrap(),
            num_nodes_without_stake: 0,
            my_own_validator_config: ValidatorConfig::default(),
            known_nodes_with_stake: gen_known_nodes_with_stake,
            known_nodes_without_stake: vec![],
            staked_committee_nodes: 5,
            non_staked_committee_nodes: 0,
            max_transactions: NonZeroUsize::new(100).unwrap(),
            min_transactions: 1,
            next_view_timeout: 10000,
            view_sync_timeout: Duration::from_millis(1000),
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            num_bootstrap: 5,
            propose_min_round_time: Duration::from_secs(0),
            propose_max_round_time: Duration::from_secs(10),
            data_request_delay: Duration::from_millis(200),
        }
    }
}
