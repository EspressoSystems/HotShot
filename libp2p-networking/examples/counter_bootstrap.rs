use structopt::StructOpt;

use color_eyre::eyre::Result;

use tracing::instrument;

use networking_demo::network_node::NetworkNodeType;

mod common;

#[async_std::main]
#[instrument]
async fn main() -> Result<()> {
    common::start_main_regular(
        common::CliOpt::from_args().idx.unwrap(),
        NetworkNodeType::Regular,
    )
    .await?;
    Ok(())
}
