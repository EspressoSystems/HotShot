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
        .await
        .unwrap();
}
