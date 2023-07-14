pub mod types;

use async_compatibility_layer::logging::{setup_backtrace, setup_logging};
use clap::Parser;
use hotshot::demos::sdemo::SDemoTypes;
use tracing::instrument;
use types::ThisMembership;

use crate::infra::OrchestratorArgs;
use crate::infra_da::run_orchestrator_da;
use crate::types::{DANetwork, NodeImpl, QuorumNetwork, ViewSyncNetwork};

#[path = "../infra/mod.rs"]
pub mod infra;
#[path = "../infra/modDA.rs"]
pub mod infra_da;

#[cfg_attr(
    feature = "tokio-executor",
    tokio::main(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::main)]
#[instrument]
async fn main() {
    setup_logging();
    setup_backtrace();
    let args = OrchestratorArgs::parse();


    run_orchestrator_da::<SDemoTypes, ThisMembership, DANetwork, QuorumNetwork, ViewSyncNetwork, NodeImpl>(args)
        .await;
}