use hotshot::traits::NetworkReliability;
use hotshot_example_types::storage_types::TestStorage;
use hotshot_orchestrator::config::ValidatorConfigFile;
use hotshot_types::traits::election::Membership;
use std::{num::NonZeroUsize, sync::Arc, time::Duration};

use hotshot::traits::{NodeImplementation, TestableNodeImplementation};

use hotshot_types::{
    traits::node_implementation::NodeType, ExecutionType, HotShotConfig, ValidatorConfig,
};

use super::completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription};
use super::{
    overall_safety_task::OverallSafetyPropertiesDescription, txn_task::TxnTaskDescription,
};
use crate::{
    spinning_task::SpinningTaskDescription,
    test_launcher::{ResourceGenerators, TestLauncher},
    view_sync_task::ViewSyncTaskDescription,
};
use hotshot_example_types::state_types::TestInstanceState;
/// data describing how a round should be timed.
#[derive(Clone, Debug, Copy)]
pub struct TimingData {
    /// Base duration for next-view timeout, in milliseconds
    pub next_view_timeout: u64,
    /// The exponential backoff ration for the next-view timeout
    pub timeout_ratio: (u64, u64),
    /// The delay a leader inserts before starting pre-commit, in milliseconds
    pub round_start_delay: u64,
    /// Delay after init before starting consensus, in milliseconds
    pub start_delay: u64,
    /// The minimum amount of time a leader has to wait to start a round
    pub propose_min_round_time: Duration,
    /// The maximum amount of time a leader can wait to start a round
    pub propose_max_round_time: Duration,
    /// time to wait until we request data associated with a proposal
    pub data_request_delay: Duration,
    /// Delay before sending through the secondary network in CombinedNetworks
    pub secondary_network_delay: Duration,
    /// view sync timeout
    pub view_sync_timeout: Duration,
}

/// metadata describing a test
#[derive(Clone, Debug)]
pub struct TestMetadata {
    /// Total number of staked nodes in the test
    pub num_nodes_with_stake: usize,
    /// Total number of non-staked nodes in the test
    pub num_nodes_without_stake: usize,
    /// nodes available at start
    pub start_nodes: usize,
    /// Whether to skip initializing nodes that will start late, which will catch up later with
    /// `HotShotInitializer::from_reload` in the spinning task.
    pub skip_late: bool,
    /// number of bootstrap nodes (libp2p usage only)
    pub num_bootstrap_nodes: usize,
    /// Size of the staked DA committee for the test
    pub da_staked_committee_size: usize,
    /// Size of the non-staked DA committee for the test
    pub da_non_staked_committee_size: usize,
    /// overall safety property description
    pub overall_safety_properties: OverallSafetyPropertiesDescription,
    /// spinning properties
    pub spinning_properties: SpinningTaskDescription,
    /// txns timing
    pub txn_description: TxnTaskDescription,
    /// completion task
    pub completion_task_description: CompletionTaskDescription,
    /// Minimum transactions required for a block
    pub min_transactions: usize,
    /// timing data
    pub timing_data: TimingData,
    /// unrelabile networking metadata
    pub unreliable_network: Option<Box<dyn NetworkReliability>>,
    /// view sync check task
    pub view_sync_properties: ViewSyncTaskDescription,
}

impl Default for TimingData {
    fn default() -> Self {
        Self {
            next_view_timeout: 4000,
            timeout_ratio: (11, 10),
            round_start_delay: 100,
            start_delay: 100,
            propose_min_round_time: Duration::new(0, 0),
            propose_max_round_time: Duration::from_millis(100),
            data_request_delay: Duration::from_millis(200),
            secondary_network_delay: Duration::from_millis(1000),
            view_sync_timeout: Duration::from_millis(2000),
        }
    }
}

impl TestMetadata {
    /// the default metadata for a stress test
    #[must_use]
    #[allow(clippy::redundant_field_names)]
    pub fn default_stress() -> Self {
        let num_nodes_with_stake = 100;
        let num_nodes_without_stake = 0;

        TestMetadata {
            num_bootstrap_nodes: num_nodes_with_stake,
            num_nodes_with_stake: num_nodes_with_stake,
            num_nodes_without_stake: num_nodes_without_stake,
            start_nodes: num_nodes_with_stake,
            overall_safety_properties: OverallSafetyPropertiesDescription {
                num_successful_views: 50,
                check_leaf: true,
                check_block: true,
                num_failed_views: 15,
                transaction_threshold: 0,
                threshold_calculator: Arc::new(|_active, total| (2 * total / 3 + 1)),
            },
            timing_data: TimingData {
                next_view_timeout: 2000,
                timeout_ratio: (1, 1),
                start_delay: 20000,
                round_start_delay: 25,
                ..TimingData::default()
            },
            view_sync_properties: ViewSyncTaskDescription::Threshold(0, num_nodes_with_stake),
            ..TestMetadata::default()
        }
    }

