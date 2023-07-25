use std::sync::Arc;

use async_compatibility_layer::channel::UnboundedStream;
use futures::future::BoxFuture;
use hotshot::traits::{TestableNodeImplementation, NodeImplementation};
use hotshot_task::{
    event_stream::ChannelStream,
    global_registry::{GlobalRegistry, HotShotTaskId},
    task::HotShotTaskCompleted,
    task_launcher::TaskRunner,
};
use hotshot_types::{event::Event, traits::{node_implementation::{NodeType, QuorumCommChannel, QuorumEx, QuorumNetwork}, network::CommunicationChannel, election::ConsensusExchange}, HotShotConfig, message::Message};

use crate::spinning_task::SpinningTask;

use super::{
    completion_task::CompletionTask, test_builder::TestMetadata,
    test_runner::TestRunner, txn_task::TxnTask, GlobalTestEvent, overall_safety_task::OverallSafetyTask,
};

/// Wrapper for a function that takes a `node_id` and returns an instance of `T`.
pub type Generator<T> = Box<dyn Fn(u64) -> T + 'static>;

/// Wrapper Type for quorum function that takes a `ConnectedNetwork` and returns a `CommunicationChannel`
pub type QuorumNetworkGenerator<TYPES, I, T> =
    Box<dyn Fn(Arc<QuorumNetwork<TYPES, I>>) -> T + 'static>;

/// Wrapper Type for committee function that takes a `ConnectedNetwork` and returns a `CommunicationChannel`
pub type CommitteeNetworkGenerator<N, T> = Box<dyn Fn(Arc<N>) -> T + 'static>;

pub type ViewSyncNetworkGenerator<N, T> = Box<dyn Fn(Arc<N>) -> T + 'static>;

/// generators for resources used by each node
pub struct ResourceGenerators<
    TYPES: NodeType,
    I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>,
>
where
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
{
    /// generate the underlying quorum network used for each node
    pub network_generator: Generator<QuorumNetwork<TYPES, I>>,

    // TODO ED Make this a committee network; is a quorum network for now to get things working
    pub secondary_network_generator: Generator<QuorumNetwork<TYPES, I>>,
    /// generate a new quorum network for each node
    pub quorum_network: QuorumNetworkGenerator<TYPES, I, QuorumCommChannel<TYPES, I>>,
    /// generate a new committee network for each node
    pub committee_network:
        CommitteeNetworkGenerator<QuorumNetwork<TYPES, I>, I::CommitteeCommChannel>,

    /// generate view sync network
    pub view_sync_network:
        ViewSyncNetworkGenerator<QuorumNetwork<TYPES, I>, I::ViewSyncCommChannel>,

    /// generate a new storage for each node
    pub storage: Generator<<I as NodeImplementation<TYPES>>::Storage>,
    /// configuration used to generate each hotshot node
    pub config: HotShotConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
}

