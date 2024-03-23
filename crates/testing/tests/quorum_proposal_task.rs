#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_quorum_proposal_task() {
    use hotshot::types::SystemContextHandle;
    use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
    use hotshot_task_impls::quorum_proposal::QuorumProposalTaskState;
    use hotshot_testing::task_helpers::build_system_handle;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(2).await.0;

    let quorum_proposal_task_state = QuorumProposalTaskState::<
        TestTypes,
        MemoryImpl,
        SystemContextHandle<TestTypes, MemoryImpl>,
    >::create_from(&handle);
}
