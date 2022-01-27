use libp2p::Multiaddr;

use structopt::StructOpt;

use color_eyre::eyre::{Result, WrapErr};

use tracing::instrument;

use networking_demo::{gen_multiaddr, Network};

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
    // -- Setup color_eyre and tracing
    color_eyre::install()?;
    networking_demo::tracing_setup::setup_tracing();
    // -- Spin up the network connection
    let mut networking: Network = Network::new().await.context("Failed to launch network")?;
    let port = CliOpt::from_args().port.unwrap_or(0u16);
    let known_peer = CliOpt::from_args().first_dial;
    let listen_addr = gen_multiaddr(port);
    networking.start(listen_addr, known_peer).await?;
    let (_send_chan, _recv_chan) = networking.spawn_listeners().await?;
    todo!()
}
