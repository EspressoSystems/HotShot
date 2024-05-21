#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_builder_failures() {
    use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
    use hotshot_testing::{
        block_builder::SimpleBuilderImplementation,
        test_builder::{BuilderChange, BuilderDescription, TestDescription},
    };

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let mut metadata = TestDescription::default_multiple_rounds();
    metadata.builders = vec1::vec1![
        BuilderDescription {
            changes: [
                (0, BuilderChange::Down),
                (15, BuilderChange::Up),
                (30, BuilderChange::FailClaims(true)),
            ]
            .into(),
        },
        BuilderDescription {
            changes: [(15, BuilderChange::Down), (30, BuilderChange::Up),].into(),
        },
    ];

    metadata
        .gen_launcher::<TestTypes, MemoryImpl>(0)
        .launch()
        .run_test::<SimpleBuilderImplementation>()
        .await;
}
