//! The marshal is the component of the CDN that authenticates users and routes
//! them to the appropriate broker
//!
//! This is meant to be run externally, e.g. when running benchmarks on the protocol.
//! If you just want to run everything required, you can use the `all` example

use anyhow::{Context, Result};
use cdn_marshal::{Config, Marshal};
use clap::Parser;
use hotshot::{helpers::initialize_logging, traits::implementations::ProductionDef};
use hotshot_example_types::node_types::TestTypes;
use hotshot_types::traits::node_implementation::NodeType;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
/// The main component of the push CDN.
struct Args {
    /// The discovery client endpoint (including scheme) to connect to.
    /// This is a URL pointing to a `KeyDB` database (e.g. `redis://127.0.0.1:6789`).
    #[arg(short, long)]
    discovery_endpoint: String,

    /// The port to bind to for connections (from users)
    #[arg(short, long, default_value_t = 1737)]
    bind_port: u16,

    /// The endpoint to bind to for externalizing metrics (in `IP:port` form). If not provided,
    /// metrics are not exposed.
    #[arg(short, long)]
    metrics_bind_endpoint: Option<String>,

    /// The path to the CA certificate
    /// If not provided, a local, pinned CA is used
    #[arg(long)]
    ca_cert_path: Option<String>,

    /// The path to the CA key
    /// If not provided, a local, pinned CA is used
    #[arg(long)]
    ca_key_path: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command-line arguments
    let args = Args::parse();

    // Initialize tracing
    initialize_logging();

    // Create a new `Config`
    let config = Config {
        discovery_endpoint: args.discovery_endpoint,
        bind_endpoint: format!("0.0.0.0:{}", args.bind_port),
        metrics_bind_endpoint: args.metrics_bind_endpoint,
        ca_cert_path: args.ca_cert_path,
        ca_key_path: args.ca_key_path,
        // Use a 1GB memory pool
        global_memory_pool_size: Some(1_073_741_824),
    };

    // Create new `Marshal` from the config
    let marshal =
        Marshal::<ProductionDef<<TestTypes as NodeType>::SignatureKey>>::new(config).await?;

    // Start the main loop, consuming it
    marshal.start().await.with_context(|| "Marshal exited")
}
