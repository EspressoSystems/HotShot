use async_std::{fs, prelude::StreamExt};
use clap::Parser;
use hotshot::types::{
    ed25519::{Ed25519Priv, Ed25519Pub},
    SignatureKey,
};
use hotshot_centralized_server::{NetworkConfig, RoundConfig, Server};
use hotshot_types::{ExecutionType, HotShotConfig};
use hotshot_utils::test_util::setup_logging;
use std::{net::IpAddr, num::NonZeroUsize, time::Duration};

#[async_std::main]
async fn main() {
    setup_logging();
    let configs = load_configs().await.expect("Could not load configs");
    let args = Args::parse();
    Server::<Ed25519Pub>::new(args.host, args.port)
        .await
        .with_round_config(RoundConfig::new(configs))
        .run()
        .await;
}

async fn load_configs() -> std::io::Result<Vec<NetworkConfig<Ed25519Pub>>> {
    let mut result = Vec::new();
    let mut files = fs::read_dir(".").await?;
    while let Some(Ok(file)) = files.next().await {
        if let Some(name) = file.path().extension() {
            if name == "toml" && file.path().file_name().unwrap() != "Cargo.toml" {
                println!(
                    "Loading {:?} (run {})",
                    file.path().as_os_str(),
                    result.len() + 1
                );
                let str = fs::read_to_string(file.path()).await?;
                let mut run =
                    toml::from_str::<NetworkConfig<Ed25519Pub>>(&str).expect("Invalid TOML");

                run.config.known_nodes = (0..run.config.total_nodes.get())
                    .map(|node_id| {
                        let private_key =
                            Ed25519Priv::generated_from_seed_indexed(run.seed, node_id as u64);
                        Ed25519Pub::from_private(&private_key)
                    })
                    .collect();

                result.push(run);
            }
        }
    }

    if result.is_empty() {
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
        println!("Written data to config.toml");
        println!("Please edit parameters in this file and re-run the server");
        println!("For multiple runs, please make multiple *.toml files in this folder with valid configs");
        std::process::exit(0);
    }

    Ok(result)
}

#[derive(Parser)]
struct Args {
    pub host: IpAddr,
    pub port: u16,
}
