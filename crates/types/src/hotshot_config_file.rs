// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::{num::NonZeroUsize, time::Duration, vec};

use surf_disco::Url;
use vec1::Vec1;

use crate::{
    constants::REQUEST_DATA_DELAY, traits::signature_key::SignatureKey,
    upgrade_config::UpgradeConfig, ExecutionType, HotShotConfig, PeerConfig, ValidatorConfig,
};

/// Default builder URL, used as placeholder
fn default_builder_urls() -> Vec1<Url> {
    vec1::vec1![Url::parse("http://0.0.0.0:3311").unwrap()]
}

/// Holds configuration for a `HotShot`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(bound(deserialize = ""))]
pub struct HotShotConfigFile<KEY: SignatureKey> {
    /// The proportion of nodes required before the orchestrator issues the ready signal,
    /// expressed as (numerator, denominator)
    pub start_threshold: (u64, u64),
    /// Total number of staked nodes in the network
    pub num_nodes_with_stake: NonZeroUsize,
    #[serde(skip)]
    /// My own public key, secret key, stake value
    pub my_own_validator_config: ValidatorConfig<KEY>,
    #[serde(skip)]
    /// The known nodes' public key and stake value
    pub known_nodes_with_stake: Vec<PeerConfig<KEY>>,
    #[serde(skip)]
    /// The known DA nodes' public key and stake values
    pub known_da_nodes: Vec<PeerConfig<KEY>>,
    #[serde(skip)]
    /// The known non-staking nodes'
    pub known_nodes_without_stake: Vec<KEY>,
    /// Number of staking DA nodes
    pub staked_da_nodes: usize,
    /// Number of fixed leaders for GPU VID
    pub fixed_leader_for_gpuvid: usize,
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
    /// The maximum amount of time a leader can wait to get a block from a builder
    pub builder_timeout: Duration,
    /// Time to wait until we request data associated with a proposal
    pub data_request_delay: Option<Duration>,
    /// Builder API base URL
    #[serde(default = "default_builder_urls")]
    pub builder_urls: Vec1<Url>,
    /// Upgrade config
    pub upgrade: UpgradeConfig,
}

impl<KEY: SignatureKey> From<HotShotConfigFile<KEY>> for HotShotConfig<KEY> {
    fn from(val: HotShotConfigFile<KEY>) -> Self {
        HotShotConfig {
            execution_type: ExecutionType::Continuous,
            start_threshold: val.start_threshold,
            num_nodes_with_stake: val.num_nodes_with_stake,
            known_da_nodes: val.known_da_nodes,
            known_nodes_with_stake: val.known_nodes_with_stake,
            known_nodes_without_stake: val.known_nodes_without_stake,
            my_own_validator_config: val.my_own_validator_config,
            da_staked_committee_size: val.staked_da_nodes,
            fixed_leader_for_gpuvid: val.fixed_leader_for_gpuvid,
            next_view_timeout: val.next_view_timeout,
            view_sync_timeout: val.view_sync_timeout,
            timeout_ratio: val.timeout_ratio,
            round_start_delay: val.round_start_delay,
            start_delay: val.start_delay,
            num_bootstrap: val.num_bootstrap,
            builder_timeout: val.builder_timeout,
            data_request_delay: val
                .data_request_delay
                .unwrap_or(Duration::from_millis(REQUEST_DATA_DELAY)),
            builder_urls: val.builder_urls,
            start_proposing_view: val.upgrade.start_proposing_view,
            stop_proposing_view: val.upgrade.stop_proposing_view,
            start_voting_view: val.upgrade.start_voting_view,
            stop_voting_view: val.upgrade.stop_voting_view,
            start_proposing_time: val.upgrade.start_proposing_time,
            stop_proposing_time: val.upgrade.stop_proposing_time,
            start_voting_time: val.upgrade.start_voting_time,
            stop_voting_time: val.upgrade.stop_voting_time,
        }
    }
}

impl<KEY: SignatureKey> HotShotConfigFile<KEY> {
    /// Creates a new `HotShotConfigFile` with 5 nodes and 10 DA nodes.
    ///
    /// # Panics
    ///
    /// Cannot panic, but will if `NonZeroUsize` is somehow an error.
    #[must_use]
    pub fn hotshot_config_5_nodes_10_da() -> Self {
        let staked_da_nodes: usize = 5;

        let mut known_da_nodes = Vec::new();

        let gen_known_nodes_with_stake = (0..10)
            .map(|node_id| {
                let mut cur_validator_config: ValidatorConfig<KEY> =
                    ValidatorConfig::generated_from_seed_indexed([0u8; 32], node_id, 1, false);

                if node_id < staked_da_nodes as u64 {
                    known_da_nodes.push(cur_validator_config.public_config());
                    cur_validator_config.is_da = true;
                }

                cur_validator_config.public_config()
            })
            .collect();

        Self {
            num_nodes_with_stake: NonZeroUsize::new(10).unwrap(),
            start_threshold: (1, 1),
            my_own_validator_config: ValidatorConfig::default(),
            known_nodes_with_stake: gen_known_nodes_with_stake,
            known_nodes_without_stake: vec![],
            staked_da_nodes,
            known_da_nodes,
            fixed_leader_for_gpuvid: 1,
            next_view_timeout: 10000,
            view_sync_timeout: Duration::from_millis(1000),
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            num_bootstrap: 5,
            builder_timeout: Duration::from_secs(10),
            data_request_delay: Some(Duration::from_millis(REQUEST_DATA_DELAY)),
            builder_urls: default_builder_urls(),
            upgrade: UpgradeConfig::default(),
        }
    }
}
