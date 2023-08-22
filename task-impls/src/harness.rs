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

pub async fn run_harness<TYPES, I, Fut>(
    input: Vec<SequencingHotShotEvent<TYPES, I>>,
    expected_output: HashMap<SequencingHotShotEvent<TYPES, I>, usize>,
    build_fn: impl FnOnce(TaskRunner, ChannelStream<SequencingHotShotEvent<TYPES, I>>) -> Fut,
) where
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
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
    if !state.expected_output.contains_key(&event) {
        panic!("Got and unexpected event: {:?}", event);
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

pub async fn build_api(
    node_id: u64,
) -> SystemContextHandle<SequencingTestTypes, SequencingMemoryImpl> {
    let builder = TestMetadata::default_multiple_rounds();

    let launcher = builder.gen_launcher::<SequencingTestTypes, SequencingMemoryImpl>();

    let networks = (launcher.resource_generator.channel_generator)(node_id);
    let storage = (launcher.resource_generator.storage)(node_id);
    let config = launcher.resource_generator.config.clone();

    let initializer = HotShotInitializer::<
        SequencingTestTypes,
        <SequencingMemoryImpl as NodeImplementation<SequencingTestTypes>>::Leaf,
    >::from_genesis(<SequencingMemoryImpl as TestableNodeImplementation<
        SequencingTestTypes,
    >>::block_genesis())
    .unwrap();

    let known_nodes = config.known_nodes.clone();
    let known_nodes_with_stake = config.known_nodes_with_stake.clone();
    let private_key = <BN254Pub as SignatureKey>::generated_from_seed_indexed([0u8; 32], node_id).1;
    let public_key = <SequencingTestTypes as NodeType>::SignatureKey::from_private(&private_key);
    let quorum_election_config = config.election_config.clone().unwrap_or_else(|| {
        <QuorumEx<SequencingTestTypes, SequencingMemoryImpl> as ConsensusExchange<
            SequencingTestTypes,
            Message<SequencingTestTypes, SequencingMemoryImpl>,
        >>::Membership::default_election_config(config.total_nodes.get() as u64)
    });

    let committee_election_config = config.election_config.clone().unwrap_or_else(|| {
        <CommitteeEx<SequencingTestTypes, SequencingMemoryImpl> as ConsensusExchange<
            SequencingTestTypes,
            Message<SequencingTestTypes, SequencingMemoryImpl>,
        >>::Membership::default_election_config(config.total_nodes.get() as u64)
    });
    let exchanges =
        <SequencingMemoryImpl as NodeImplementation<SequencingTestTypes>>::Exchanges::create(
            known_nodes_with_stake.clone(),
            known_nodes.clone(),
            (quorum_election_config, committee_election_config),
            networks,
            public_key,
            public_key.get_stake_table_entry(1u64),
            private_key.clone(),
        );
    SystemContext::init(
        public_key,
        private_key,
        node_id,
        config,
        storage,
        exchanges,
        initializer,
        NoMetrics::boxed(),
    )
    .await
    .expect("Could not init hotshot")
}
