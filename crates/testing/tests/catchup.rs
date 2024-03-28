#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_catchup() {
    use std::time::Duration;

    use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
    use hotshot_testing::{
        completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription},
        overall_safety_task::OverallSafetyPropertiesDescription,
        spinning_task::{ChangeNode, SpinningTaskDescription, UpDown},
        test_builder::{TestMetadata, TimingData},
    };
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();
    let timing_data = TimingData {
        next_view_timeout: 2000,
        ..Default::default()
    };
    let mut metadata = TestMetadata::default();
    let catchup_node = vec![ChangeNode {
        idx: 19,
        updown: UpDown::Up,
    }];

    metadata.timing_data = timing_data;
    metadata.start_nodes = 19;
    metadata.num_nodes_with_stake = 20;

    metadata.view_sync_properties =
        hotshot_testing::view_sync_task::ViewSyncTaskDescription::Threshold(0, 20);

    metadata.spinning_properties = SpinningTaskDescription {
        // Start the nodes before their leadership.
        node_changes: vec![(13, catchup_node)],
    };

    metadata.completion_task_description =
        CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_secs(60),
            },
        );
    metadata.overall_safety_properties = OverallSafetyPropertiesDescription {
        // Make sure we keep commiting rounds after the catchup, but not the full 50.
        num_successful_views: 22,
        num_failed_views: 5,
        ..Default::default()
    };

    metadata
        .gen_launcher::<TestTypes, MemoryImpl>(0)
        .launch()
        .run_test()
        .await;
}

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_catchup_cdn() {
    use std::time::Duration;

    use hotshot_example_types::node_types::{PushCdnImpl, TestTypes};
    use hotshot_testing::{
        completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription},
        overall_safety_task::OverallSafetyPropertiesDescription,
        spinning_task::{ChangeNode, SpinningTaskDescription, UpDown},
        test_builder::{TestMetadata, TimingData},
    };

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();
    let timing_data = TimingData {
        next_view_timeout: 2000,
        ..Default::default()
    };
    let mut metadata = TestMetadata::default();
    let catchup_nodes = vec![ChangeNode {
        idx: 18,
        updown: UpDown::Up,
    }];
    metadata.timing_data = timing_data;
    metadata.start_nodes = 19;
    metadata.num_nodes_with_stake = 20;

    metadata.spinning_properties = SpinningTaskDescription {
        // Start the nodes before their leadership.
        node_changes: vec![(10, catchup_nodes)],
    };

    metadata.completion_task_description =
        CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_millis(100_000),
            },
        );
    metadata.overall_safety_properties = OverallSafetyPropertiesDescription {
        num_failed_views: 5,
        ..Default::default()
    };

    metadata
        .gen_launcher::<TestTypes, PushCdnImpl>(0)
        .launch()
        .run_test()
        .await;
}

/// Test that one node catches up and has sucessful views after coming back
#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_catchup_one_node() {
    use std::time::Duration;

    use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
    use hotshot_testing::{
        completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription},
        overall_safety_task::OverallSafetyPropertiesDescription,
        spinning_task::{ChangeNode, SpinningTaskDescription, UpDown},
        test_builder::{TestMetadata, TimingData},
    };
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();
    let timing_data = TimingData {
        next_view_timeout: 2000,
        ..Default::default()
    };
    let mut metadata = TestMetadata::default();
    let catchup_nodes = vec![ChangeNode {
        idx: 18,
        updown: UpDown::Up,
    }];
    metadata.timing_data = timing_data;
    metadata.start_nodes = 19;
    metadata.num_nodes_with_stake = 20;

    metadata.spinning_properties = SpinningTaskDescription {
        // Start the nodes before their leadership.
        node_changes: vec![(10, catchup_nodes)],
    };

    metadata.completion_task_description =
        CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_secs(60),
            },
        );
    metadata.overall_safety_properties = OverallSafetyPropertiesDescription {
        // Make sure we keep commiting rounds after the catchup, but not the full 50.
        num_successful_views: 22,
        num_failed_views: 2,
        ..Default::default()
    };

    metadata
        .gen_launcher::<TestTypes, MemoryImpl>(0)
        .launch()
        .run_test()
        .await;
}

/// Same as `test_catchup` except we start the nodes after their leadership so they join during view sync
#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_catchup_in_view_sync() {
    use std::time::Duration;

    use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
    use hotshot_testing::{
        completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription},
        overall_safety_task::OverallSafetyPropertiesDescription,
        spinning_task::{ChangeNode, SpinningTaskDescription, UpDown},
        test_builder::{TestMetadata, TimingData},
    };
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();
    let timing_data = TimingData {
        next_view_timeout: 2000,
        ..Default::default()
    };
    let mut metadata = TestMetadata::default();
    let catchup_nodes = vec![
        ChangeNode {
            idx: 18,
            updown: UpDown::Up,
        },
        ChangeNode {
            idx: 19,
            updown: UpDown::Up,
        },
    ];

    metadata.timing_data = timing_data;
    metadata.start_nodes = 18;
    metadata.num_nodes_with_stake = 20;
    metadata.view_sync_properties =
        hotshot_testing::view_sync_task::ViewSyncTaskDescription::Threshold(0, 20);

    metadata.spinning_properties = SpinningTaskDescription {
        node_changes: vec![(10, catchup_nodes)],
    };

    metadata.completion_task_description =
        CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_secs(60),
            },
        );
    metadata.overall_safety_properties = OverallSafetyPropertiesDescription {
        num_failed_views: 5,
        ..Default::default()
    };

    metadata
        .gen_launcher::<TestTypes, MemoryImpl>(0)
        .launch()
        .run_test()
        .await;
}

// Almost the same as `test_catchup`, but with catchup nodes reloaded from anchor leaf rather than
// initialized from genesis.
#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_catchup_reload() {
    use std::time::Duration;

    use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
    use hotshot_testing::{
        completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription},
        overall_safety_task::OverallSafetyPropertiesDescription,
        spinning_task::{ChangeNode, SpinningTaskDescription, UpDown},
        test_builder::{TestMetadata, TimingData},
    };

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();
    let timing_data = TimingData {
        next_view_timeout: 2000,
        ..Default::default()
    };
    let mut metadata = TestMetadata::default();
    let catchup_node = vec![ChangeNode {
        idx: 19,
        updown: UpDown::Up,
    }];

    metadata.timing_data = timing_data;
    metadata.start_nodes = 19;
    metadata.skip_late = true;
    metadata.num_nodes_with_stake = 20;

    metadata.view_sync_properties =
        hotshot_testing::view_sync_task::ViewSyncTaskDescription::Threshold(0, 20);

    metadata.spinning_properties = SpinningTaskDescription {
        // Start the nodes before their leadership.
        node_changes: vec![(13, catchup_node)],
    };

    metadata.completion_task_description =
        CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_secs(60),
            },
        );
    metadata.overall_safety_properties = OverallSafetyPropertiesDescription {
        // Make sure we keep commiting rounds after the catchup, but not the full 50.
        num_successful_views: 22,
        ..Default::default()
    };

    metadata
        .gen_launcher::<TestTypes, MemoryImpl>(0)
        .launch()
        .run_test()
        .await;
}
