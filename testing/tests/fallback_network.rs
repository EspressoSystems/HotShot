use hotshot_testing::{test_builder::TestMetadata, node_types::{SequencingTestTypes, StaticFallbackImpl, SequencingLibp2pImpl}};
use tracing::instrument;

/// web server with libp2p network test
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn webserver_libp2p_network() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();
    let metadata = TestMetadata::default_multiple_rounds();
    metadata.gen_launcher::<SequencingTestTypes, SequencingLibp2pImpl>()
        .launch()
        .run_test()
        .await
}

// stress test for web server with libp2p
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn test_stress_webserver_libp2p_network() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();
    let metadata = TestMetadata::default_stress();
    metadata.gen_launcher::<SequencingTestTypes, SequencingLibp2pImpl>()
        .launch()
        .run_test()
        .await
}
