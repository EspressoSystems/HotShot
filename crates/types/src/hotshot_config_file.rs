// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::time::Duration;

use url::Url;

use crate::{
    constants::REQUEST_DATA_DELAY, traits::signature_key::SignatureKey,
    upgrade_config::UpgradeConfig, HotShotConfig, PeerConfig, ValidatorConfig,
};

/// Default builder URL, used as placeholder
fn default_builder_urls() -> Vec<Url> {
    vec![Url::parse("http://0.0.0.0:3311").unwrap()]
}

/// Contains configuration values for `HotShot`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(bound(deserialize = ""))]
pub struct HotShotConfigFile<KEY: SignatureKey> {
    /// The known nodes' public key and stake values
    #[serde(rename = "known_nodes", alias = "known_nodes_with_stake")]
    pub known_nodes: Vec<PeerConfig<KEY>>,
    /// The known DA nodes' public key and stake values
    pub known_da_nodes: Vec<PeerConfig<KEY>>,
    /// Number of fixed leaders for GPU VID
    pub fixed_leader_for_gpuvid: usize,
    /// The duration for the timeout before the next view, in milliseconds
    pub next_view_timeout: u64,
    /// The duration for view sync round timeout
    pub view_sync_timeout: Duration,
    /// The maximum amount of time a leader can wait to get a block from a builder
    pub builder_timeout: Duration,
    /// Time to wait until we request data associated with a proposal
    pub data_request_delay: Option<Duration>,
    /// Builder API base URL
    #[serde(default = "default_builder_urls")]
    pub builder_urls: Vec<Url>,
    /// Upgrade config
    pub upgrade: UpgradeConfig,
    /// Number of blocks in an epoch, zero means there are no epochs
    pub epoch_height: u64,
}

impl<KEY: SignatureKey> From<HotShotConfigFile<KEY>> for HotShotConfig<KEY> {
    fn from(val: HotShotConfigFile<KEY>) -> Self {
        HotShotConfig {
            known_da_nodes: val.known_da_nodes,
            known_nodes: val.known_nodes,
            fixed_leader_for_gpuvid: val.fixed_leader_for_gpuvid,
            next_view_timeout: val.next_view_timeout,
            view_sync_timeout: val.view_sync_timeout,
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
            epoch_height: val.epoch_height,
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
            known_nodes: gen_known_nodes_with_stake,
            known_da_nodes,
            fixed_leader_for_gpuvid: 1,
            next_view_timeout: 10000,
            view_sync_timeout: Duration::from_millis(1000),
            builder_timeout: Duration::from_secs(10),
            data_request_delay: Some(Duration::from_millis(REQUEST_DATA_DELAY)),
            builder_urls: default_builder_urls(),
            upgrade: UpgradeConfig::default(),
            epoch_height: 0,
        }
    }
}
