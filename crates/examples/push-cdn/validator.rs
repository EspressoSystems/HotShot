//! A validator
use async_compatibility_layer::logging::{setup_backtrace, setup_logging};
use clap::Parser;
use hotshot_example_types::state_types::TestTypes;
use hotshot_orchestrator::client::ValidatorArgs;
use tracing::{debug, instrument};

use crate::types::{Network, NodeImpl, ThisRun};

/// types used for this example
pub mod types;

/// general infra used for this example
#[path = "../infra/mod.rs"]
pub mod infra;

#[cfg_attr(async_executor_impl = "tokio", tokio::main(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::main)]
#[instrument]
async fn main() {
    setup_logging();
    setup_backtrace();
    let args = ValidatorArgs::parse();
    debug!("connecting to orchestrator at {:?}", args.url);
    infra::main_entry_point::<TestTypes, Network, NodeImpl, ThisRun>(args).await;
}
