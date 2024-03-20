//! Types and Traits for the `HotShot` consensus module
use crate::utils::bincode_opts;
use bincode::Options;
use displaydoc::Display;
use light_client::StateVerKey;
use std::fmt::Debug;
use std::{future::Future, num::NonZeroUsize, pin::Pin, time::Duration};
use tracing::error;
use traits::{election::ElectionConfig, signature_key::SignatureKey};
pub mod consensus;
pub mod constants;
pub mod data;
pub mod error;
pub mod event;
pub mod light_client;
pub mod message;
pub mod qc;
pub mod signature_key;
pub mod simple_certificate;
pub mod simple_vote;
pub mod stake_table;
pub mod traits;
pub mod utils;
pub mod vid;
pub mod vote;

/// Pinned future that is Send and Sync
pub type BoxSyncFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + Sync + 'a>>;

/// yoinked from futures crate
pub fn assert_future<T, F>(future: F) -> F
where
    F: Future<Output = T>,
{
    future
}
/// yoinked from futures crate, adds sync bound that we need
pub fn boxed_sync<'a, F>(fut: F) -> BoxSyncFuture<'a, F::Output>
where
    F: Future + Sized + Send + Sync + 'a,
{
    assert_future::<F::Output, _>(Box::pin(fut))
}
/// the type of consensus to run. Either:
/// wait for a signal to start a view,
/// or constantly run
/// you almost always want continuous
/// incremental is just for testing
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum ExecutionType {
    /// constantly increment view as soon as view finishes
    Continuous,
    /// wait for a signal
    Incremental,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Display)]
#[serde(bound(deserialize = ""))]
/// config for validator, including public key, private key, stake value
pub struct ValidatorConfig<KEY: SignatureKey> {
    /// The validator's public key and stake value
    pub public_key: KEY,
    /// The validator's private key, should be in the mempool, not public
    pub private_key: KEY::PrivateKey,
    /// The validator's stake
    pub stake_value: u64,
    /// the validator's key pairs for state signing/verification
    pub state_key_pair: light_client::StateKeyPair,
}

impl<KEY: SignatureKey> ValidatorConfig<KEY> {
    /// generate validator config from input seed, index and stake value
    #[must_use]
    pub fn generated_from_seed_indexed(seed: [u8; 32], index: u64, stake_value: u64) -> Self {
        let (public_key, private_key) = KEY::generated_from_seed_indexed(seed, index);
        let state_key_pairs = light_client::StateKeyPair::generate_from_seed_indexed(seed, index);
        Self {
            public_key,
            private_key,
            stake_value,
            state_key_pair: state_key_pairs,
        }
    }

    /// get the public config of the validator
    pub fn get_public_config(&self) -> PeerConfig<KEY> {
        PeerConfig {
            stake_table_entry: self.public_key.get_stake_table_entry(self.stake_value),
            state_ver_key: self.state_key_pair.0.ver_key(),
        }
    }
}

impl<KEY: SignatureKey> Default for ValidatorConfig<KEY> {
    fn default() -> Self {
        Self::generated_from_seed_indexed([0u8; 32], 0, 1)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Display)]
#[serde(bound(deserialize = ""))]
/// structure of peers' config, including public key, stake value, and state key.
pub struct PeerConfig<KEY: SignatureKey> {
    /// The peer's public key and stake value
    pub stake_table_entry: KEY::StakeTableEntry,
    /// the peer's state public key
    pub state_ver_key: StateVerKey,
}

impl<KEY: SignatureKey> PeerConfig<KEY> {
    /// Serialize a peer's config to bytes
    pub fn to_bytes(config: &Self) -> Vec<u8> {
        let x = bincode_opts().serialize(config);
        match x {
            Ok(x) => x,
            Err(e) => {
                error!(?e, "Failed to serialize public key");
                vec![]
            }
        }
    }

    /// Deserialize a peer's config from bytes
    /// # Errors
    /// Will return `None` if deserialization fails
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let x: Result<PeerConfig<KEY>, _> = bincode_opts().deserialize(bytes);
        match x {
            Ok(pub_key) => Some(pub_key),
            Err(e) => {
                error!(?e, "Failed to deserialize public key");
                None
            }
        }
    }
}

impl<KEY: SignatureKey> Default for PeerConfig<KEY> {
    fn default() -> Self {
        let default_validator_config = ValidatorConfig::<KEY>::default();
        default_validator_config.get_public_config()
    }
}

/// Holds configuration for a `HotShot`
#[derive(Clone, custom_debug::Debug, serde::Serialize, serde::Deserialize)]
#[serde(bound(deserialize = ""))]
pub struct HotShotConfig<KEY: SignatureKey, ELECTIONCONFIG: ElectionConfig> {
    /// Whether to run one view or continuous views
    pub execution_type: ExecutionType,
    /// Total number of nodes in the network
    // Earlier it was total_nodes
    pub num_nodes_with_stake: NonZeroUsize,
    /// Number of nodes without stake
    pub num_nodes_without_stake: usize,
    /// Minimum transactions per block
    pub min_transactions: usize,
    /// Maximum transactions per block
    pub max_transactions: NonZeroUsize,
    /// List of known node's public keys and stake value for certificate aggregation, serving as public parameter
    pub known_nodes_with_stake: Vec<PeerConfig<KEY>>,
    /// List of known non-staking nodes' public keys
    pub known_nodes_without_stake: Vec<KEY>,
    /// My own validator config, including my public key, private key, stake value, serving as private parameter
    pub my_own_validator_config: ValidatorConfig<KEY>,
    /// List of DA committee (staking)nodes for static DA committe
    pub da_staked_committee_size: usize,
    /// List of DA committee nodes (non-staking)nodes for static DA committe
    pub da_non_staked_committee_size: usize,
    /// Base duration for next-view timeout, in milliseconds
    pub next_view_timeout: u64,
    /// Duration of view sync round timeouts
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
    /// time to wait until we request data associated with a proposal
    pub data_request_delay: Duration,
    /// the election configuration
    pub election_config: Option<ELECTIONCONFIG>,
}
