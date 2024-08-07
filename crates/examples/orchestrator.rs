// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

//! A orchestrator

use async_compatibility_layer::logging::{setup_backtrace, setup_logging};
use hotshot_example_types::state_types::TestTypes;
use tracing::instrument;

use crate::infra::{read_orchestrator_init_config, run_orchestrator, OrchestratorArgs};

/// general infra used for this example
#[path = "./infra/mod.rs"]
pub mod infra;

#[cfg_attr(async_executor_impl = "tokio", tokio::main(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::main)]
#[instrument]
async fn main() {
    setup_logging();
    setup_backtrace();
    let (config, orchestrator_url) = read_orchestrator_init_config::<TestTypes>();
    run_orchestrator::<TestTypes>(OrchestratorArgs::<TestTypes> {
        url: orchestrator_url.clone(),
        config: config.clone(),
    })
    .await;
}
