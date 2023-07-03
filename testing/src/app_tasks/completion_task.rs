use std::{sync::Arc, time::Duration};

use either::Either::{self, Left, Right};
use futures::{FutureExt, Stream, future::BoxFuture};
use hotshot::{tasks::DATaskState, traits::TestableNodeImplementation};
use async_compatibility_layer::{art::async_sleep, channel::UnboundedStream};
use hotshot_types::{traits::{node_implementation::NodeType, consensus_type::sequencing_consensus::SequencingConsensus}, event::Event};
use nll::nll_todo::nll_todo;
use snafu::Snafu;
use hotshot_task::{task::{TaskErr, TS, HST, HandleEvent, HandleMessage, HotShotTaskCompleted, FilterEvent, HotShotTaskTypes}, task_impls::{HSTWithEvent, HSTWithEventAndMessage, TaskBuilder}, event_stream::{ChannelStream, SendableStream, EventStream, self}, global_registry::{GlobalRegistry, HotShotTaskId}, GeneratedStream, boxed_sync, Merge};

use super::{GlobalTestEvent, TestTask};

/// the idea here is to run as long as we want

/// Data Availability task error
#[derive(Snafu, Debug)]
pub struct CompletionTaskErr {}
impl TaskErr for CompletionTaskErr {}

/// Data availability task state
pub struct CompletionTask {
    test_event_stream: ChannelStream<GlobalTestEvent>
}
impl TS for CompletionTask {}

/// Completion task types
pub type CompletionTaskTypes<
    TYPES: NodeType,
    I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>
> =
    HSTWithEventAndMessage<
        CompletionTaskErr,
        GlobalTestEvent,
        ChannelStream<GlobalTestEvent>,
        Either<(), Event<TYPES, I::Leaf>>,
        Merge<GeneratedStream<()>, UnboundedStream<Event<TYPES, I::Leaf>>>,
        CompletionTask
    >;

pub struct TimeBasedCompletionTaskBuilder {
    duration: Duration,
    state: CompletionTask
}

impl TimeBasedCompletionTaskBuilder {
    /// create the task and launch it
    pub async fn build_and_launch<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>(
        self,
        registry: &mut GlobalRegistry,
        test_event_stream: ChannelStream<GlobalTestEvent>,
        hotshot_event_stream: UnboundedStream<Event<TYPES, I::Leaf>>,
    ) -> (HotShotTaskId, BoxFuture<'static, HotShotTaskCompleted>) {
        // TODO we'll possibly want multiple criterion including:
        // - certain number of txns committed
        // - anchor of certain depth
        // - some other stuff? probably?
        let event_handler = HandleEvent::<CompletionTaskTypes<TYPES, I>>(Arc::new(move |event, state| {
            async move {
                match event {
                    GlobalTestEvent::ShutDown => {
                        return (Some(HotShotTaskCompleted::ShutDown), state);
                    },
                    // TODO
                    _ => { unimplemented!() }
                }
            }.boxed()
        }));
        let message_handler = HandleMessage::<CompletionTaskTypes<TYPES, I>>(Arc::new(move |msg, state| {
            async move {
                match msg {
                    Left(_) => {
                        // msg is from timer
                        // at this point we're done.
                        state.test_event_stream.publish(GlobalTestEvent::ShutDown).await;
                        (Some(HotShotTaskCompleted::ShutDown), state)
                    },
                    Right(_) => {
                        (None, state)
                    }
                }
            }.boxed()
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
        let merged_stream = Merge::new(stream_generator, hotshot_event_stream);
        let builder = TaskBuilder::<CompletionTaskTypes<TYPES, I>>::new("Test Completion Task".to_string())
            .register_event_stream(test_event_stream, FilterEvent::default()).await
            .register_registry(registry).await
            .register_state(self.state)
            .register_event_handler(event_handler)
            .register_message_handler(message_handler)
            .register_message_stream(merged_stream)
            ;
        let task_id = builder.get_task_id().unwrap();
        (task_id, CompletionTaskTypes::<TYPES, I>::build(builder).launch())
    }
}