    /// the default metadata for multiple rounds
    #[must_use]
    #[allow(clippy::redundant_field_names)]
    pub fn default_multiple_rounds() -> TestMetadata {
        let num_nodes_with_stake = 10;
        let num_nodes_without_stake = 0;
        TestMetadata {
            // TODO: remove once we have fixed the DHT timeout issue
            // https://github.com/EspressoSystems/HotShot/issues/2088
            num_bootstrap_nodes: num_nodes_with_stake,
            num_nodes_with_stake: num_nodes_with_stake,
            num_nodes_without_stake: num_nodes_without_stake,
            start_nodes: num_nodes_with_stake,
            overall_safety_properties: OverallSafetyPropertiesDescription {
                num_successful_views: 20,
                check_leaf: true,
                check_block: true,
                num_failed_views: 8,
                transaction_threshold: 0,
                threshold_calculator: Arc::new(|_active, total| (2 * total / 3 + 1)),
            },
            timing_data: TimingData {
                start_delay: 120_000,
                round_start_delay: 25,
                ..TimingData::default()
            },
            view_sync_properties: ViewSyncTaskDescription::Threshold(0, num_nodes_with_stake),
            ..TestMetadata::default()
        }
    }

    /// Default setting with 20 nodes and 8 views of successful views.
    #[must_use]
    #[allow(clippy::redundant_field_names)]
    pub fn default_more_nodes() -> TestMetadata {
        let num_nodes_with_stake = 20;
        let num_nodes_without_stake = 0;
        TestMetadata {
            num_nodes_with_stake: num_nodes_with_stake,
            num_nodes_without_stake: num_nodes_without_stake,
            start_nodes: num_nodes_with_stake,
            num_bootstrap_nodes: num_nodes_with_stake,
            // The first 14 (i.e., 20 - f) nodes are in the DA committee and we may shutdown the
            // remaining 6 (i.e., f) nodes. We could remove this restriction after fixing the
            // following issue.
            // TODO: Update message broadcasting to avoid hanging
            // <https://github.com/EspressoSystems/HotShot/issues/1567>
            da_staked_committee_size: 14,
            completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
                TimeBasedCompletionTaskDescription {
                    // Increase the duration to get the expected number of successful views.
                    duration: Duration::new(340, 0),
                },
            ),
            overall_safety_properties: OverallSafetyPropertiesDescription {
                ..Default::default()
            },
            timing_data: TimingData {
                next_view_timeout: 5000,
                ..TimingData::default()
            },
            view_sync_properties: ViewSyncTaskDescription::Threshold(0, num_nodes_with_stake),
            ..TestMetadata::default()
        }
    }
}

impl Default for TestMetadata {
    /// by default, just a single round
    #[allow(clippy::redundant_field_names)]
    fn default() -> Self {
        let num_nodes_with_stake = 6;
        let num_nodes_without_stake = 0;
        Self {
            timing_data: TimingData::default(),
            min_transactions: 0,
            num_nodes_with_stake: num_nodes_with_stake,
            num_nodes_without_stake: num_nodes_without_stake,
            start_nodes: num_nodes_with_stake,
            skip_late: false,
            num_bootstrap_nodes: num_nodes_with_stake,
            da_staked_committee_size: num_nodes_with_stake,
            da_non_staked_committee_size: num_nodes_without_stake,
            spinning_properties: SpinningTaskDescription {
                node_changes: vec![],
            },
            overall_safety_properties: OverallSafetyPropertiesDescription::default(),
            // arbitrary, haven't done the math on this
            txn_description: TxnTaskDescription::RoundRobinTimeBased(Duration::from_millis(100)),
            completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
                TimeBasedCompletionTaskDescription {
                    // TODO ED Put a configurable time here - 10 seconds for now
                    duration: Duration::from_millis(10000),
                },
            ),
            unreliable_network: None,
            view_sync_properties: ViewSyncTaskDescription::Threshold(0, num_nodes_with_stake),
        }
    }
}

