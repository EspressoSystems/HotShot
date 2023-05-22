use crate::infra::{main_entry_point, ValidatorArgs};
use clap::Parser;
use hotshot::demos::vdemo::VDemoTypes;
use tracing::instrument;

use crate::types::{NodeImpl, ThisMembership, ThisNetwork, ThisRun};

pub mod types;

#[path = "../infra/mod.rs"]
pub mod infra;

#[cfg_attr(
    feature = "tokio-executor",
    tokio::main(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::main)]
#[instrument]
async fn main() {
    let args = ValidatorArgs::parse();
    main_entry_point::<VDemoTypes, ThisMembership, ThisNetwork, NodeImpl, ThisRun>(args).await;
}
