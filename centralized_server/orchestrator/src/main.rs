use clap::Parser;
use hotshot::types::{
    ed25519::{Ed25519Priv, Ed25519Pub},
    SignatureKey,
};
use hotshot_centralized_server::{
    config::{HotShotConfigFile, Libp2pConfigFile, NetworkConfigFile, RoundConfig},
    NetworkConfig, Server,
};
use hotshot_utils::art::async_main;
use hotshot_utils::test_util::setup_logging;
use std::{fs, num::NonZeroUsize, time::Duration};

#[derive(clap::Parser)]
enum Args {
    Centralized { host: String, port: u16 },
    Libp2p { host: String, port: u16 },
}

#[async_main]
async fn main() {
    setup_logging();
    let args = Args::parse();

    let (is_libp2p, host, port) = match args {
        Args::Centralized { host, port } => (false, host, port),
        Args::Libp2p { host, port } => (true, host, port),
    };
    let configs = load_configs(is_libp2p).expect("Could not load configs");

    Server::<Ed25519Pub>::new(host.parse().expect("Invalid host address"), port)
        .await
        .with_round_config(RoundConfig::new(configs))
        .run()
        .await;
}

fn load_configs(is_libp2p: bool) -> std::io::Result<Vec<NetworkConfig<Ed25519Pub>>> {
    let mut result = Vec::new();
    for file in fs::read_dir(".")? {
        let file = file?;
        if let Some(name) = file.path().extension() {
            if name == "toml" && file.path().file_name().unwrap() != "Cargo.toml" {
                println!(
                    "Loading {:?} (run {})",
                    file.path().as_os_str(),
                    result.len() + 1
                );
                let str = fs::read_to_string(file.path())?;
                let run = toml::from_str::<NetworkConfigFile>(&str).expect("Invalid TOML");
                let mut run: NetworkConfig<Ed25519Pub> = run.into();

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
        let toml = toml::to_string_pretty(&NetworkConfigFile {
            config: HotShotConfigFile {
                total_nodes: NonZeroUsize::new(10).unwrap(),
                threshold: NonZeroUsize::new(7).unwrap(),
                max_transactions: NonZeroUsize::new(100).unwrap(),
                next_view_timeout: 10000,
                timeout_ratio: (11, 10),
                round_start_delay: 1,
                start_delay: 1,
                propose_min_round_time: Duration::from_secs(0),
                propose_max_round_time: Duration::from_secs(1),
                num_bootstrap: 4,
            },
            node_index: 0,
            rounds: 100,
            seed: [0u8; 32],
            transactions_per_round: 10,
            libp2p_config: if is_libp2p {
                Some(Libp2pConfigFile {
                    bootstrap_mesh_n_high: 4,
                    bootstrap_mesh_n_low: 4,
                    bootstrap_mesh_outbound_min: 2,
                    bootstrap_mesh_n: 4,
                    mesh_n_high: 4,
                    mesh_n_low: 4,
                    mesh_outbound_min: 2,
                    mesh_n: 4,
                    next_view_timeout: 10,
                    propose_min_round_time: 0,
                    propose_max_round_time: 10,
                    online_time: 10,
                    num_txn_per_round: 10,
                    base_port: 2346,
                })
            } else {
                None
            },
            padding: 10,
            start_delay_seconds: 60,
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
