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

    use hotshot_testing::{spinning_task::SpinningTaskDescription, overall_safety_task::OverallSafetyPropertiesDescription, completion_task::{TimeBasedCompletionTaskDescription, CompletionTaskDescription}};

    // TODO add test with icnreased DA committee size
    // DA comimttee where all nodes are in the DA committee
    // none of DA comitee members get shut down
    // half of DA committee gets shut down

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();
    let mut metadata = hotshot_testing::test_builder::TestMetadata {
        timing_data: hotshot_testing::test_builder::TimingData {
            next_view_timeout: 1000,
            ..Default::default()
        },
        total_nodes: 20,
        start_nodes: 20,
        num_bootstrap_nodes: 20,
        overall_safety_properties: OverallSafetyPropertiesDescription {
            check_block: false,
            ..Default::default()
        },
        completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(TimeBasedCompletionTaskDescription { duration: Duration::new(90, 0) })
        ,

        ..hotshot_testing::test_builder::TestMetadata::default()
    };
    let dead_nodes = vec![
        hotshot_testing::spinning_task::ChangeNode {
            idx: 10,
            updown: hotshot_testing::spinning_task::UpDown::Down,
        },
        hotshot_testing::spinning_task::ChangeNode {
            idx: 11,
            updown: hotshot_testing::spinning_task::UpDown::Down,
        },
        hotshot_testing::spinning_task::ChangeNode {
            idx: 12,
            updown: hotshot_testing::spinning_task::UpDown::Down,
        },
        hotshot_testing::spinning_task::ChangeNode {
            idx: 13,
            updown: hotshot_testing::spinning_task::UpDown::Down,
        },
        hotshot_testing::spinning_task::ChangeNode {
            idx: 14,
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
