use structopt::StructOpt;

use color_eyre::eyre::Result;

use tracing::instrument;

mod common;

use common::{start_main, CliOpt};

#[async_std::main]
#[instrument]
async fn main() -> Result<()> {
    let args = CliOpt::from_args();
    start_main(args.ip.unwrap(), args.path).await?;

    // optional UI perhaps? for monitoring purposes
    Ok(())
}
