pub mod types;

use clap::Parser;
use hotshot::demos::vdemo::VDemoTypes;
use tracing::instrument;
use types::ThisElection;

use crate::{
    infra::{run_orchestrator, CliOrchestrated},
    types::{ThisNetwork, ThisNode},
};

#[path = "../infra/mod.rs"]
pub mod infra;

#[cfg_attr(
    feature = "tokio-executor",
    tokio::main(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::main)]
#[instrument]
async fn main() {
    let args = CliOrchestrated::parse();

    run_orchestrator::<VDemoTypes, ThisElection, ThisNetwork, ThisNode>(args).await;
}
