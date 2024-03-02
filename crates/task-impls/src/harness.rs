use crate::events::{HotShotEvent, HotShotTaskCompleted};
use async_broadcast::broadcast;

use async_compatibility_layer::art::async_timeout;
use hotshot_task::task::{Task, TaskRegistry, TaskState};
use hotshot_types::traits::node_implementation::NodeType;
use std::{collections::HashMap, sync::Arc, time::Duration};

/// The state for the test harness task. Keeps track of which events and how many we expect to get
pub struct TestHarnessState<TYPES: NodeType> {
    /// The expected events we get from the test.  Maps an event to the number of times we expect to see it
    expected_output: HashMap<HotShotEvent<TYPES>, usize>,
    /// If true we won't fail the test if extra events come in
    allow_extra_output: bool,
}

impl<TYPES: NodeType> TaskState for TestHarnessState<TYPES> {
    type Event = HotShotEvent<TYPES>;
    type Output = HotShotTaskCompleted;

    async fn handle_event(
        event: Self::Event,
        task: &mut Task<Self>,
    ) -> Option<HotShotTaskCompleted> {
        let extra = task.state_mut().allow_extra_output;
        handle_event(event, task, extra)
    }

    fn should_shutdown(event: &Self::Event) -> bool {
        matches!(event, HotShotEvent::Shutdown)
    }
}

// A much more tightly-choreographed version of `TestHarnessState`.
//
// A `TestScript` is a sequence of pairs (input sequence, output sequence).
type TestScript<TYPES, S> = Vec<TestScriptStage<TYPES, S>>;

pub struct TestScriptStage<TYPES: NodeType, S: TaskState<Event = HotShotEvent<TYPES>>> {
    pub inputs: Vec<HotShotEvent<TYPES>>,
    pub outputs: Vec<Predicate<HotShotEvent<TYPES>>>,
    pub asserts: Vec<Predicate<S>>,
}

pub struct Predicate<INPUT> {
    pub function: Box<dyn for<'a> Fn(&'a INPUT) -> bool>,
    pub info: String,
}

impl<INPUT> std::fmt::Debug for Predicate<INPUT> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "{}", self.info)
    }
}

/// `run_test_script` reads a pair (inputs, outputs) in the vector,
/// broadcasts all given inputs (in order) and waits to receive all outputs (in order).
/// It moves on to the next pair only *after* it has finished receiving all expected outputs.
pub async fn run_test_script<TYPES, S: TaskState<Event = HotShotEvent<TYPES>>>(
    mut script: TestScript<TYPES, S>,
    state: S,
) where
    TYPES: NodeType,
    S: Send + 'static,
{
    let registry = Arc::new(TaskRegistry::default());

    let (test_input, task_receiver) = broadcast(1024);
    let (task_input, mut test_receiver) = broadcast(1024);

    let mut task = Task::new(
        task_input.clone(),
        task_receiver.clone(),
        registry.clone(),
        state,
    );

    for (stage_number, stage) in script.iter_mut().enumerate() {
        tracing::debug!("Beginning test stage {}", stage_number);
        for input in &mut *stage.inputs {
            if S::should_shutdown(&input) {
                task.state_mut().shutdown().await;
                break;
            }
            if task.state_mut().filter(&input) {
                continue;
            }
            if let Some(res) = S::handle_event(input.clone(), &mut task).await {
                task.state_mut().handle_result(&res).await;
                task.state_mut().shutdown().await;
                break;
            }
            tracing::debug!("Test sent: {:?}", input);
        }

        for expected in &stage.outputs {
            let timeout_error = format!(
                "Timeout while waiting for output at stage {}: {:?}",
                stage_number, expected
            );

            if let Ok(received_output) =
                async_timeout(Duration::from_secs(2), test_receiver.recv_direct())
                    .await
                    .expect(&timeout_error)
            {
                let predicate = &expected.function;
                tracing::debug!("Test received: {:?}", received_output);
                assert!(
                    predicate(&received_output),
                    "Output failed to satisfy {:?}",
                    expected
                );
            } else {
                panic!("{}", timeout_error);
            }
        }

        for assert in &stage.asserts {
            assert!(
                task.test_state_with(&assert.function),
                "TaskState assertion failed at stage {}: {:?}",
                stage_number,
                assert
            );
        }

        // Once we've seen all expected outputs, we wait to see
        // if there are any trailing unexpected outputs.
        if let Ok(output) = async_timeout(Duration::from_secs(2), test_receiver.recv_direct()).await
        {
            panic!(
                "Received unexpected output at stage {}: {:?}",
                stage_number, output
            );
        }
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
#[allow(clippy::panic)]
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
    let mut tasks = vec![];
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
    let task = Task::new(to_test.clone(), from_test.clone(), registry.clone(), state);

    tasks.push(test_task.run());
    tasks.push(task.run());

    for event in input {
        to_task.broadcast_direct(event).await.unwrap();
    }

    if async_timeout(Duration::from_secs(2), futures::future::join_all(tasks))
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
        tracing::info!("test harness task completed");
        return Some(HotShotTaskCompleted);
    }

    None
}
