use clap::Parser;
use hotshot::demos::vdemo::VDemoTypes;
use tracing::instrument;

use crate::{
    infra::{main_entry_point, ValidatorArgs},
    types::{ThisMembership, ThisNetwork, ThisNode, ThisRun},
};

pub mod types;

#[path = "../infra2/mod.rs"]
pub mod infra;

#[cfg_attr(
    feature = "tokio-executor",
    tokio::main(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::main)]
#[instrument]
async fn main() {
    let args = ValidatorArgs::parse();
    main_entry_point::<VDemoTypes, ThisMembership, ThisNetwork, ThisNode, ThisRun>(args).await;
}
