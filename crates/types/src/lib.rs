// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

//! Types and Traits for the `HotShot` consensus module
use std::{fmt::Debug, future::Future, num::NonZeroUsize, pin::Pin, time::Duration};

use bincode::Options;
use displaydoc::Display;
use light_client::StateVerKey;
use tracing::error;
use traits::signature_key::SignatureKey;
use url::Url;
use vec1::Vec1;

use crate::utils::bincode_opts;
pub mod bundle;
pub mod consensus;
pub mod constants;
pub mod data;
/// Holds the types and functions for DRB computation.
pub mod drb;
pub mod error;
pub mod event;
/// Holds the configuration file specification for a HotShot node.
pub mod hotshot_config_file;
pub mod light_client;
pub mod message;

/// Holds the network configuration specification for HotShot nodes.
pub mod network;
pub mod qc;
pub mod request_response;
pub mod signature_key;
pub mod simple_certificate;
pub mod simple_vote;
pub mod stake_table;
pub mod traits;

/// Holds the upgrade configuration specification for HotShot nodes.
pub mod upgrade_config;
pub mod utils;
/// Holds the validator configuration specification for HotShot nodes.
pub mod validator_config;
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

#[derive(Clone, Debug, Display)]
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
    /// Whether or not this validator is DA
    pub is_da: bool,
}

impl<KEY: SignatureKey> ValidatorConfig<KEY> {
    /// generate validator config from input seed, index, stake value, and whether it's DA
    #[must_use]
    pub fn generated_from_seed_indexed(
        seed: [u8; 32],
        index: u64,
        stake_value: u64,
        is_da: bool,
    ) -> Self {
        let (public_key, private_key) = KEY::generated_from_seed_indexed(seed, index);
        let state_key_pairs = light_client::StateKeyPair::generate_from_seed_indexed(seed, index);
        Self {
            public_key,
            private_key,
            stake_value,
            state_key_pair: state_key_pairs,
            is_da,
        }
    }

    /// get the public config of the validator
    pub fn public_config(&self) -> PeerConfig<KEY> {
        PeerConfig {
            stake_table_entry: self.public_key.stake_table_entry(self.stake_value),
            state_ver_key: self.state_key_pair.0.ver_key(),
        }
    }
}

impl<KEY: SignatureKey> Default for ValidatorConfig<KEY> {
    fn default() -> Self {
        Self::generated_from_seed_indexed([0u8; 32], 0, 1, true)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Display, PartialEq, Eq, Hash)]
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
        default_validator_config.public_config()
    }
}

/// Holds configuration for a `HotShot`
#[derive(Clone, derive_more::Debug, serde::Serialize, serde::Deserialize)]
#[serde(bound(deserialize = ""))]
pub struct HotShotConfig<KEY: SignatureKey> {
    /// The proportion of nodes required before the orchestrator issues the ready signal,
    /// expressed as (numerator, denominator)
    pub start_threshold: (u64, u64),
    /// Total number of nodes in the network
    // Earlier it was total_nodes
    pub num_nodes_with_stake: NonZeroUsize,
    /// List of known node's public keys and stake value for certificate aggregation, serving as public parameter
    pub known_nodes_with_stake: Vec<PeerConfig<KEY>>,
    /// All public keys known to be DA nodes
    pub known_da_nodes: Vec<PeerConfig<KEY>>,
    /// List of DA committee (staking)nodes for static DA committee
    pub da_staked_committee_size: usize,
    /// Number of fixed leaders for GPU VID, normally it will be 0, it's only used when running GPU VID
    pub fixed_leader_for_gpuvid: usize,
    /// Base duration for next-view timeout, in milliseconds
    pub next_view_timeout: u64,
    /// Duration of view sync round timeouts
    pub view_sync_timeout: Duration,
    /// Number of network bootstrap nodes
    pub num_bootstrap: usize,
    /// The maximum amount of time a leader can wait to get a block from a builder
    pub builder_timeout: Duration,
    /// time to wait until we request data associated with a proposal
    pub data_request_delay: Duration,
    /// Builder API base URL
    pub builder_urls: Vec1<Url>,
    /// View to start proposing an upgrade
    pub start_proposing_view: u64,
    /// View to stop proposing an upgrade. To prevent proposing an upgrade, set stop_proposing_view <= start_proposing_view.
    pub stop_proposing_view: u64,
    /// View to start voting on an upgrade
    pub start_voting_view: u64,
    /// View to stop voting on an upgrade. To prevent voting on an upgrade, set stop_voting_view <= start_voting_view.
    pub stop_voting_view: u64,
    /// Unix time in seconds at which we start proposing an upgrade
    pub start_proposing_time: u64,
    /// Unix time in seconds at which we stop proposing an upgrade. To prevent proposing an upgrade, set stop_proposing_time <= start_proposing_time.
    pub stop_proposing_time: u64,
    /// Unix time in seconds at which we start voting on an upgrade
    pub start_voting_time: u64,
    /// Unix time in seconds at which we stop voting on an upgrade. To prevent voting on an upgrade, set stop_voting_time <= start_voting_time.
    pub stop_voting_time: u64,
    /// Number of blocks in an epoch, zero means there are no epochs
    pub epoch_height: u64,
}

impl<KEY: SignatureKey> HotShotConfig<KEY> {
    /// Update a hotshot config to have a view-based upgrade.
    pub fn set_view_upgrade(&mut self, view: u64) {
        self.start_proposing_view = view;
        self.stop_proposing_view = view + 1;
        self.start_voting_view = view.saturating_sub(1);
        self.stop_voting_view = view + 10;
        self.start_proposing_time = 0;
        self.stop_proposing_time = u64::MAX;
        self.start_voting_time = 0;
        self.stop_voting_time = u64::MAX;
    }
}
