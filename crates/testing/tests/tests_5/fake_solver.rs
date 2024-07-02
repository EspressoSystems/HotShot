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

    let client = reqwest::Client::new();

    // Then, hit the API
    let resp = client.get("http://localhost:9000/v0/api/auction_results/1")
        .send()
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

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
#[instrument]
async fn test_fake_solver_fetch_non_permissioned_with_errors() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let solver_state = FakeSolverState::new(
        Some(0.5),
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

    let client = reqwest::Client::new();
    let mut payloads = Vec::new();

    for i in 0..10 {
        // Then, hit the API
        let resp = client.get(format!("http://localhost:9000/v0/api/auction_results/{i}"))
            .send()
            .await;

        if let Err(ref e) = resp {
            // We want to die if we don't get a 500, because that's an unexpected error.
            assert!(e.is_status(), "Got unexpected error response; error = {e:?}");

            // Otherwise, make sure it's a 500 error
            let status = e.status().unwrap();

            // if it is, we're good to go, and we expect this.
            assert_eq!(
                status, reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                "Got unexpected error code; code = {status:?}"
            );
        }  else {
            let resp = resp.unwrap();

            if resp.status() != reqwest::StatusCode::OK {
                assert_eq!(
                    resp.status(), reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                    "Got unexpected error code; code = {:?}", resp.status(),
                );

                // Early return if it's an okay status code
                return;
            }

            let payload = resp.json::<Vec<HashMap<String, String>>>().await.unwrap();
            payloads.push(payload);
        }
    }

    #[cfg(async_executor_impl = "async-std")]
    solver_handle.cancel().await;
    #[cfg(async_executor_impl = "tokio")]
    solver_handle.abort();

    // Assert over the payloads with a 50% error rate.
    for payload in payloads {
        assert_eq!(payload[0]["url"], "http://localhost:1111/");
    }
}
