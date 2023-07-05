use std::sync::Arc;
use rand::SeedableRng;

use hotshot::{traits::TestableNodeImplementation, HotShotInitializer, SystemContext, HotShotType, ViewRunner};
use hotshot_task::{task_launcher::TaskRunner, global_registry::GlobalRegistry, event_stream::ChannelStream, task::FilterEvent};
use hotshot_types::{traits::{node_implementation::{NodeType, QuorumCommChannel, QuorumEx}, election::ConsensusExchange, metrics::NoMetrics, network::CommunicationChannel}, HotShotConfig, message::Message};
use hotshot_types::traits::election::Membership;
use hotshot_types::traits::signature_key::SignatureKey;
use hotshot_types::traits::node_implementation::ExchangesType;
use nll::nll_todo::nll_todo;

use crate::{test_runner::Node, test_errors::ConsensusTestError};

use super::{test_launcher::TestLauncher, txn_task::TxnTask, completion_task::{self, CompletionTask}, safety_task::SafetyTask, node_ctx::NodeCtx};

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
    pub(crate) task_runner: TaskRunner
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
                Networks = (QuorumCommChannel<TYPES, I>, I::CommitteeCommChannel),
                >,
        {

            self.add_nodes(self.launcher.metadata.start_nodes).await;
            let TestRunner { launcher, nodes, next_node_id, mut task_runner } = self;
            let registry = GlobalRegistry::default();
            let test_event_stream = ChannelStream::new();

            let txn_task_state = TxnTask {
                handles: nodes.clone(),
                next_node_idx: Some(0)
            };
            let (id, task) = (launcher.txn_task_generator)(txn_task_state, registry.clone(), test_event_stream.clone()).await;
            task_runner = task_runner.add_task(id, "blah".to_string(), task);

            let completion_task_state = CompletionTask {
                handles: nodes.clone(),
                test_event_stream: test_event_stream.clone(),
            };
            let (id, task) = (launcher.completion_task_generator)(completion_task_state, registry.clone(), test_event_stream.clone()).await;
            task_runner = task_runner.add_task(id, "blah".to_string(), task);

            // self.launcher.txn_task_generator(self.nodes.clone())
            for mut node in nodes {
                let safety_task_state = SafetyTask {
                    ctx: NodeCtx::default(),
                };
                let (stream, stream_id) = node.handle.get_event_stream_known_impl(FilterEvent::default()).await;
                let (id, task) = (launcher.per_node_safety_task_generator)(
                    safety_task_state,
                    registry.clone(),
                    test_event_stream.clone(),
                    stream
                ).await;
            }

            task_runner.launch().await;
            // TODO turn errors into something sensible
            nll_todo()

        }

        pub async fn add_nodes(&mut self, count: usize) -> Vec<u64>
        where
            SystemContext<TYPES::ConsensusType, TYPES, I>: ViewRunner<TYPES, I>,
            I::Exchanges: ExchangesType<
                TYPES::ConsensusType,
                TYPES,
                I::Leaf,
                Message<TYPES, I>,
                Networks = (QuorumCommChannel<TYPES, I>, I::CommitteeCommChannel),
            >,
        {
            let mut results = vec![];
            for _i in 0..count {
                tracing::error!("running node{}", _i);
                let node_id = self.next_node_id;
                let network_generator = Arc::new((self.launcher.resource_generator.network_generator)(node_id));
                let quorum_network =
                    (self.launcher.resource_generator.quorum_network)(network_generator.clone());
                let committee_network = (self.launcher.resource_generator.committee_network)(network_generator);
                let storage = (self.launcher.resource_generator.storage)(node_id);
                let config = self.launcher.resource_generator.config.clone();
                let initializer =
                    HotShotInitializer::<TYPES, I::Leaf>::from_genesis(I::block_genesis()).unwrap();
                let node_id = self
                    .add_node_with_config(
                        quorum_network,
                        committee_network,
                        storage,
                        initializer,
                        config,
                        )
                    .await;
                results.push(node_id);
            }

            results
        }

        pub async fn add_node_with_config(
            &mut self,
            quorum_network: QuorumCommChannel<TYPES, I>,
            committee_network: I::CommitteeCommChannel,
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
            Networks = (QuorumCommChannel<TYPES, I>, I::CommitteeCommChannel),
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
                    let election_config = config.election_config.clone().unwrap_or_else(|| {
                        <QuorumEx<TYPES,I> as ConsensusExchange<
                            TYPES,
                            Message<TYPES, I>,
                            >>::Membership::default_election_config(config.total_nodes.get() as u64)
                    });
                    let exchanges = I::Exchanges::create(
                        known_nodes.clone(),
                        election_config.clone(),
                        (quorum_network, committee_network),
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


