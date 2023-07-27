use std::{sync::Arc, time::Duration};

use async_compatibility_layer::art::async_sleep;
use hotshot::traits::TestableNodeImplementation;
use hotshot_task::{
    boxed_sync,
    event_stream::{ChannelStream, EventStream},
    global_registry::GlobalRegistry,
    task::{
        FilterEvent, HandleEvent, HandleMessage, HotShotTaskCompleted, HotShotTaskTypes, TaskErr,
        TS,
    },
    task_impls::{HSTWithEventAndMessage, TaskBuilder},
    GeneratedStream,
};
use hotshot_types::traits::node_implementation::NodeType;
use snafu::Snafu;

use crate::test_runner::Node;

use super::{test_launcher::TaskFuture, GlobalTestEvent};

/// the idea here is to run as long as we want

/// Data Availability task error
#[derive(Snafu, Debug)]
pub struct CompletionTaskErr {}
impl TaskErr for CompletionTaskErr {}

/// Data availability task state
pub struct CompletionTask<
    TYPES: NodeType,
    I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>,
> {
    pub(crate) test_event_stream: ChannelStream<GlobalTestEvent>,
    pub(crate) handles: Vec<Node<TYPES, I>>,
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> TS
    for CompletionTask<TYPES, I>
{
}

/// Completion task types
pub type CompletionTaskTypes<TYPES, I> = HSTWithEventAndMessage<
    CompletionTaskErr,
    GlobalTestEvent,
    ChannelStream<GlobalTestEvent>,
    (),
    GeneratedStream<()>,
    CompletionTask<TYPES, I>,
>;

// TODO this is broken. Need to communicate to handles to kill everything
/// Description for a time-based completion task.
#[derive(Clone, Debug)]
pub struct TimeBasedCompletionTaskDescription {
    /// Duration of the task.
    pub duration: Duration,
}

/// Description for a completion task.
#[derive(Clone, Debug)]
pub enum CompletionTaskDescription {
    /// Time-based completion task.
    TimeBasedCompletionTaskBuilder(TimeBasedCompletionTaskDescription),
}

impl CompletionTaskDescription {
    /// Build and launch a completion task.
    pub fn build_and_launch<
        TYPES: NodeType,
        I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>,
    >(
        self,
    ) -> Box<
        dyn FnOnce(
            CompletionTask<TYPES, I>,
            GlobalRegistry,
            ChannelStream<GlobalTestEvent>,
        ) -> TaskFuture,
    > {
        match self {
            CompletionTaskDescription::TimeBasedCompletionTaskBuilder(td) => td.build_and_launch(),
        }
    }
}

impl TimeBasedCompletionTaskDescription {
    /// create the task and launch it
    pub fn build_and_launch<
        TYPES: NodeType,
        I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>,
    >(
        self,
    ) -> Box<
        dyn FnOnce(
            CompletionTask<TYPES, I>,
            GlobalRegistry,
            ChannelStream<GlobalTestEvent>,
        ) -> TaskFuture,
    > {
        Box::new(move |state, mut registry, test_event_stream| {
            async move {
                // TODO we'll possibly want multiple criterion including:
                // - certain number of txns committed
                // - anchor of certain depth
                // - some other stuff? probably?
                let event_handler =
                    HandleEvent::<CompletionTaskTypes<TYPES, I>>(Arc::new(move |event, state| {
                        async move {
                            match event {
                                GlobalTestEvent::ShutDown => {
                                    (Some(HotShotTaskCompleted::ShutDown), state)
                                }
                            }
                        }
                        .boxed()
                    }));
                let message_handler =
                    HandleMessage::<CompletionTaskTypes<TYPES, I>>(Arc::new(move |_, state| {
                        async move {
                            state
                                .test_event_stream
                                .publish(GlobalTestEvent::ShutDown)
                                .await;
                            for node in &state.handles {
                                node.handle.clone().shut_down().await;
                            }
                            (Some(HotShotTaskCompleted::ShutDown), state)
                        }
                        .boxed()
                    }));
                // normally I'd say "let's use Interval from async-std!"
                // but doing this is easier than unifying async-std with tokio's slightly different
                // interval abstraction
                let stream_generator = GeneratedStream::new(Arc::new(move || {
                    let fut = async move {
                        async_sleep(self.duration).await;
                    };
                    boxed_sync(fut)
                }));
                let builder = TaskBuilder::<CompletionTaskTypes<TYPES, I>>::new(
                    "Test Completion Task".to_string(),
                )
                .register_event_stream(test_event_stream, FilterEvent::default())
                .await
                .register_registry(&mut registry)
                .await
                .register_state(state)
                .register_event_handler(event_handler)
                .register_message_handler(message_handler)
                .register_message_stream(stream_generator);
                let task_id = builder.get_task_id().unwrap();
                (task_id, CompletionTaskTypes::build(builder).launch())
            }
            .boxed()
        })
    }
}
