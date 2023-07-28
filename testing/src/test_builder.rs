use hotshot::types::SignatureKey;
use hotshot_types::traits::consensus_type::sequencing_consensus::SequencingConsensus;
use hotshot_types::traits::election::{ConsensusExchange, Membership};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use hotshot::traits::{NodeImplementation, TestableNodeImplementation};
use hotshot_types::message::{Message, SequencingMessage};
use hotshot_types::traits::network::CommunicationChannel;
use hotshot_types::traits::node_implementation::{
    NodeType, QuorumCommChannel, QuorumEx, SequencingExchangesType,
};
use hotshot_types::{ExecutionType, HotShotConfig};

use crate::spinning_task::SpinningTaskDescription;
use crate::test_launcher::ResourceGenerators;

use super::completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription};
use super::test_launcher::TestLauncher;

use super::{
    overall_safety_task::OverallSafetyPropertiesDescription, txn_task::TxnTaskDescription,
};

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
}

/// metadata describing a test
#[derive(Clone, Debug)]
pub struct TestMetadata {
    /// Total number of nodes in the test
    pub total_nodes: usize,
    /// nodes available at start
    pub start_nodes: usize,
    /// number of bootstrap nodes (libp2p usage only)
    pub num_bootstrap_nodes: usize,
    /// Size of the DA committee for the test.  0 == no DA.
    pub da_committee_size: usize,
    // overall safety property description
    pub overall_safety_properties: OverallSafetyPropertiesDescription,
    /// spinning properties
    pub spinning_properties: SpinningTaskDescription,
    // txns timing
    pub txn_description: TxnTaskDescription,
    // completion task
    pub completion_task_description: CompletionTaskDescription,
    /// Minimum transactions required for a block
    pub min_transactions: usize,
    /// timing data
    pub timing_data: TimingData,
}

impl Default for TimingData {
    fn default() -> Self {
        Self {
            next_view_timeout: 10000,
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            propose_min_round_time: Duration::new(0, 0),
            propose_max_round_time: Duration::new(5, 0),
        }
    }
}

impl TestMetadata {
    pub fn default_stress() -> Self {
        TestMetadata {
            num_bootstrap_nodes: 15,
            total_nodes: 100,
            start_nodes: 100,
            overall_safety_properties: OverallSafetyPropertiesDescription {
                num_successful_views: 50,
                check_leaf: true,
                check_state: true,
                check_block: true,
                num_failed_views: 15,
                threshold_calculator: Arc::new(|_active, total| (2 * total / 3 + 1)),
            },
            timing_data: TimingData {
                next_view_timeout: 2000,
                timeout_ratio: (1, 1),
                start_delay: 20000,
                round_start_delay: 25,
                ..TimingData::default()
            },
            ..TestMetadata::default()
        }
    }

    pub fn default_multiple_rounds() -> TestMetadata {
        TestMetadata {
            total_nodes: 10,
            start_nodes: 10,
            overall_safety_properties: OverallSafetyPropertiesDescription {
                num_successful_views: 20,
                check_leaf: true,
                check_state: true,
                check_block: true,
                num_failed_views: 8,
                threshold_calculator: Arc::new(|_active, total| (2 * total / 3 + 1)),
            },
            timing_data: TimingData {
                start_delay: 120000,
                round_start_delay: 25,
                ..TimingData::default()
            },
            ..TestMetadata::default()
        }
    }
}

impl Default for TestMetadata {
    /// by default, just a single round
    fn default() -> Self {
        Self {
            timing_data: TimingData::default(),
            min_transactions: 0,
            total_nodes: 5,
            start_nodes: 5,
            num_bootstrap_nodes: 5,
            da_committee_size: 5,
            spinning_properties: SpinningTaskDescription {
                node_changes: vec![],
            },
            overall_safety_properties: OverallSafetyPropertiesDescription::default(),
            // arbitrary, haven't done the math on this
            txn_description: TxnTaskDescription::RoundRobinTimeBased(Duration::from_millis(10)),
            completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
                TimeBasedCompletionTaskDescription {
                    // TODO ED Put a configurable time here - 10 seconds for now
                    duration: Duration::from_millis(10000),
                },
            ),
        }
    }
}

impl TestMetadata {
    pub fn gen_launcher<
        TYPES: NodeType,
        I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>,
    >(
        self,
    ) -> TestLauncher<TYPES, I>
    where
        QuorumCommChannel<TYPES, I>: CommunicationChannel<
            TYPES,
            Message<TYPES, I>,
            <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
            <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
            <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
        >,
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        <I as NodeImplementation<TYPES>>::Exchanges:
            SequencingExchangesType<TYPES, Message<TYPES, I>>,
        I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
    {
        let TestMetadata {
            total_nodes,
            num_bootstrap_nodes,
            min_transactions,
            timing_data,
            da_committee_size,
            txn_description,
            completion_task_description,
            overall_safety_properties,
            spinning_properties,
            ..
        } = self.clone();

        let known_nodes: Vec<<TYPES as NodeType>::SignatureKey> = (0..total_nodes)
            .map(|id| {
                let priv_key = I::generate_test_key(id as u64);
                TYPES::SignatureKey::from_private(&priv_key)
            })
            .collect();
        // let da_committee_nodes = known_nodes[0..da_committee_size].to_vec();
        let config = HotShotConfig {
            total_nodes: NonZeroUsize::new(total_nodes).unwrap(),
            num_bootstrap: num_bootstrap_nodes,
            min_transactions,
            max_transactions: NonZeroUsize::new(99999).unwrap(),
            known_nodes,
            da_committee_size,
            next_view_timeout: 500,
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            // TODO do we use these fields??
            propose_min_round_time: Duration::from_millis(0),
            propose_max_round_time: Duration::from_millis(1000),
            // TODO what's the difference between this and the second config?
            election_config: Some(<QuorumEx<TYPES, I> as ConsensusExchange<
                TYPES,
                Message<TYPES, I>,
            >>::Membership::default_election_config(
                total_nodes as u64
            )),
        };
        let network_generator =
            I::network_generator(total_nodes, num_bootstrap_nodes, da_committee_size, false);
        let secondary_network_generator =
            I::network_generator(total_nodes, num_bootstrap_nodes, da_committee_size, true);
        let TimingData {
            next_view_timeout,
            timeout_ratio,
            round_start_delay,
            start_delay,
            propose_min_round_time,
            propose_max_round_time,
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
            };

        let txn_task_generator = txn_description.build();
        let completion_task_generator = completion_task_description.build_and_launch();
        let overall_safety_task_generator = overall_safety_properties.build();
        let spinning_task_generator = spinning_properties.build();
        TestLauncher {
            resource_generator: ResourceGenerators {
                network_generator,
                secondary_network_generator,
                quorum_network: I::quorum_comm_channel_generator(),
                committee_network: I::committee_comm_channel_generator(),
                view_sync_network: I::view_sync_comm_channel_generator(),
                storage: Box::new(|_| I::construct_tmp_storage().unwrap()),
                config,
            },
            metadata: self,
            txn_task_generator,
            overall_safety_task_generator,
            completion_task_generator,
            spinning_task_generator,
            hooks: vec![],
        }
        .modify_default_config(mod_config)
    }
}
