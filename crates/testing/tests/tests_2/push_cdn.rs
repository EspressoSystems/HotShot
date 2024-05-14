use std::time::Duration;

use hotshot_example_types::node_types::{PushCdnImpl, TestTypes};
use hotshot_testing::{
    block_builder::SimpleBuilderImplementation,
    completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription},
    overall_safety_task::OverallSafetyPropertiesDescription,
    test_builder::{TestDescription, TimingData},
};

use tracing::instrument;

/// Push CDN network test
#[tokio::test(flavor = "multi_thread")]
#[instrument]
async fn push_cdn_network() {
    hotshot_types::logging::setup_logging();
    let metadata = TestDescription {
        timing_data: TimingData {
            round_start_delay: 25,
            next_view_timeout: 10_000,
            start_delay: 120_000,

            ..Default::default()
        },
        overall_safety_properties: OverallSafetyPropertiesDescription {
            num_failed_views: 33,
            num_successful_views: 35,
            ..Default::default()
        },
        completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_secs(60),
            },
        ),
        ..TestDescription::default()
    };
    metadata
        .gen_launcher::<TestTypes, PushCdnImpl>(0)
        .launch()
        .run_test::<SimpleBuilderImplementation>()
        .await;
}
