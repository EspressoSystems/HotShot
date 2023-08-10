use rand::SeedableRng;
use std::sync::Arc;

use hotshot::{
    traits::TestableNodeImplementation, HotShotInitializer, HotShotType, SystemContext, ViewRunner,
};
use hotshot_task::{
    event_stream::ChannelStream, global_registry::GlobalRegistry, task::FilterEvent,
    task_launcher::TaskRunner,
};
use hotshot_task_impls::events::SequencingHotShotEvent;
use hotshot_types::traits::election::Membership;
use hotshot_types::traits::node_implementation::ExchangesType;
use hotshot_types::traits::signature_key::SignatureKey;
use hotshot_types::{
    message::Message,
    traits::{
        election::ConsensusExchange,
        metrics::NoMetrics,
        network::CommunicationChannel,
        node_implementation::{NodeType, QuorumCommChannel, QuorumEx},
    },
    HotShotConfig,
};
use nll::nll_todo::nll_todo;

use crate::{test_errors::ConsensusTestError, test_runner::Node};

use super::{
    completion_task::{self, CompletionTask},
    node_ctx::NodeCtx,
    safety_task::SafetyTask,
    test_launcher::TestLauncher,
    txn_task::TxnTask,
};
use hotshot_types::traits::signature_key::bn254::{BN254Priv, BN254Pub};
use jf_primitives::signatures::bls_over_bn254::{KeyPair as QCKeyPair};
use hotshot_primitives::qc::bit_vector::StakeTableEntry;
use rand_chacha::ChaCha20Rng;
use ethereum_types::U256;

