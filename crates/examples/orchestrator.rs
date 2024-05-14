//! A orchestrator using the web server

use hotshot_example_types::state_types::TestTypes;
use tracing::instrument;

use crate::infra::{read_orchestrator_init_config, run_orchestrator, OrchestratorArgs};

/// general infra used for this example
#[path = "./infra/mod.rs"]
pub mod infra;

#[tokio::main]
#[instrument]
async fn main() {
    hotshot_types::logging::setup_logging();
    
    let (config, orchestrator_url) = read_orchestrator_init_config::<TestTypes>();
    run_orchestrator::<TestTypes>(OrchestratorArgs::<TestTypes> {
        url: orchestrator_url.clone(),
        config: config.clone(),
    })
    .await;
}
