use async_compatibility_layer::logging::shutdown_logging;
use hotshot_testing::{node_types::{SequencingTestTypes, SequencingWebImpl}, test_builder::{TestMetadata, TimingData}};
use tracing::instrument;

/// Web server network test
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn web_server_network() {
    let metadata = TestMetadata {
        timing_data: TimingData {
            round_start_delay: 25,
            next_view_timeout: 3000,
            start_delay: 120000,
            ..Default::default()
        },
        ..TestMetadata::default()
    };
    // TODO web server network doesn't implement TestableNetworkingImplementation
    metadata
        .gen_launcher::<SequencingTestTypes, SequencingWebImpl>()
        .launch()
        .run_test()
        .await;
    shutdown_logging();
}
