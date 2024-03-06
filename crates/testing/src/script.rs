use crate::predicates::Predicate;
use async_broadcast::broadcast;
use hotshot_task_impls::events::{HotShotEvent, HotShotTaskCompleted};

use hotshot_task::task::{Task, TaskRegistry, TaskState};
use hotshot_types::traits::node_implementation::NodeType;
use std::sync::Arc;

/// A `TestScript` is a sequence of triples (input sequence, output sequence, assertions).
type TestScript<TYPES, S> = Vec<TestScriptStage<TYPES, S>>;

fn panic_extra_output<S>(stage_number: usize, output: &S)
where
    S: std::fmt::Debug,
{
    let extra_output_error = format!(
        "Stage {} | Received unexpected additional output: {:?}",
        stage_number, output
    );

    panic!("{}", extra_output_error);
}

fn panic_missing_output<S>(stage_number: usize, output: &S)
where
    S: std::fmt::Debug,
{
    let output_missing_error = format!(
        "Stage {} | Failed to receive output for predicate: {:?}",
        stage_number, output
    );

    panic!("{}", output_missing_error);
}

fn validate_task_state_or_panic<S>(stage_number: usize, state: &S, assert: &Predicate<S>) {
    assert!(
        (assert.function)(state),
        "Stage {} | Task state failed to satisfy: {:?}",
        stage_number,
        assert
    );
}

fn validate_output_or_panic<S>(stage_number: usize, output: &S, assert: &Predicate<S>) {
    assert!(
        (assert.function)(output),
        "Stage {} | Output failed to satisfy: {:?}",
        stage_number,
        assert
    );
}

pub struct TestScriptStage<TYPES: NodeType, S: TaskState<Event = HotShotEvent<TYPES>>> {
    pub inputs: Vec<HotShotEvent<TYPES>>,
    pub outputs: Vec<Predicate<HotShotEvent<TYPES>>>,
    pub asserts: Vec<Predicate<S>>,
}

pub struct TaskScript<TYPES: NodeType> {
    pub state: Box<dyn TaskState<Event = HotShotEvent<TYPES>, Output = HotShotTaskCompleted>>,
    pub expectations: Vec<Expectations<TYPES, Box<dyn TaskState<Event = HotShotEvent<TYPES>, Output = HotShotTaskCompleted>>>>,
}

pub struct Expectations<TYPES: NodeType, S> {
    pub output_asserts: Vec<Predicate<HotShotEvent<TYPES>>>,
    pub task_state_asserts: Vec<Predicate<S>>,
}

// pub fn dummy<TYPES: NodeType>(dummy: Vec<TaskScript<TYPES, Box<dyn TaskState<Event = HotShotEvent<TYPES>, Output = ()>>>>) { todo!() }

