use crate::events::{HotShotEvent, HotShotTaskCompleted};
use async_broadcast::broadcast;

use hotshot_types::traits::node_implementation::NodeType;
use std::{collections::HashMap, future::Future, sync::Arc};
use task::task::{Task, TaskRegistry, TaskState};

/// The state for the test harness task. Keeps track of which events and how many we expect to get
pub struct TestHarnessState<TYPES: NodeType> {
    /// The expected events we get from the test.  Maps an event to the number of times we expect to see it
    expected_output: HashMap<HotShotEvent<TYPES>, usize>,
    ///
    allow_extra_output: bool,
}

impl<TYPES: NodeType> TaskState for TestHarnessState<TYPES> {
    type Event = HotShotEvent<TYPES>;
    type Result = HotShotTaskCompleted;

    async fn handle_event(
        event: Self::Event,
        task: &mut task::task::Task<Self>,
    ) -> Option<HotShotTaskCompleted> {
        let extra = task.state_mut().allow_extra_output;
        handle_event(event, task, extra)
    }

    fn should_shutdown(event: &Self::Event) -> bool {
        matches!(event, HotShotEvent::Shutdown)
    }
}

/// Runs a test by building the task using `build_fn` and then passing it the `input` events
/// and testing the make sure all of the `expected_output` events are seen
///
/// # Arguments
/// * `event_stream` - if given, will be used to register the task builder.
/// * `allow_extra_output` - whether to allow an extra output after we've seen all expected
/// outputs. Should be `false` in most cases.
///
/// # Panics
/// Panics if any state the test expects is not set. Panicing causes a test failure
#[allow(clippy::implicit_hasher)]
pub async fn run_harness<TYPES, S: TaskState<Event = HotShotEvent<TYPES>>>(
    input: Vec<HotShotEvent<TYPES>>,
    expected_output: HashMap<HotShotEvent<TYPES>, usize>,
    state: S,
    allow_extra_output: bool,
) where
    TYPES: NodeType,
    S: Send + 'static,
{
    let registry = Arc::new(TaskRegistry::default());
    // set up two broadcast channels so the test sends to the task and the task back to the test
    let (to_task, from_test) = broadcast(1024);
    let (to_test, from_task) = broadcast(1024);
    let test_state = TestHarnessState {
        expected_output,
        allow_extra_output,
    };

    let test_task = Task::new(
        to_test.clone(),
        from_task.clone(),
        registry.clone(),
        test_state,
    );
    let task = Task::new(to_task.clone(), from_test.clone(), registry.clone(), state);
    registry.run_task(test_task).await;
    registry.run_task(task).await;

    for event in input {
        let _ = to_task.broadcast(event).await.unwrap();
    }

    let _ = Arc::into_inner(registry).unwrap().join_all().await;
}

/// Handles an event for the Test Harness Task.  If the event is expected, remove it from
/// the `expected_output` in state.  If unexpected fail test.
///
/// # Arguments
/// * `allow_extra_output` - whether to allow an extra output after we've seen all expected
/// outputs. Should be `false` in most cases.
///
///  # Panics
/// Will panic to fail the test when it receives and unexpected event
#[allow(clippy::needless_pass_by_value)]
pub fn handle_event<TYPES: NodeType>(
    event: HotShotEvent<TYPES>,
    task: &mut Task<TestHarnessState<TYPES>>,
    allow_extra_output: bool,
) -> Option<HotShotTaskCompleted> {
    let state = task.state_mut();
    // Check the output in either case:
    // * We allow outputs only in our expected output set.
    // * We haven't received all expected outputs yet.
    if !allow_extra_output || !state.expected_output.is_empty() {
        assert!(
            state.expected_output.contains_key(&event),
            "Got an unexpected event: {event:?}",
        );
    }

    let num_expected = state.expected_output.get_mut(&event).unwrap();
    if *num_expected == 1 {
        state.expected_output.remove(&event);
    } else {
        *num_expected -= 1;
    }

    if state.expected_output.is_empty() {
        return Some(HotShotTaskCompleted);
    }

    None
}
