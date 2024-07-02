use async_compatibility_layer::art::{async_sleep, async_spawn};
use hotshot_fakeapi::fake_solver::FakeSolverState;
use hotshot_example_types::{
    node_types::TestTypes,
};
use tracing::instrument;
use url::Url;
use std::collections::HashMap;

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
#[instrument]
async fn test_fake_solver_fetch_non_permissioned_no_error() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let solver_state = FakeSolverState::new(
        None /* 0% error rate */,
        vec!["http://localhost:1111".parse().unwrap()],
    );

    // Fire up the solver.
    let solver_url: Url = "http://localhost:9000".parse().unwrap();
    let solver_handle = async_spawn(
        async move {
            solver_state.run::<TestTypes>(solver_url).await.unwrap();
        }
    );
    async_sleep(std::time::Duration::from_secs(1)).await;

    // Then, hit the API
    let resp = reqwest::get("http://localhost:9000/v0/api/auction_results/1")
        .await
        .unwrap()
        .json::<Vec<HashMap<String, String>>>()
        .await
        .unwrap();


    #[cfg(async_executor_impl = "async-std")]
    solver_handle.cancel().await;
    #[cfg(async_executor_impl = "tokio")]
    solver_handle.abort();

    assert_eq!(resp[0]["url"], "http://localhost:1111/");
}
