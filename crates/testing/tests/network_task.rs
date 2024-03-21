use async_compatibility_layer::art::async_timeout;
use async_lock::RwLock;
use hotshot::tasks::add_network_message_task;
use hotshot::traits::implementations::MemoryNetwork;
use hotshot_example_types::node_types::MemoryImpl;
use hotshot_example_types::node_types::TestTypes;
use hotshot_task::task::{Task, TaskRegistry};
use hotshot_task_impls::events::HotShotEvent;
use hotshot_task_impls::network::{self, NetworkEventTaskState};
use hotshot_testing::test_builder::TestMetadata;
use hotshot_testing::view_generator::TestViewGenerator;
use hotshot_types::traits::election::Membership;
use hotshot_types::traits::node_implementation::NodeType;
use hotshot_types::{data::ViewNumber, traits::node_implementation::ConsensusTime};
use std::sync::Arc;
use std::time::Duration;

// Test that the event task sends a message, and the message task receives it
// and emits the proper event
#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
#[allow(clippy::too_many_lines)]
async fn test_network_task() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let builder = TestMetadata::default_multiple_rounds();
    let node_id = 1;

    let launcher = builder.gen_launcher::<TestTypes, MemoryImpl>(node_id);

    let networks = (launcher.resource_generator.channel_generator)(node_id);

    let storage = Arc::new(RwLock::new((launcher.resource_generator.storage)(node_id)));
    let config = launcher.resource_generator.config.clone();
    let public_key = config.my_own_validator_config.public_key;
    let known_nodes_with_stake = config.known_nodes_with_stake.clone();
    let quorum_election_config = config.election_config.clone().unwrap_or_else(|| {
        <TestTypes as NodeType>::Membership::default_election_config(
            config.num_nodes_with_stake.get() as u64,
            config.num_nodes_without_stake as u64,
        )
    });

    let membership = <TestTypes as NodeType>::Membership::create_election(
        known_nodes_with_stake.clone(),
        quorum_election_config.clone(),
    );
    let channel = networks.0.clone();
    let network_state: NetworkEventTaskState<TestTypes, MemoryNetwork<_, _>, _> =
        NetworkEventTaskState {
            channel: channel.clone(),
            view: ViewNumber::new(0),
            membership: membership.clone(),
            filter: network::quorum_filter,
            storage,
        };
    let (tx, rx) = async_broadcast::broadcast(10);
    let task_reg = Arc::new(TaskRegistry::default());

    let task = Task::new(tx.clone(), rx, task_reg.clone(), network_state);
    task_reg.run_task(task).await;

    let mut generator = TestViewGenerator::generate(membership.clone());
    let view = generator.next().unwrap();

    let (out_tx, mut out_rx) = async_broadcast::broadcast(10);
    add_network_message_task(task_reg, out_tx.clone(), channel.clone()).await;

    tx.broadcast_direct(Arc::new(HotShotEvent::QuorumProposalSend(
        view.quorum_proposal,
        public_key,
    )))
    .await
    .unwrap();
    let res = async_timeout(Duration::from_millis(100), out_rx.recv_direct())
        .await
        .expect("timed out waiting for response")
        .expect("channel closed");
    assert!(matches!(
        res.as_ref(),
        HotShotEvent::QuorumProposalRecv(_, _)
    ));
}

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_network_storage_fail() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let builder = TestMetadata::default_multiple_rounds();
    let node_id = 1;

    let launcher = builder.gen_launcher::<TestTypes, MemoryImpl>(node_id);

    let networks = (launcher.resource_generator.channel_generator)(node_id);

    let storage = Arc::new(RwLock::new((launcher.resource_generator.storage)(node_id)));
    storage.write().await.should_return_err = true;
    let config = launcher.resource_generator.config.clone();
    let public_key = config.my_own_validator_config.public_key;
    let known_nodes_with_stake = config.known_nodes_with_stake.clone();
    let quorum_election_config = config.election_config.clone().unwrap_or_else(|| {
        <TestTypes as NodeType>::Membership::default_election_config(
            config.num_nodes_with_stake.get() as u64,
            config.num_nodes_without_stake as u64,
        )
    });

    let membership = <TestTypes as NodeType>::Membership::create_election(
        known_nodes_with_stake.clone(),
        quorum_election_config.clone(),
    );
    let channel = networks.0.clone();
    let network_state: NetworkEventTaskState<TestTypes, MemoryNetwork<_, _>, _> =
        NetworkEventTaskState {
            channel: channel.clone(),
            view: ViewNumber::new(0),
            membership: membership.clone(),
            filter: network::quorum_filter,
            storage,
        };
    let (tx, rx) = async_broadcast::broadcast(10);
    let task_reg = Arc::new(TaskRegistry::default());

    let task = Task::new(tx.clone(), rx, task_reg.clone(), network_state);
    task_reg.run_task(task).await;

    let mut generator = TestViewGenerator::generate(membership.clone());
    let view = generator.next().unwrap();

    let (out_tx, mut out_rx) = async_broadcast::broadcast(10);
    add_network_message_task(task_reg, out_tx.clone(), channel.clone()).await;

    tx.broadcast_direct(Arc::new(HotShotEvent::QuorumProposalSend(
        view.quorum_proposal,
        public_key,
    )))
    .await
    .unwrap();
    let res = async_timeout(Duration::from_millis(100), out_rx.recv_direct()).await;
    assert!(res.is_err());
}
