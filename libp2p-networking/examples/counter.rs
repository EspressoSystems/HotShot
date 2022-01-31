use libp2p::Multiaddr;

use structopt::StructOpt;

use color_eyre::eyre::{Result, WrapErr};

use tracing::instrument;

use networking_demo::{gen_multiaddr, NetworkNode};

/// command line arguments
#[derive(StructOpt)]
struct CliOpt {
    /// Path to the node configuration file
    #[structopt(long = "port", short = "p")]
    port: Option<u16>,

    #[structopt()]
    first_dial: Option<Multiaddr>,
}

#[async_std::main]
#[instrument]
async fn main() -> Result<()> {
    todo!()
}
