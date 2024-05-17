use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::Result;
use async_broadcast::{broadcast, Receiver, Sender};
use async_compatibility_layer::art::async_timeout;
use async_trait::async_trait;
use hotshot_task::task::{ConsensusTaskRegistry, Task, TaskState};
use hotshot_types::traits::node_implementation::NodeType;

use crate::events::{HotShotEvent, HotShotTaskCompleted};

/// The state for the test harness task. Keeps track of which events and how many we expect to get
pub struct TestHarnessState<TYPES: NodeType> {
    /// The expected events we get from the test.  Maps an event to the number of times we expect to see it
    expected_output: HashMap<HotShotEvent<TYPES>, usize>,
    /// If true we won't fail the test if extra events come in
    allow_extra_output: bool,
}

#[async_trait]
impl<TYPES: NodeType> TaskState for TestHarnessState<TYPES> {
    type Event = HotShotEvent<TYPES>;

    async fn handle_event(
        &mut self,
        event: Arc<Self::Event>,
        _sender: &Sender<Arc<Self::Event>>,
        _receiver: &Receiver<Arc<Self::Event>>,
    ) -> Result<()> {
        let extra = self.allow_extra_output;
        handle_event(event, self, extra);

        Ok(())
    }

    async fn cancel_subtasks(&mut self) {}
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
/// Panics if any state the test expects is not set. Panicking causes a test failure
#[allow(clippy::implicit_hasher)]
#[allow(clippy::panic)]
pub async fn run_harness<TYPES, S: TaskState<Event = HotShotEvent<TYPES>> + Send + 'static>(
    input: Vec<HotShotEvent<TYPES>>,
    expected_output: HashMap<HotShotEvent<TYPES>, usize>,
    state: S,
    allow_extra_output: bool,
) where
    TYPES: NodeType,
    TestHarnessState<TYPES>: TaskState<Event = HotShotEvent<TYPES>>,
{
    let registry = ConsensusTaskRegistry::new();
    // set up two broadcast channels so the test sends to the task and the task back to the test
    let (to_task, from_test) = broadcast(1024);
    let (to_test, from_task) = broadcast(1024);
    let test_state = TestHarnessState {
        expected_output,
        allow_extra_output,
    };

    let test_task = Task::new(test_state, to_test.clone(), from_task.clone());
    let task = Task::new(state, to_test.clone(), from_test.clone());

    let test_handle = test_task.run();
    let handle = task.run();

    registry.register(test_handle).await;
    registry.register(handle).await;

    for event in input {
        to_task.broadcast_direct(Arc::new(event)).await.unwrap();
    }

    if async_timeout(Duration::from_secs(2), registry.join_all())
        .await
        .is_err()
    {
        panic!("Test timeout out before all all expected outputs received");
    }
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
    event: Arc<HotShotEvent<TYPES>>,
    state: &mut TestHarnessState<TYPES>,
    allow_extra_output: bool,
) -> Option<HotShotTaskCompleted> {
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
        tracing::info!("test harness task completed");
        return Some(HotShotTaskCompleted);
    }

    None
}
