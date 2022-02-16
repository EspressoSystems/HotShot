use common::{start_main_controller, CliOpt};

use structopt::StructOpt;

use color_eyre::eyre::Result;

use tracing::instrument;

mod common;

#[async_std::main]
#[instrument]
async fn main() -> Result<()> {
    start_main_controller(CliOpt::from_args().idx.unwrap()).await?;
    Ok(())
}
