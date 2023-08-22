use crate::events::SequencingHotShotEvent;
use async_compatibility_layer::art::async_spawn;

use futures::FutureExt;
use hotshot_task::event_stream::EventStream;
use hotshot_task::{
    event_stream::{self, ChannelStream},
    task::{FilterEvent, HandleEvent, HotShotTaskCompleted, HotShotTaskTypes, TS},
    task_impls::{HSTWithEvent, TaskBuilder},
    task_launcher::TaskRunner,
};
use hotshot_types::traits::node_implementation::{NodeImplementation, NodeType};
use snafu::Snafu;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use tracing::error;

pub struct TestHarnessState<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    expected_output: HashMap<SequencingHotShotEvent<TYPES, I>, usize>,
}

pub struct EventBundle<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    Vec<SequencingHotShotEvent<TYPES, I>>,
);

pub enum EventInputOutput<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    Input(EventBundle<TYPES, I>),
    Output(EventBundle<TYPES, I>),
}

pub struct EventSequence<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    Vec<EventInputOutput<TYPES, I>>,
);

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> TS for TestHarnessState<TYPES, I> {}

#[derive(Snafu, Debug)]
pub struct TestHarnessTaskError {}

pub type TestHarnessTaskTypes<TYPES, I> = HSTWithEvent<
    TestHarnessTaskError,
    SequencingHotShotEvent<TYPES, I>,
    ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    TestHarnessState<TYPES, I>,
>;

// pub type TestHarnessMessageTaskTypes<TYPES, I> = HSTWithMessage<
//     TestHarnessTaskError,
//     Either<Messages<TYPES, I>, Messages<TYPES, I>>,
//     // A combination of broadcast and direct streams.
//     Merge<GeneratedStream<Messages<TYPES, I>>, GeneratedStream<Messages<TYPES, I>>>,
//     TestHarnessState<TYPES, I>,
// >;

pub async fn run_harness<TYPES: NodeType, I: NodeImplementation<TYPES>, Fut>(
    input: Vec<SequencingHotShotEvent<TYPES, I>>,
    expected_output: HashMap<SequencingHotShotEvent<TYPES, I>, usize>,
    build_fn: impl FnOnce(TaskRunner, ChannelStream<SequencingHotShotEvent<TYPES, I>>) -> Fut,
) where
    Fut: Future<Output = TaskRunner>,
{
    let task_runner = TaskRunner::new();
    let registry = task_runner.registry.clone();
    let event_stream = event_stream::ChannelStream::new();
    let state = TestHarnessState { expected_output };
    let handler = HandleEvent(Arc::new(move |event, state| {
        async move { handle_event(event, state) }.boxed()
    }));
    let filter = FilterEvent::default();
    let builder = TaskBuilder::<TestHarnessTaskTypes<TYPES, I>>::new("test_harness".to_string())
        .register_event_stream(event_stream.clone(), filter)
        .await
        .register_registry(&mut registry.clone())
        .await
        .register_state(state)
        .register_event_handler(handler);
    // if handle_messages {
    //     let channel = exchange.network().clone();
    //     let broadcast_stream = GeneratedStream::<Messages<TYPES, I>>::new(Arc::new(move || {
    //         let network = channel.clone();
    //         let closure = async move {
    //             loop {
    //                 let msgs = Messages(
    //                     network
    //                         .recv_msgs(TransmitType::Broadcast)
    //                         .await
    //                         .expect("Failed to receive broadcast messages"),
    //                 );
    //                 if msgs.0.is_empty() {
    //                     async_sleep(Duration::new(0, 500)).await;
    //                 } else {
    //                     break msgs;
    //                 }
    //             }
    //         };
    //         Some(boxed_sync(closure))
    //     }));
    //     let channel = exchange.network().clone();
    //     let direct_stream = GeneratedStream::<Messages<TYPES, I>>::new(Arc::new(move || {
    //         let network = channel.clone();
    //         let closure = async move {
    //             loop {
    //                 let msgs = Messages(
    //                     network
    //                         .recv_msgs(TransmitType::Direct)
    //                         .await
    //                         .expect("Failed to receive direct messages"),
    //                 );
    //                 if msgs.0.is_empty() {
    //                     async_sleep(Duration::new(0, 500)).await;
    //                 } else {
    //                     break msgs;
    //                 }
    //             }
    //         };
    //         Some(boxed_sync(closure))
    //     }));
    //     let message_stream = Merge::new(broadcast_stream, direct_stream);
    //     let message_builder =
    //     TaskBuilder::<NetworkMessageTaskTypes<_, _>>::new("test_harness_message".to_string())
    //         .register_message_stream(message_stream)
    //         .register_registry(&mut registry.clone())
    //         .await
    //         .register_state(network_state)
    //         .register_message_handler(network_message_handler);
    // }

    let id = builder.get_task_id().unwrap();

    let task = TestHarnessTaskTypes::build(builder).launch();

    let task_runner = task_runner.add_task(id, "test_harness".to_string(), task);
    let task_runner = build_fn(task_runner, event_stream.clone()).await;

    let runner = async_spawn(async move { task_runner.launch().await });

    for event in input {
        let _ = event_stream.publish(event).await;
    }
    let _ = runner.await;
}

pub fn handle_event<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    event: SequencingHotShotEvent<TYPES, I>,
    mut state: TestHarnessState<TYPES, I>,
) -> (
    std::option::Option<HotShotTaskCompleted>,
    TestHarnessState<TYPES, I>,
) {
    error!("got event: {:?}", event);
    if !state.expected_output.contains_key(&event) {
        panic!("Got an unexpected event: {:?}", event);
    }
    let num_expected = state.expected_output.get_mut(&event).unwrap();
    if *num_expected == 1 {
        state.expected_output.remove(&event);
    } else {
        *num_expected -= 1;
    }

    if state.expected_output.is_empty() {
        return (Some(HotShotTaskCompleted::ShutDown), state);
    }
    (None, state)
}