pub async fn run_scripts<TYPES>(
    inputs: Vec<Vec<HotShotEvent<TYPES>>>,
    task_expectations: Vec<
        TaskScript<TYPES> //, Box<dyn TaskState<Event = HotShotEvent<TYPES>, Output = HotShotTaskCompleted> + 'static>>,
    >,
) where
    TYPES: NodeType,
{
    let registry = Arc::new(TaskRegistry::default());

    let (test_input, task_receiver) = broadcast(1024);
    // let (task_input, mut test_receiver) = broadcast(1024);

    let task_input = test_input.clone();
    let mut test_receiver = task_receiver.clone();

    let mut loop_receiver = task_receiver.clone();

    // let mut tasks: Vec<_> = task_expectations.into_iter().map(
    // |e| { Task {
    //   event_sender: task_input.clone(), 
    //   event_receiver: task_receiver.clone(), registry: registry.clone(), state: (e.state) }}


    // ).collect();
    //
    let exp = task_expectations[0];

    let task_c =

//     Task {
//       event_sender: task_input.clone(), 
//       event_receiver: task_receiver.clone(), registry: registry.clone(), state: Box::new(*(exp.state)) };
     Task::new(task_input.clone(), task_receiver.clone(), registry.clone(), *(exp.state));


//        .into_iter()
//        .map(|expectation| {
//            (
//                Task::new(
//                    task_input.clone(),
//                    task_receiver.clone(),
//                    registry.clone(),
//                    *(expectation.state),
//                ),
//                expectation.expectations,
//                0,
//            )
//        })
//        .collect();

//    for (stage_number, input_group) in inputs.into_iter().enumerate() {
//        for input in &input_group {
//            for (task, task_expectation, output_index) in &mut tasks {
//                if !task.state().filter(input) {
//                    tracing::debug!("Test sent: {:?}", input);
//
//                    if let Some(res) = task.handle_event(input.clone()).await {
//                        task.state().handle_result(&res).await;
//                    }
//
//                    while let Ok(received_output) = test_receiver.try_recv() {
//                        tracing::debug!("Test received: {:?}", received_output);
//
//                        let output_asserts = &task_expectation[stage_number].output_asserts;
//
//                        if *output_index >= output_asserts.len() {
//                            panic_extra_output(stage_number, &received_output);
//                        };
//
//                        let assert = &output_asserts[*output_index];
//
//                        validate_output_or_panic(stage_number, &received_output, assert);
//
//                        *output_index += 1;
//                    }
//                }
//            }
//        }
//
//        while let Ok(input) = loop_receiver.try_recv() {
//            for (task, task_expectation, output_index) in &mut tasks {
//                if !task.state().filter(&input) {
//                    tracing::debug!("Test sent: {:?}", input);
//
//                    if let Some(res) = task.handle_event(input.clone()).await {
//                        task.state().handle_result(&res).await;
//                    }
//
//                    while let Ok(received_output) = test_receiver.try_recv() {
//                        tracing::debug!("Test received: {:?}", received_output);
//
//                        let output_asserts = &task_expectation[stage_number].output_asserts;
//
//                        if *output_index >= output_asserts.len() {
//                            panic_extra_output(stage_number, &received_output);
//                        };
//
//                        let assert = &output_asserts[*output_index];
//
//                        validate_output_or_panic(stage_number, &received_output, assert);
//
//                        *output_index += 1;
//                    }
//                }
//            }
//        }
//
//        for (task, task_expectation, output_index) in &mut tasks {
//            let output_asserts = &task_expectation[stage_number].output_asserts;
//
//            if *output_index < output_asserts.len() {
//                panic_missing_output(stage_number, &output_asserts[*output_index]);
//            }
//
//            let task_state_asserts = &task_expectation[stage_number].task_state_asserts;
//
//            for assert in task_state_asserts {
//                validate_task_state_or_panic(stage_number, task.state(), assert);
//            }
//        }
//    }
}

pub struct ScriptStage<TYPES: NodeType, S: TaskState<Event = HotShotEvent<TYPES>>> {
    pub outputs: Vec<Predicate<HotShotEvent<TYPES>>>,
    pub asserts: Vec<Predicate<S>>,
}

impl<INPUT> std::fmt::Debug for Predicate<INPUT> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "{}", self.info)
    }
}

/// `run_test_script` reads a triple (inputs, outputs, asserts) in a `TestScript`,
/// It broadcasts all given inputs (in order) and waits to receive all outputs (in order).
/// Once the expected outputs have been received, it validates the task state at that stage
/// against the given assertions.
///
/// If all assertions pass, it moves onto the next stage. If it receives an unexpected output
/// or fails to receive an output, the test fails immediately with a panic.
///
/// Note: the task is not spawned with an async thread; instead, the harness just calls `handle_event`.
/// This has a few implications, e.g. shutting down tasks doesn't really make sense,
/// and event ordering is deterministic.
pub async fn run_test_script<TYPES, S: TaskState<Event = HotShotEvent<TYPES>>>(
    mut script: TestScript<TYPES, S>,
    state: S,
) where
    TYPES: NodeType,
    S: Send + 'static,
{
    let registry = Arc::new(TaskRegistry::default());

    let (test_input, task_receiver) = broadcast(1024);
    // let (task_input, mut test_receiver) = broadcast(1024);

    let task_input = test_input.clone();
    let mut test_receiver = task_receiver.clone();

    let mut task = Task::new(
        task_input.clone(),
        task_receiver.clone(),
        registry.clone(),
        state,
    );

    for (stage_number, stage) in script.iter_mut().enumerate() {
        tracing::debug!("Beginning test stage {}", stage_number);
        for input in &mut *stage.inputs {
            if !task.state_mut().filter(input) {
                tracing::debug!("Test sent: {:?}", input);

                if let Some(res) = S::handle_event(input.clone(), &mut task).await {
                    task.state_mut().handle_result(&res).await;
                }
            }
        }

        for assert in &stage.outputs {
            if let Ok(received_output) = test_receiver.try_recv() {
                tracing::debug!("Test received: {:?}", received_output);
                validate_output_or_panic(stage_number, &received_output, assert);
            } else {
                panic_missing_output(stage_number, assert);
            }
        }

        for assert in &stage.asserts {
            validate_task_state_or_panic(stage_number, task.state(), assert);
        }

        if let Ok(received_output) = test_receiver.try_recv() {
            panic_extra_output(stage_number, &received_output);
        }
    }
}
