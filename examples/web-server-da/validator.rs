use clap::Parser;
use hotshot::demos::sdemo::SDemoTypes;
use tracing::instrument;

use crate::{
    infraDA::{main_entry_point},
    types::{NodeImpl, ThisMembership, DANetwork, QuorumNetwork, ThisRun, },
};

use crate::infra::{ValidatorArgs};

pub mod types;

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
    let args = ValidatorArgs::parse();
    main_entry_point::<SDemoTypes, ThisMembership, DANetwork, QuorumNetwork, NodeImpl, ThisRun>(args).await;
}