/// The runner of a test network
/// spin up and down nodes, execute rounds
pub struct TestRunner<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>
where
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
{
    pub(crate) launcher: TestLauncher<TYPES, I>,
    pub(crate) nodes: Vec<Node<TYPES, I>>,
    pub(crate) next_node_id: u64,
    pub(crate) task_runner: TaskRunner,
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>
    TestRunner<TYPES, I>
where
    SystemContext<TYPES::ConsensusType, TYPES, I>: HotShotType<TYPES, I>,
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
{
    pub(crate) fn new(launcher: TestLauncher<TYPES, I>) -> Self {
        Self {
            nodes: Vec::new(),
            next_node_id: 0,
            launcher,
            task_runner: TaskRunner::default(),
        }
    }

    pub async fn run_test(mut self) -> Result<(), ConsensusTestError>
    where
        SystemContext<TYPES::ConsensusType, TYPES, I>: ViewRunner<TYPES, I>,
        I::Exchanges: ExchangesType<
            TYPES::ConsensusType,
            TYPES,
            I::Leaf,
            Message<TYPES, I>,
            Networks = (
                QuorumCommChannel<TYPES, I>,
                I::ViewSyncCommChannel,
                I::CommitteeCommChannel,
            ),
            ElectionConfigs = (TYPES::ElectionConfigType, I::CommitteeElectionConfig),
        >,
    {
        self.add_nodes(self.launcher.metadata.start_nodes).await;

        let TestRunner {
            launcher,
            nodes,
            next_node_id,
            mut task_runner,
        } = self;
        let registry = GlobalRegistry::default();
        let test_event_stream = ChannelStream::new();

        let txn_task_state = TxnTask {
            handles: nodes.clone(),
            next_node_idx: Some(0),
        };
        let (id, task) = (launcher.txn_task_generator)(
            txn_task_state,
            registry.clone(),
            test_event_stream.clone(),
        )
        .await;
        task_runner = task_runner.add_task(id, "blah".to_string(), task);

        let completion_task_state = CompletionTask {
            handles: nodes.clone(),
            test_event_stream: test_event_stream.clone(),
        };
        let (id, task) = (launcher.completion_task_generator)(
            completion_task_state,
            registry.clone(),
            test_event_stream.clone(),
        )
        .await;
        task_runner = task_runner.add_task(id, "blah".to_string(), task);

        // self.launcher.txn_task_generator(self.nodes.clone())
        for mut node in nodes.clone() {
            let safety_task_state = SafetyTask {
                ctx: NodeCtx::default(),
            };
            let (stream, stream_id) = node
                .handle
                .get_event_stream_known_impl(FilterEvent::default())
                .await;
            let (id, task) = (launcher.per_node_safety_task_generator)(
                safety_task_state,
                registry.clone(),
                test_event_stream.clone(),
                stream,
            )
            .await;
            task_runner = task_runner.add_task(id, id.to_string(), task);
        }

        // Start hotshot
        for node in nodes {
            node.handle.hotshot.start_consensus().await;
        }

        task_runner.launch().await;

        // TODO turn errors into something sensible
        Ok(())
    }

    pub async fn add_nodes(&mut self, count: usize) -> Vec<u64>
    where
        SystemContext<TYPES::ConsensusType, TYPES, I>: ViewRunner<TYPES, I>,
        I::Exchanges: ExchangesType<
            TYPES::ConsensusType,
            TYPES,
            I::Leaf,
            Message<TYPES, I>,
            Networks = (
                QuorumCommChannel<TYPES, I>,
                I::ViewSyncCommChannel,
                I::CommitteeCommChannel,
            ),
            ElectionConfigs = (TYPES::ElectionConfigType, I::CommitteeElectionConfig),
        >,
    {
        let mut results = vec![];
        for _i in 0..count {
            tracing::error!("running node{}", _i);
            let node_id = self.next_node_id;
            let network_generator = Arc::new((self.launcher.resource_generator.network_generator)(
                node_id,
            ));

            let secondary_network_generator =
                Arc::new((self
                    .launcher
                    .resource_generator
                    .secondary_network_generator)(node_id));

            let quorum_network =
                (self.launcher.resource_generator.quorum_network)(network_generator.clone());
            let committee_network = (self.launcher.resource_generator.committee_network)(
                secondary_network_generator.clone(),
            );
            let view_sync_network =
                (self.launcher.resource_generator.view_sync_network)(network_generator);
            let storage = (self.launcher.resource_generator.storage)(node_id);
            let config = self.launcher.resource_generator.config.clone();
            let initializer =
                HotShotInitializer::<TYPES, I::Leaf>::from_genesis(I::block_genesis()).unwrap();
            let node_id = self
                .add_node_with_config(
                    quorum_network,
                    committee_network,
                    view_sync_network,
                    storage,
                    initializer,
                    config,
                )
                .await;
            results.push(node_id);
        }

        results
    }

    ///
    pub async fn add_node_with_config(
        &mut self,
        quorum_network: QuorumCommChannel<TYPES, I>,
        committee_network: I::CommitteeCommChannel,
        view_sync_network: I::ViewSyncCommChannel,
        storage: I::Storage,
        initializer: HotShotInitializer<TYPES, I::Leaf>,
        config: HotShotConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    ) -> u64
    where
        SystemContext<TYPES::ConsensusType, TYPES, I>: ViewRunner<TYPES, I>,
        I::Exchanges: ExchangesType<
            TYPES::ConsensusType,
            TYPES,
            I::Leaf,
            Message<TYPES, I>,
            Networks = (
                QuorumCommChannel<TYPES, I>,
                I::ViewSyncCommChannel,
                I::CommitteeCommChannel,
            ),
            ElectionConfigs = (TYPES::ElectionConfigType, I::CommitteeElectionConfig),
        >,
    {
        let node_id = self.next_node_id;
        self.next_node_id += 1;

        let known_nodes = config.known_nodes.clone();
        let known_nodes_qc = config.known_nodes_qc.clone();
        // Get KeyPair for certificate Aggregation
        let private_key = TYPES::SignatureKey::generated_from_seed_indexed(
            [0u8; 32],
            node_id,
        ).1;
        let public_key = TYPES::SignatureKey::from_private(&private_key);
        let entry = StakeTableEntry {
            stake_key: public_key.get_internal_pub_key(),
            stake_amount: U256::from(1u8),
        };
        let quorum_election_config = config.election_config.clone().unwrap_or_else(|| {
            <QuorumEx<TYPES,I> as ConsensusExchange<
                TYPES,
                Message<TYPES, I>,
            >>::Membership::default_election_config(config.total_nodes.get() as u64)
        });

        let committee_election_config = I::committee_election_config_generator();

        let exchanges = I::Exchanges::create(
            known_nodes_qc.clone(),
            known_nodes.clone(),
            (
                quorum_election_config,
                committee_election_config(config.da_committee_size as u64),
            ),
            (quorum_network, view_sync_network, committee_network),
            public_key.clone(),
            entry.clone(),
            private_key.clone(),
        );
        let handle = SystemContext::init(
            public_key,
            private_key,
            node_id,
            config,
            storage,
            exchanges,
            initializer,
            NoMetrics::boxed(),
        )
        .await
        .expect("Could not init hotshot");
        self.nodes.push(Node { handle, node_id });
        node_id
    }
}
