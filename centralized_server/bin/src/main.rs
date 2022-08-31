use clap::Parser;
use hotshot::types::{
    ed25519::{Ed25519Priv, Ed25519Pub},
    SignatureKey,
};
use hotshot_centralized_server::{NetworkConfig, Server};
use hotshot_types::{ExecutionType, HotShotConfig};
use hotshot_utils::test_util::setup_logging;
use std::{net::IpAddr, num::NonZeroUsize, time::Duration};
use tracing::{error};

#[async_std::main]
async fn main() {
    setup_logging();
    let config = load_config();
    let args = Args::parse();
    Server::<Ed25519Pub>::new(args.host, args.port)
        .await
        .with_network_config(config)
        .run()
        .await;
}

fn load_config() -> NetworkConfig<Ed25519Pub> {
    if let Ok(str) = std::fs::read_to_string("config.toml") {
        if let Ok(mut decoded) = toml::from_str::<NetworkConfig<Ed25519Pub>>(&str) {
            decoded.config.known_nodes = (0..decoded.config.total_nodes.get())
                .map(|node_id| {
                    let private_key =
                        Ed25519Priv::generated_from_seed_indexed(decoded.seed, node_id as u64);
                    Ed25519Pub::from_private(&private_key)
                })
                .collect();
            return decoded;
        }
    }
    let toml = toml::to_string_pretty(&NetworkConfig {
        config: HotShotConfig::<Ed25519Pub> {
            execution_type: ExecutionType::Continuous,
            total_nodes: NonZeroUsize::new(10).unwrap(),
            threshold: NonZeroUsize::new(7).unwrap(),
            max_transactions: NonZeroUsize::new(100).unwrap(),
            known_nodes: vec![],
            next_view_timeout: 10000,
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            propose_min_round_time: Duration::from_secs(0),
            propose_max_round_time: Duration::from_secs(1),
            num_bootstrap: 0,
        },
        node_index: 0,
        rounds: 100,
        seed: [0u8; 32],
        transactions_per_round: 10,
        padding: 10,
    })
    .expect("Could not serialize to TOML");
    std::fs::write("config.toml", toml).expect("Could not write config.toml");
    error!("config.toml created, please edit this and restart");
    std::process::exit(0);
}

#[derive(Parser)]
struct Args {
    pub host: IpAddr,
    pub port: u16,
}
