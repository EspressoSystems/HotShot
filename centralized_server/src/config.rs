pub use crate::runs::RoundConfig;

use crate::Run;
use hotshot_types::{ExecutionType, HotShotConfig};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    num::NonZeroUsize,
    time::Duration,
};

pub struct ClientConfig<K> {
    pub run: Run,
    pub config: NetworkConfig<K>,
}

impl<K> Default for ClientConfig<K> {
    fn default() -> Self {
        Self {
            run: Run(0),
            config: NetworkConfig::default(),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Libp2pConfig {
    pub run: Run,
    pub bootstrap_nodes: Vec<(SocketAddr, Vec<u8>)>,
    pub public_ip: IpAddr,
    pub base_port: u16,
    pub node_index: u64,
    pub bootstrap_mesh_n_high: usize,
    pub bootstrap_mesh_n_low: usize,
    pub bootstrap_mesh_outbound_min: usize,
    pub bootstrap_mesh_n: usize,
    pub mesh_n_high: usize,
    pub mesh_n_low: usize,
    pub mesh_outbound_min: usize,
    pub mesh_n: usize,
    pub next_view_timeout: u64,
    pub propose_min_round_time: u64,
    pub propose_max_round_time: u64,
    pub online_time: u64,
    pub num_txn_per_round: u64,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Libp2pConfigFile {
    pub bootstrap_mesh_n_high: usize,
    pub bootstrap_mesh_n_low: usize,
    pub bootstrap_mesh_outbound_min: usize,
    pub bootstrap_mesh_n: usize,
    pub mesh_n_high: usize,
    pub mesh_n_low: usize,
    pub mesh_outbound_min: usize,
    pub mesh_n: usize,
    pub next_view_timeout: u64,
    pub propose_min_round_time: u64,
    pub propose_max_round_time: u64,
    pub online_time: u64,
    pub num_txn_per_round: u64,
    pub base_port: u16,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct NetworkConfig<K> {
    pub rounds: usize,
    pub transactions_per_round: usize,
    pub node_index: u64,
    pub seed: [u8; 32],
    pub padding: usize,
    pub libp2p_config: Option<Libp2pConfig>,
    pub config: HotShotConfig<K>,
}

impl<K> Default for NetworkConfig<K> {
    fn default() -> Self {
        Self {
            rounds: default_rounds(),
            transactions_per_round: default_transactions_per_round(),
            node_index: 0,
            seed: [0u8; 32],
            padding: default_padding(),
            libp2p_config: None,
            config: default_config().into(),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct NetworkConfigFile {
    #[serde(default = "default_rounds")]
    pub rounds: usize,
    #[serde(default = "default_transactions_per_round")]
    pub transactions_per_round: usize,
    #[serde(default)]
    pub node_index: u64,
    #[serde(default)]
    pub seed: [u8; 32],
    #[serde(default = "default_padding")]
    pub padding: usize,
    #[serde(default)]
    pub libp2p_config: Option<Libp2pConfigFile>,
    #[serde(default = "default_config")]
    pub config: HotShotConfigFile,
}

impl<K> From<NetworkConfigFile> for NetworkConfig<K> {
    fn from(val: NetworkConfigFile) -> Self {
        NetworkConfig {
            rounds: val.rounds,
            transactions_per_round: val.transactions_per_round,
            node_index: 0,
            seed: val.seed,
            padding: val.padding,
            libp2p_config: val.libp2p_config.map(|libp2p_config| Libp2pConfig {
                run: Run(0),
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
                next_view_timeout: libp2p_config.next_view_timeout,
                propose_min_round_time: libp2p_config.propose_min_round_time,
                propose_max_round_time: libp2p_config.propose_max_round_time,
                online_time: libp2p_config.online_time,
                num_txn_per_round: libp2p_config.num_txn_per_round,
            }),
            config: val.config.into(),
        }
    }
}

/// Holds configuration for a `HotShot`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HotShotConfigFile {
    /// Total number of nodes in the network
    pub total_nodes: NonZeroUsize,
    /// Nodes required to reach a decision
    pub threshold: NonZeroUsize,
    /// Maximum transactions per block
    pub max_transactions: NonZeroUsize,
    /// Minimum transactions per block
    pub min_transactions: usize,
    /// Base duration for next-view timeout, in milliseconds
    pub next_view_timeout: u64,
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
}

impl<K> From<HotShotConfigFile> for HotShotConfig<K> {
    fn from(val: HotShotConfigFile) -> Self {
        HotShotConfig {
            execution_type: ExecutionType::Continuous,
            total_nodes: val.total_nodes,
            threshold: val.threshold,
            max_transactions: val.max_transactions,
            min_transactions: val.min_transactions,
            known_nodes: Vec::new(),
            next_view_timeout: val.next_view_timeout,
            timeout_ratio: val.timeout_ratio,
            round_start_delay: val.round_start_delay,
            start_delay: val.start_delay,
            num_bootstrap: val.num_bootstrap,
            propose_min_round_time: val.propose_min_round_time,
            propose_max_round_time: val.propose_max_round_time,
        }
    }
}

// This is hacky, blame serde for not having something like `default_value = "10"`

fn default_rounds() -> usize {
    10
}
fn default_transactions_per_round() -> usize {
    10
}
fn default_padding() -> usize {
    100
}
fn default_config() -> HotShotConfigFile {
    HotShotConfigFile {
        total_nodes: NonZeroUsize::new(10).unwrap(),
        threshold: NonZeroUsize::new(7).unwrap(),
        max_transactions: NonZeroUsize::new(100).unwrap(),
        min_transactions: 0,
        next_view_timeout: 10000,
        timeout_ratio: (11, 10),
        round_start_delay: 1,
        start_delay: 1,
        propose_min_round_time: Duration::from_secs(0),
        propose_max_round_time: Duration::from_secs(10),
        num_bootstrap: 7,
    }
}