/// test launcher
pub struct TestLauncher<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>
{
    /// generator for resources
    pub resource_generator: ResourceGenerators<TYPES, I>,
    /// metadasta used for tasks
    pub metadata: TestMetadata,
    /// overrideable txn task generator function
    pub txn_task_generator: Box<
        dyn FnOnce(
            TxnTask<TYPES, I>,
            GlobalRegistry,
            ChannelStream<GlobalTestEvent>,
        )
            -> BoxFuture<'static, (HotShotTaskId, BoxFuture<'static, HotShotTaskCompleted>)>,
    >,
    /// overrideable timeout task generator function
    pub completion_task_generator: Box<
        dyn FnOnce(
            CompletionTask<TYPES, I>,
            GlobalRegistry,
            ChannelStream<GlobalTestEvent>,
        )
            -> BoxFuture<'static, (HotShotTaskId, BoxFuture<'static, HotShotTaskCompleted>)>,
    >,
    /// overall safety task generator
    pub overall_safety_task_generator: Box<
        dyn FnOnce(
            OverallSafetyTask<TYPES, I>,
            GlobalRegistry,
            ChannelStream<GlobalTestEvent>,
        )
            -> BoxFuture<'static, (HotShotTaskId, BoxFuture<'static, HotShotTaskCompleted>)>,
    >,

    pub spinning_task_generator: Box<
        dyn FnOnce(SpinningTask<TYPES, I>,
                   GlobalRegistry,
                   ChannelStream<GlobalTestEvent>,
        )
            -> BoxFuture<'static, (HotShotTaskId, BoxFuture<'static, HotShotTaskCompleted>)>
    >,


    pub hooks: Vec<Box<
        dyn FnOnce(GlobalRegistry, ChannelStream<GlobalTestEvent>) -> BoxFuture<'static, (HotShotTaskId, BoxFuture<'static, HotShotTaskCompleted>)>
    >>
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>
    TestLauncher<TYPES, I>
{
    /// launch the test
    pub fn launch(self) -> TestRunner<TYPES, I> {
        TestRunner {
            launcher: self,
            nodes: Vec::new(),
            next_node_id: 0,
            task_runner: TaskRunner::default(),
        }
    }

    /// override the safety task generator
    pub fn with_overall_safety_task_generator(self, overall_safety_task_generator: Box<
        dyn FnOnce(
            OverallSafetyTask<TYPES, I>,
            GlobalRegistry,
            ChannelStream<GlobalTestEvent>,
        )
            -> BoxFuture<'static, (HotShotTaskId, BoxFuture<'static, HotShotTaskCompleted>)>,
    >) -> Self {
        Self {
            overall_safety_task_generator,
            ..self
        }

    }

    /// override the safety task generator
    pub fn with_spinning_task_generator(self, spinning_task_generator: Box<
        dyn FnOnce(
            SpinningTask<TYPES, I>,
            GlobalRegistry,
            ChannelStream<GlobalTestEvent>,
        )
            -> BoxFuture<'static, (HotShotTaskId, BoxFuture<'static, HotShotTaskCompleted>)>,
    >) -> Self {
        Self {
            spinning_task_generator,
            ..self
        }

    }

    /// overridde the completion task generator
    pub fn with_completion_task_generator(self, completion_task_generator:
Box<
        dyn FnOnce(
            CompletionTask<TYPES, I>,
            GlobalRegistry,
            ChannelStream<GlobalTestEvent>,
        )
            -> BoxFuture<'static, (HotShotTaskId, BoxFuture<'static, HotShotTaskCompleted>)>,
    >,

    ) -> Self {
        Self {
            completion_task_generator,
            ..self
        }

    }

    /// override the txn task generator
    pub fn with_txn_task_generator(self, txn_task_generator:
Box<
        dyn FnOnce(
            TxnTask<TYPES, I>,
            GlobalRegistry,
            ChannelStream<GlobalTestEvent>,
        )
            -> BoxFuture<'static, (HotShotTaskId, BoxFuture<'static, HotShotTaskCompleted>)>,
    >,
    ) -> Self {
        Self {
            txn_task_generator,
            ..self
        }

    }

    /// override resource generators
    pub fn with_resource_generator(self, resource_generator: ResourceGenerators<TYPES, I>) -> Self{
        Self {
            resource_generator,
            ..self
        }
    }

    /// add a hook
    pub fn add_hook(mut self, hook: Box<
        dyn FnOnce(GlobalRegistry, ChannelStream<GlobalTestEvent>) -> BoxFuture<'static, (HotShotTaskId, BoxFuture<'static, HotShotTaskCompleted>)>
    >) -> Self {
        self.hooks.push(hook);
        self
    }

    /// overwrite hooks with more hooks
    pub fn with_hooks(mut self, hooks: Vec<Box<
        dyn FnOnce(GlobalRegistry, ChannelStream<GlobalTestEvent>) -> BoxFuture<'static, (HotShotTaskId, BoxFuture<'static, HotShotTaskCompleted>)>
    >>) -> Self {
        Self{
            hooks,
            ..self
        }

    }

    /// Modifies the config used when generating nodes with `f`
    pub fn modify_default_config(
        mut self,
        mut f: impl FnMut(&mut HotShotConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>),
    ) -> Self {
        f(&mut self.resource_generator.config);
        self
    }
}
