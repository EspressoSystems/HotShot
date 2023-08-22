#[cfg(test)]
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
async fn test_timeout() {
    use std::time::Duration;

    use hotshot_testing::{
        node_types::{SequencingMemoryImpl, SequencingTestTypes},
        spinning_task::{ChangeNode, SpinningTaskDescription, UpDown},
        test_builder::{TestMetadata, TimingData},
        completion_task::{TimeBasedCompletionTaskDescription, CompletionTaskDescription},
    };

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();
    let mut timing_data = TimingData::default(); 
    timing_data.next_view_timeout = 1000;
    let mut metadata = TestMetadata::default();
    let dead_nodes = vec![ChangeNode {
        idx: 2,
        updown: UpDown::Down,
    }];

    metadata.timing_data = timing_data; 

    metadata.spinning_properties = SpinningTaskDescription {
        node_changes: vec![(Duration::new(1, 0), dead_nodes)],
    };
    metadata.completion_task_description = CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
        TimeBasedCompletionTaskDescription {
            duration: Duration::from_millis(30000),
        },
    );
    metadata
        .gen_launcher::<SequencingTestTypes, SequencingMemoryImpl>()
        .launch()
        .run_test()
        .await;
}

