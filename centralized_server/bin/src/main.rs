use clap::Parser;
use hotshot::types::ed25519::Ed25519Pub;
use hotshot_centralized_server::Server;
use std::net::IpAddr;

#[async_std::main]
async fn main() {
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
