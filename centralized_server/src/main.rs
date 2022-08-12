use clap::Parser;
use hotshot::{
    demos::dentry::DEntryNode, traits::implementations::CentralizedServerNetwork, H_256,
};
use std::net::IpAddr;

#[async_std::main]
async fn main() {
    let args = Args::parse();
    hotshot_centralized_server::run::<DEntryNode<CentralizedServerNetwork>, H_256>(
        args.host, args.port,
    )
    .await;
}

#[derive(Parser)]
struct Args {
    pub host: IpAddr,
    pub port: u16,
}
