use color_eyre::eyre::Result;
use common::{start_main, CliOpt};
use structopt::StructOpt;
use tracing::instrument;

pub mod common;

#[async_std::main]
#[instrument]
async fn main() -> Result<()> {
    let args = CliOpt::from_args();
    start_main(
        args.ip.unwrap(),
        args.path,
        #[cfg(feature = "webui")]
        args.webui,
    )
    .await?;

    // optional UI perhaps? for monitoring purposes
    Ok(())
}
