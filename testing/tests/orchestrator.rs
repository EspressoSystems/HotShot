use hotshot::traits::election::static_committee::StaticElectionConfig;
use hotshot::types::ed25519::Ed25519Pub;
use nll::nll_todo;
use hotshot_orchestrator::run_orchestrator;

use hotshot_orchestrator;

#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]

async fn test_orchestrator() {
    run_orchestrator::<Ed25519Pub, StaticElectionConfig>(todo!(), todo!(), todo!()).await;
    assert!(true)
}
