#[cfg(test)]
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
async fn test_basic() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();
    let metadata = hotshot_testing::test_builder::TestMetadata::default();
    metadata
        .gen_launcher::<hotshot_testing::node_types::SequencingTestTypes, hotshot_testing::node_types::SequencingMemoryImpl>()
        .launch()
        .run_test()
        .await;
}

#[cfg(test)]
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
async fn test_with_failures() {
    use std::time::Duration;

    use hotshot_testing::{
        completion_task::TimeBasedCompletionTaskDescription, spinning_task::SpinningTaskDescription,
    };

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();
    let mut metadata = hotshot_testing::test_builder::TestMetadata {
        total_nodes: 20,
        start_nodes: 20,
        num_bootstrap_nodes: 20,
        completion_task_description: hotshot_testing::completion_task::CompletionTaskDescription::TimeBasedCompletionTaskBuilder(TimeBasedCompletionTaskDescription{duration: Duration::new(120, 0)}),
        // overall_safety_properties: OverallSafetyPropertiesDescription {
        //     threshold_calculator: std::sync::Arc::new(|_, _| {10}),
        //     ..Default::default()
        // },
        ..hotshot_testing::test_builder::TestMetadata::default()
    };
    let dead_nodes = vec![
        hotshot_testing::spinning_task::ChangeNode {
            idx: 0,
            updown: hotshot_testing::spinning_task::UpDown::Down,
        },
        hotshot_testing::spinning_task::ChangeNode {
            idx: 1,
            updown: hotshot_testing::spinning_task::UpDown::Down,
        },
        hotshot_testing::spinning_task::ChangeNode {
            idx: 2,
            updown: hotshot_testing::spinning_task::UpDown::Down,
        },
        hotshot_testing::spinning_task::ChangeNode {
            idx: 3,
            updown: hotshot_testing::spinning_task::UpDown::Down,
        },
        hotshot_testing::spinning_task::ChangeNode {
            idx: 4,
            updown: hotshot_testing::spinning_task::UpDown::Down,
        },
    ];

    metadata.spinning_properties = SpinningTaskDescription {
        node_changes: vec![(std::time::Duration::new(4, 0), dead_nodes)],
    };
    metadata
        .gen_launcher::<hotshot_testing::node_types::SequencingTestTypes, hotshot_testing::node_types::SequencingMemoryImpl>()
        .launch()
        .run_test()
        .await;
}
