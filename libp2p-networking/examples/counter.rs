use structopt::StructOpt;

use color_eyre::eyre::Result;

use tracing::instrument;

mod common;

#[async_std::main]
#[instrument]
async fn main() -> Result<()> {
    println!("hello world we are starting");
    common::start_main(common::CliOpt::from_args().idx.unwrap()).await?;
    Ok(())
}
