use super::overall_safety_task::OverallSafetyTask;
use super::overall_safety_task::RoundCtx;
use super::{completion_task::CompletionTask, txn_task::TxnTask};
use crate::test_launcher::Networks;
use crate::test_launcher::TestLauncher;
use hotshot::types::SystemContextHandle;
use hotshot::{
    traits::TestableNodeImplementation, HotShotInitializer, HotShotType, SystemContext, ViewRunner,
};
use hotshot_task::{
    event_stream::ChannelStream, global_registry::GlobalRegistry, task_launcher::TaskRunner,
};
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
#[allow(deprecated)]
use rand::SeedableRng;
use tracing::info;

#[derive(Clone)]
pub struct Node<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> {
    pub node_id: u64,
    pub handle: SystemContextHandle<TYPES, I>,
}

/// The runner of a test network
/// spin up and down nodes, execute rounds
pub struct TestRunner<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>
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

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> TestRunner<TYPES, I>
where
    SystemContext<TYPES, I>: HotShotType<TYPES, I>,
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
{
    /// excecute test
    pub async fn run_test(mut self)
    where
        SystemContext<TYPES, I>: ViewRunner<TYPES, I>,
        I::Exchanges: ExchangesType<
            TYPES,
            I::Leaf,
            Message<TYPES, I>,
            ElectionConfigs = (TYPES::ElectionConfigType, I::CommitteeElectionConfig),
        >,
    {
        let spinning_changes = self
            .launcher
            .metadata
            .spinning_properties
            .node_changes
            .clone();
        self.add_nodes(self.launcher.metadata.start_nodes).await;

        let TestRunner {
            launcher,
            nodes,
            next_node_id: _,
            mut task_runner,
        } = self;
        let registry = GlobalRegistry::default();
        let test_event_stream = ChannelStream::new();

        // add transaction task
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
        task_runner =
            task_runner.add_task(id, "Test Transaction Submission Task".to_string(), task);

        // add completion task
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
        task_runner = task_runner.add_task(id, "Completion Task".to_string(), task);

        // add spinning task
        let spinning_task_state = crate::spinning_task::SpinningTask {
            handles: nodes.clone(),
            changes: spinning_changes.into_iter().map(|(_, b)| b).collect(),
        };
        let (id, task) = (launcher.spinning_task_generator)(
            spinning_task_state,
            registry.clone(),
            test_event_stream.clone(),
        )
        .await;
        task_runner = task_runner.add_task(id, "Completion Task".to_string(), task);

        // add safety task
        let overall_safety_task_state = OverallSafetyTask {
            handles: nodes.clone(),
            ctx: RoundCtx::default(),
            test_event_stream: test_event_stream.clone(),
        };
        let (id, task) = (launcher.overall_safety_task_generator)(
            overall_safety_task_state,
            registry.clone(),
            test_event_stream.clone(),
        )
        .await;
        task_runner = task_runner.add_task(id, "Overall Safety Task".to_string(), task);

        // Start hotshot
        // Goes through all nodes, but really only needs to call this on the leader node of the first view
        for node in nodes {
            node.handle.hotshot.start_consensus().await;
        }

        let results = task_runner.launch().await;

        let mut error_list = vec![];
        for (name, result) in results {
            match result {
                hotshot_task::task::HotShotTaskCompleted::ShutDown => {
                    info!("Task {} shut down successfully", name)
                }
                hotshot_task::task::HotShotTaskCompleted::Error(e) => error_list.push((name, e)),
                _ => {
                    panic!("Future impl for task abstraction failed! This should never happen");
                }
            }
        }
        if !error_list.is_empty() {
            panic!("TEST FAILED! Results: {:?}", error_list);
        }
    }

    /// add nodes
    pub async fn add_nodes(&mut self, count: usize) -> Vec<u64>
    where
        SystemContext<TYPES, I>: ViewRunner<TYPES, I>,
        I::Exchanges: ExchangesType<
            TYPES,
            I::Leaf,
            Message<TYPES, I>,
            ElectionConfigs = (TYPES::ElectionConfigType, I::CommitteeElectionConfig),
        >,
    {
        let mut results = vec![];
        for _i in 0..count {
            tracing::error!("running node{}", _i);
            let node_id = self.next_node_id;
            // let network_generator = Arc::new((self.launcher.resource_generator.network_generator)(
            //     node_id,
            // ));
            //
            // // NOTE ED: This creates a secondary network for the committee network.  As of now this always creates a secondary network,
            // // so libp2p tests will not work since they are not configured to have two running at the same time.  If you want to
            // // test libp2p commout out the below lines where noted.
            //
            // // NOTE ED: Comment out this line to run libp2p tests
            // let secondary_network_generator =
            //     Arc::new((self
            //         .launcher
            //         .resource_generator
            //         .secondary_network_generator)(node_id));

            // let quorum_network =
            //     (self.launcher.resource_generator.quorum_network)(network_generator.clone());
            // let committee_network =
            //     (self.launcher.resource_generator.committee_network)(secondary_network_generator);
            // NOTE ED: Switch the below line with the above line to run libp2p tests
            // let committee_network = (self.launcher.generator.committee_network)(network_generator);

            // let view_sync_network =
            //     (self.launcher.resource_generator.view_sync_network)(network_generator);
            let storage = (self.launcher.resource_generator.storage)(node_id);
            let config = self.launcher.resource_generator.config.clone();
            let initializer =
                HotShotInitializer::<TYPES, I::Leaf>::from_genesis(I::block_genesis()).unwrap();
            let node_id = self
                .add_node_with_config(nll::nll_todo::nll_todo(), storage, initializer, config)
                .await;
            results.push(node_id);
        }

        results
    }

    ///
    pub async fn add_node_with_config(
        &mut self,
        networks: Networks<TYPES, I>,
        storage: I::Storage,
        initializer: HotShotInitializer<TYPES, I::Leaf>,
        config: HotShotConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    ) -> u64
    where
        SystemContext<TYPES, I>: ViewRunner<TYPES, I>,
        I::Exchanges: ExchangesType<
            TYPES,
            I::Leaf,
            Message<TYPES, I>,
            ElectionConfigs = (TYPES::ElectionConfigType, I::CommitteeElectionConfig),
        >,
    {
        let node_id = self.next_node_id;
        self.next_node_id += 1;

        let known_nodes = config.known_nodes.clone();
        let private_key = I::generate_test_key(node_id);
        let public_key = TYPES::SignatureKey::from_private(&private_key);
        let ek = jf_primitives::aead::KeyPair::generate(&mut rand_chacha::ChaChaRng::from_seed(
            [0u8; 32],
        ));
        let quorum_election_config = config.election_config.clone().unwrap_or_else(|| {
            <QuorumEx<TYPES,I> as ConsensusExchange<
                TYPES,
                Message<TYPES, I>,
            >>::Membership::default_election_config(config.total_nodes.get() as u64)
        });
        let committee_election_config = I::committee_election_config_generator();
        let exchanges = I::Exchanges::create(
            known_nodes.clone(),
            (
                quorum_election_config,
                committee_election_config(config.da_committee_size as u64),
            ),
            networks,
            public_key.clone(),
            private_key.clone(),
            ek.clone(),
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
