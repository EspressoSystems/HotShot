use clap::Parser;
use hotshot::demos::vdemo::VDemoTypes;
use tracing::instrument;

pub mod types;

use types::ThisElection;

#[path = "../infra/mod.rs"]
pub mod infra;

use infra::main_entry_point;
use infra::CliOrchestrated;

use crate::types::ThisConfig;
use crate::types::ThisNetwork;
use crate::types::ThisNode;

#[cfg_attr(
    feature = "tokio-executor",
    tokio::main(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::main)]
#[instrument]
async fn main() {
    let args = CliOrchestrated::parse();

    main_entry_point::<VDemoTypes, ThisElection, ThisNetwork, ThisNode, ThisConfig>(args).await;
}
