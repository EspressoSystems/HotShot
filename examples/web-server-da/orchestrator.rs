pub mod types;

use clap::Parser;
use hotshot::demos::sdemo::SDemoTypes;
use tracing::instrument;
use types::ThisMembership;

use crate::infra::{OrchestratorArgs};
use crate::infraDA::run_orchestrator_da;
use crate::types::{NodeImpl, DANetwork, QuorumNetwork};

#[path = "../infra/modDA.rs"]
pub mod infraDA;
#[path = "../infra/mod.rs"]
pub mod infra;

#[cfg_attr(
    feature = "tokio-executor",
    tokio::main(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::main)]
#[instrument]
async fn main() {
    let args = OrchestratorArgs::parse();

    run_orchestrator_da::<SDemoTypes, ThisMembership, DANetwork, QuorumNetwork, NodeImpl>(args).await;
}
