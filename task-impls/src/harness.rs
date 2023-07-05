use crate::events::SequencingHotShotEvent;
use async_compatibility_layer::art::async_spawn;

use futures::FutureExt;
use hotshot_task::event_stream::EventStream;
use hotshot_task::{
    event_stream::{self, ChannelStream},
    task::{FilterEvent, HandleEvent, HotShotTaskCompleted, HotShotTaskTypes, TaskErr, TS},
    task_impls::{HSTWithEvent, TaskBuilder},
    task_launcher::TaskRunner,
};

use hotshot_types::traits::node_implementation::{NodeImplementation, NodeType};
use snafu::Snafu;
use std::collections::HashSet;
use std::sync::Arc;

pub struct TestHarnessState<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    expected_output: HashSet<SequencingHotShotEvent<TYPES, I>>,
    timeout: usize,
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

/// view sync error type
#[derive(Snafu, Debug)]
pub struct TestHarnessTaskError {}
impl TaskErr for TestHarnessTaskError {}

pub type TestHarnessTaskTypes<TYPES, I> = HSTWithEvent<
    TestHarnessTaskError,
    SequencingHotShotEvent<TYPES, I>,
    ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    TestHarnessState<TYPES, I>,
>;

pub async fn run_harness<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    input: Vec<SequencingHotShotEvent<TYPES, I>>,
    expected_output: HashSet<SequencingHotShotEvent<TYPES, I>>,
    build_fn: fn(TaskRunner, ChannelStream<SequencingHotShotEvent<TYPES, I>>) -> TaskRunner,
) {
    let task_runner = TaskRunner::new();
    let registry = task_runner.registry.clone();
    let event_stream = event_stream::ChannelStream::new();
    let state = TestHarnessState {
        expected_output,
        timeout: 10000,
    };
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

    let id = builder.get_task_id().unwrap();

    let task = TestHarnessTaskTypes::build(builder).launch();

    let task_runner = task_runner.add_task(id, "test_harness".to_string(), task);
    let task_runner = build_fn(task_runner, event_stream.clone());

    let runner = async_spawn(async move { task_runner.launch().await });

    for event in input {
        event_stream.publish(event);
    }

    let results = runner.await;
    for (_task_name, result) in results {
        assert!(matches!(result, HotShotTaskCompleted::ShutDown));
    }
}

pub fn handle_event<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    event: SequencingHotShotEvent<TYPES, I>,
    mut state: TestHarnessState<TYPES, I>,
) -> (
    std::option::Option<HotShotTaskCompleted>,
    TestHarnessState<TYPES, I>,
) {
    if !state.expected_output.contains(&event) {
        panic!("Got and unexpected event: {:?}", event);
    }
    state.expected_output.remove(&event);

    if state.expected_output.is_empty() {
        return (Some(HotShotTaskCompleted::ShutDown), state);
    }
    (None, state)
}
