use async_compatibility_layer::channel::UnboundedStream;
use futures::future::BoxFuture;
use hotshot::traits::TestableNodeImplementation;
use hotshot_task::{
    event_stream::ChannelStream,
    global_registry::{GlobalRegistry, HotShotTaskId},
    task::HotShotTaskCompleted,
    task_launcher::TaskRunner,
};
use hotshot_types::{event::Event, traits::node_implementation::NodeType, HotShotConfig};

use crate::test_launcher::ResourceGenerators;

use super::{
    completion_task::CompletionTask, safety_task::SafetyTask, test_builder::TestMetadata,
    test_runner::TestRunner, txn_task::TxnTask, GlobalTestEvent,
};

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
    /// overrideable safety task generator function
    pub per_node_safety_task_generator: Box<
        dyn Fn(
            SafetyTask<TYPES, I>,
            GlobalRegistry,
            ChannelStream<GlobalTestEvent>,
            UnboundedStream<Event<TYPES, I::Leaf>>,
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
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>
    TestLauncher<TYPES, I>
{
    // TODO overrides

    pub fn launch(self) -> TestRunner<TYPES, I> {
        TestRunner {
            launcher: self,
            nodes: Vec::new(),
            next_node_id: 0,
            task_runner: TaskRunner::default(),
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
