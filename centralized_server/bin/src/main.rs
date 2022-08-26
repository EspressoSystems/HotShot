use clap::Parser;
use hotshot::types::ed25519::Ed25519Pub;
use hotshot_centralized_server::Server;
use hotshot_utils::test_util::setup_logging;
use std::net::IpAddr;

#[async_std::main]
async fn main() {
    setup_logging();
    let args = Args::parse();
    Server::<Ed25519Pub>::new(args.host, args.port)
        .await
        .run()
        .await;
}

#[derive(Parser)]
struct Args {
    pub host: IpAddr,
    pub port: u16,
}
