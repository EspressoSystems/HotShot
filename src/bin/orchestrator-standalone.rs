use clap::Parser;
use std::fs;

use hotshot::{
    traits::election::static_committee::StaticElectionConfig,
    types::{
        ed25519::{Ed25519Priv, Ed25519Pub},
        SignatureKey,
    },
};
use hotshot_orchestrator::{
    self,
    config::{CentralizedWebServerConfig, NetworkConfig, NetworkConfigFile},
};

#[derive(clap::Parser)]
enum Args {
    Centralized {
        host: String,
        port: u16,
    },
    Web {
        host: String,
        port: u16,
        web_host: String,
        web_port: u16,
    },
    Libp2p {
        host: String,
        port: u16,
    },
}

// TODO ED integrate old centralized server and libp2p
#[async_std::main]
pub async fn main() {
    let args = Args::parse();
    let (host, port, web_host, web_port) = match args {
        Args::Web {
            host,
            port,
            web_host,
            web_port,
        } => (host, port, web_host, web_port),
        _ => panic!(), // TODO ED implement other options
    };
    // Read config from file
    // TODO ED make a cli argument
    let config_file = "./orchestrator/default-run-config.toml";
    let config: String =
        fs::read_to_string(config_file).expect("Should have been able to read the file");
    let run = toml::from_str::<NetworkConfigFile>(&config).expect("Invalid TOML");
    let mut run: NetworkConfig<Ed25519Pub, StaticElectionConfig> = run.into();
    run.centralized_web_server_config = Some(CentralizedWebServerConfig {
        host: web_host.as_str().parse().unwrap(),
        port: web_port,
    });

    run.config.known_nodes = (0..run.config.total_nodes.get())
        .map(|node_id| {
            let private_key = Ed25519Priv::generated_from_seed_indexed(run.seed, node_id as u64);
            Ed25519Pub::from_private(&private_key)
        })
        .collect();

    hotshot_orchestrator::run_orchestrator::<Ed25519Pub, StaticElectionConfig>(
        run,
        host.as_str().parse().unwrap(),
        port,
    )
    .await;
}
