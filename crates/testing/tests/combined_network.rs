use std::time::Duration;

use hotshot_testing::{
    completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription},
    node_types::{CombinedImpl, TestTypes},
    overall_safety_task::OverallSafetyPropertiesDescription,
    spinning_task::{ChangeNode, SpinningTaskDescription, UpDown},
    test_builder::{TestMetadata, TimingData},
};
use tracing::instrument;

/// web server with libp2p network test
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
#[instrument]
async fn test_combined_network_sad() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();
    let mut metadata: TestMetadata = TestMetadata {
        timing_data: TimingData {
            round_start_delay: 25,
            next_view_timeout: 10000,
            start_delay: 120000,

            ..Default::default()
        },
        overall_safety_properties: OverallSafetyPropertiesDescription {
            num_successful_views: 35,
            ..Default::default()
        },
        // allow more time to pass in CI
        completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_millis(1_200_000),
            },
        ),
        ..TestMetadata::default_multiple_rounds()
    };

    let mut all_nodes = vec![];
    for node in 0..metadata.total_nodes {
        all_nodes.push(ChangeNode {
            idx: node,
            updown: UpDown::NetworkDown,
        });
    }

    metadata.spinning_properties = SpinningTaskDescription {
        node_changes: vec![(Duration::new(4, 0), all_nodes)],
    };

    metadata
        .gen_launcher::<TestTypes, CombinedImpl>()
        .launch()
        .run_test()
        .await
}

// stress test for web server with libp2p
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
#[instrument]
#[ignore]
async fn test_stress_combined_network() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();
    let metadata = TestMetadata::default_stress();
    metadata
        .gen_launcher::<TestTypes, CombinedImpl>()
        .launch()
        .run_test()
        .await
}
