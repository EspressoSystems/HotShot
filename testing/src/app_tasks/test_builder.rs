use hotshot::types::SignatureKey;
use hotshot_types::traits::election::{ConsensusExchange, Membership};
use std::num::NonZeroUsize;
use std::time::Duration;

use hotshot_types::message::SequencingMessage;
use crate::test_builder::TimingData;
use crate::test_launcher::ResourceGenerators;
use hotshot::traits::TestableNodeImplementation;
use hotshot_types::message::Message;
use hotshot_types::traits::consensus_type::sequencing_consensus::SequencingConsensus;
use hotshot_types::traits::network::CommunicationChannel;
use hotshot_types::traits::node_implementation::NodeImplementation;
use hotshot_types::traits::node_implementation::SequencingExchangesType;
use hotshot_types::traits::node_implementation::{NodeType, QuorumCommChannel, QuorumEx};
use hotshot_types::{ExecutionType, HotShotConfig};

use super::completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription};
use super::safety_task::SafetyTaskDescription;
use super::test_launcher::TestLauncher;

use super::{
    safety_task::{NodeSafetyPropertiesDescription, OverallSafetyPropertiesDescription},
    txn_task::TxnTaskDescription,
};

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
    /// per-node safety property description
    /// TODO rename this
    pub per_node_safety_properties: NodeSafetyPropertiesDescription,
    // overall safety property description
    pub overall_safety_properties: OverallSafetyPropertiesDescription,
    // txns timing
    pub txn_description: TxnTaskDescription,
    // completion task
    pub completion_task_description: CompletionTaskDescription,
    /// Minimum transactions required for a block
    pub min_transactions: usize,
    /// timing data
    pub timing_data: TimingData,
}

impl Default for TestMetadata {
    /// by default, just a single round
    fn default() -> Self {
        Self {
            timing_data: TimingData::default(),
            min_transactions: 0,
            total_nodes: 5,
            start_nodes: 5,
            // failure_threshold: 10,
            num_bootstrap_nodes: 5,
            da_committee_size: 5,
            per_node_safety_properties: NodeSafetyPropertiesDescription {
                // TODO Update these numbers
                num_failed_views: Some(5),
                num_decide_events: Some(5),
            },
            overall_safety_properties: OverallSafetyPropertiesDescription {},
            // arbitrary, haven't done the math on this
            txn_description: TxnTaskDescription::RoundRobinTimeBased(Duration::from_millis(10)),
            completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
                TimeBasedCompletionTaskDescription {
                    // TODO ED Put a configurable time here - 10 seconds for now
                    duration: Duration::from_millis(4 * 60 * 60 * 1000),
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
            I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>
    {
        let TestMetadata {
            total_nodes,
            num_bootstrap_nodes,
            min_transactions,
            timing_data,
            da_committee_size,
            txn_description,
            completion_task_description,
            per_node_safety_properties,
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
            // TODO this doesn't exist anymore
            execution_type: ExecutionType::Incremental,
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
        let per_node_safety_task_description =
            SafetyTaskDescription::<TYPES, I>::GenProperties(per_node_safety_properties);
        let per_node_safety_task_generator = per_node_safety_task_description.build();
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
            per_node_safety_task_generator,
            completion_task_generator,
        }
        .modify_default_config(mod_config)
    }
}
