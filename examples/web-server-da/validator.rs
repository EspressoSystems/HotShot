use async_compatibility_layer::logging::setup_logging;
use clap::Parser;
use hotshot::demos::sdemo::SDemoTypes;
use tracing::instrument;

use crate::types::{DANetwork, NodeImpl, QuorumNetwork, ThisMembership, ThisRun};

use crate::infra::ValidatorArgs;

pub mod types;

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
    let args = ValidatorArgs::parse();
    tracing::error!(
        "connecting to orchestrator at {:?}:{:?}",
        args.host,
        args.port
    );
    infraDA::main_entry_point::<
        SDemoTypes,
        ThisMembership,
        DANetwork,
        QuorumNetwork,
        NodeImpl,
        ThisRun,
    >(args)
    .await;
}
