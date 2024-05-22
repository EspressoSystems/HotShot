use std::{time::Duration};

use hotshot::tasks::task_state::CreateTaskState;
use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
use hotshot_macros::test_scripts;
use hotshot_task_impls::{
    consensus::ConsensusTaskState, events::HotShotEvent::*,
};
use hotshot_testing::{
    predicates::event::{exact, timeout_vote_send, view_sync_timeout},
    script::{Expectations, TaskScript},
};
use hotshot_types::{
    data::{ViewNumber},
    traits::{
        node_implementation::{ConsensusTime},
    },
};

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_view_sync_task() {
    use hotshot_task_impls::{view_sync::ViewSyncTaskState};
    use hotshot_testing::task_helpers::build_system_handle;
    use hotshot_types::simple_vote::ViewSyncPreCommitData;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    // Build the API for node 5.
    let handle = build_system_handle(5).await.0;

    let vote_data = ViewSyncPreCommitData {
        relay: 0,
        round: <TestTypes as hotshot_types::traits::node_implementation::NodeType>::Time::new(5),
    };
    let vote = hotshot_types::simple_vote::ViewSyncPreCommitVote::<TestTypes>::create_signed_vote(
        vote_data,
        <TestTypes as hotshot_types::traits::node_implementation::NodeType>::Time::new(5),
        hotshot_types::traits::consensus_api::ConsensusApi::public_key(&handle),
        hotshot_types::traits::consensus_api::ConsensusApi::private_key(&handle),
    )
    .expect("Failed to create a ViewSyncPreCommitVote!");

    let view_sync_state = ViewSyncTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;
    let consensus_state = ConsensusTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;

    let inputs = vec![vec![ViewChange(ViewNumber::new(2))]];

    let mut view_sync_script = TaskScript {
        timeout: Duration::from_millis(2000),
        ignore_trailing: true,
        state: view_sync_state,
        expectations: vec![Expectations {
            output_asserts: vec![
                exact(Timeout(ViewNumber::new(3))),
                exact(ViewChange(ViewNumber::new(3))),
                exact(Timeout(ViewNumber::new(4))),
                exact(ViewSyncPreCommitVoteSend(vote.clone())),
            ],
            task_state_asserts: vec![],
        }],
    };

    let mut consensus_script = TaskScript {
        timeout: Duration::from_millis(2000),
        ignore_trailing: true,
        state: consensus_state,
        expectations: vec![Expectations {
            output_asserts: vec![timeout_vote_send(), timeout_vote_send(), view_sync_timeout()],
            task_state_asserts: vec![],
        }],
    };

    test_scripts![inputs, consensus_script, view_sync_script].await;
}
