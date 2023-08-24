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
use std::sync::Arc;
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

/// Build a task runner for testing harness.
pub async fn build_harness<TYPES, I>(
    expected_output: HashMap<SequencingHotShotEvent<TYPES, I>, usize>,
    event_stream: Option<ChannelStream<SequencingHotShotEvent<TYPES, I>>>,
) -> (TaskRunner, ChannelStream<SequencingHotShotEvent<TYPES, I>>)
where
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
{
    let task_runner = TaskRunner::new();
    let registry = task_runner.registry.clone();
    let event_stream = event_stream.unwrap_or(event_stream::ChannelStream::new());
    let state = TestHarnessState { expected_output };
    let handler = HandleEvent(Arc::new(move |event, state| {
        async move { handle_event(event, state) }.boxed()
    }));
    let filter = FilterEvent::default();
    let builder = TaskBuilder::<TestHarnessTaskTypes<TYPES, I>>::new("test_harness".to_string())
        .register_event_stream(event_stream.clone().clone(), filter)
        .await
        .register_registry(&mut registry.clone())
        .await
        .register_state(state)
        .register_event_handler(handler);

    let id = builder.get_task_id().unwrap();

    let task = TestHarnessTaskTypes::build(builder).launch();

    (
        task_runner.add_task(id, "test_harness".to_string(), task),
        event_stream,
    )
}

// Run the harness after subtasks are added.
pub async fn run_harness<TYPES, I>(
    input: Vec<SequencingHotShotEvent<TYPES, I>>,
    task_runner: TaskRunner,
    event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>,
) where
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
{
    let runner = async_spawn(async move { task_runner.launch().await });

    for event in input {
        let _ = event_stream.publish(event).await;
    }
    tracing::error!("before running harness");
    let _ = runner.await;
    tracing::error!("ran harness");
}

pub fn handle_event<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    event: SequencingHotShotEvent<TYPES, I>,
    mut state: TestHarnessState<TYPES, I>,
) -> (
    std::option::Option<HotShotTaskCompleted>,
    TestHarnessState<TYPES, I>,
) {
    tracing::error!("Got an event: {:?}", event);
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
        tracing::error!("shutdown harness");
        return (Some(HotShotTaskCompleted::ShutDown), state);
    }
    (None, state)
}