impl TestMetadata {
    /// turn a description of a test (e.g. a [`TestMetadata`]) into
    /// a [`TestLauncher`] that can be used to launch the test.
    /// # Panics
    /// if some of the the configuration values are zero
    #[must_use]
    pub fn gen_launcher<
        TYPES: NodeType<InstanceState = TestInstanceState>,
        I: TestableNodeImplementation<TYPES, CommitteeElectionConfig = TYPES::ElectionConfigType>,
    >(
        self,
        node_id: u64,
    ) -> TestLauncher<TYPES, I>
    where
        I: NodeImplementation<TYPES>,
    {
        let TestMetadata {
            num_nodes_with_stake,
            num_bootstrap_nodes,
            min_transactions,
            timing_data,
            da_staked_committee_size,
            da_non_staked_committee_size,
            unreliable_network,
            ..
        } = self.clone();

        // We assign known_nodes' public key and stake value here rather than read from config file since it's a test.
        let known_nodes_with_stake = (0..num_nodes_with_stake)
            .map(|node_id_| {
                let cur_validator_config: ValidatorConfig<TYPES::SignatureKey> =
                    ValidatorConfig::generated_from_seed_indexed([0u8; 32], node_id_ as u64, 1);
                cur_validator_config.get_public_config()
            })
            .collect();
        // But now to test validator's config, we input the info of my_own_validator from config file when node_id == 0.
        let mut my_own_validator_config =
            ValidatorConfig::generated_from_seed_indexed([0u8; 32], node_id, 1);
        if node_id == 0 {
            my_own_validator_config = ValidatorConfig::from(ValidatorConfigFile::from_file(
                "config/ValidatorConfigFile.toml",
            ));
        }
        // let da_committee_nodes = known_nodes[0..da_committee_size].to_vec();
        let config = HotShotConfig {
            // TODO this doesn't exist anymore
            execution_type: ExecutionType::Incremental,
            num_nodes_with_stake: NonZeroUsize::new(num_nodes_with_stake).unwrap(),
            // Currently making this zero for simplicity
            num_nodes_without_stake: 0,
            num_bootstrap: num_bootstrap_nodes,
            min_transactions,
            max_transactions: NonZeroUsize::new(99999).unwrap(),
            known_nodes_with_stake,
            known_nodes_without_stake: vec![],
            my_own_validator_config,
            da_staked_committee_size,
            da_non_staked_committee_size,
            next_view_timeout: 500,
            view_sync_timeout: Duration::from_millis(250),
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            // TODO do we use these fields??
            propose_min_round_time: Duration::from_millis(0),
            propose_max_round_time: Duration::from_millis(1000),
            data_request_delay: Duration::from_millis(200),
            // TODO what's the difference between this and the second config?
            election_config: Some(TYPES::Membership::default_election_config(
                num_nodes_with_stake as u64,
                0,
            )),
        };
        let TimingData {
            next_view_timeout,
            timeout_ratio,
            round_start_delay,
            start_delay,
            propose_min_round_time,
            propose_max_round_time,
            data_request_delay,
            secondary_network_delay,
            view_sync_timeout,
        } = timing_data;
        let mod_config =
            // TODO this should really be using the timing config struct
            |a: &mut HotShotConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>| {
                a.next_view_timeout = next_view_timeout;
                a.timeout_ratio = timeout_ratio;
                a.round_start_delay = round_start_delay;
                a.start_delay = start_delay;
                a.propose_min_round_time = propose_min_round_time;
                a.propose_max_round_time = propose_max_round_time;
                a.data_request_delay = data_request_delay;
                a.view_sync_timeout = view_sync_timeout;
            };

        TestLauncher {
            resource_generator: ResourceGenerators {
                channel_generator: <I as TestableNodeImplementation<TYPES>>::gen_networks(
                    num_nodes_with_stake,
                    num_bootstrap_nodes,
                    da_staked_committee_size,
                    unreliable_network,
                    secondary_network_delay,
                ),
                storage: Box::new(|_| TestStorage::<TYPES>::default()),
                config,
            },
            metadata: self,
        }
        .modify_default_config(mod_config)
    }
}
