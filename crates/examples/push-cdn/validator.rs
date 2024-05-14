//! A validator using the web server

use clap::Parser;
use hotshot_example_types::state_types::TestTypes;
use hotshot_orchestrator::client::ValidatorArgs;
use tracing::{info, instrument};

use crate::types::{DaNetwork, NodeImpl, QuorumNetwork, ThisRun};

/// types used for this example
pub mod types;

/// general infra used for this example
#[path = "../infra/mod.rs"]
pub mod infra;

#[tokio::main]
#[instrument]
async fn main() {
    hotshot_types::logging::setup_logging();

    let args = ValidatorArgs::parse();
    info!("connecting to orchestrator at {:?}", args.url);
    infra::main_entry_point::<TestTypes, DaNetwork, QuorumNetwork, NodeImpl, ThisRun>(args).await;
}
