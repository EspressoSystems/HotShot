use std::fs;

use hotshot::{
    traits::election::static_committee::StaticElectionConfig,
    types::{
        ed25519::{Ed25519Priv, Ed25519Pub},
        SignatureKey,
    },
};
use hotshot_orchestrator::{self, config::{NetworkConfig, NetworkConfigFile, CentralizedWebServerConfig}};

#[async_std::main]
pub async fn main()  {
    // Read config from file
    let config_file = "./orchestrator/default-run-config.toml";
    let config: String = fs::read_to_string(config_file).expect("Should have been able to read the file");
    let run = toml::from_str::<NetworkConfigFile>(&config).expect("Invalid TOML");
    let mut run: NetworkConfig<Ed25519Pub, StaticElectionConfig> = run.into();
    run.centralized_web_server_config = Some(CentralizedWebServerConfig {
        host: "127.0.0.1".parse().unwrap(),
        port: 8080,
    });


    run.config.known_nodes = (0..run.config.total_nodes.get())
    .map(|node_id| {
        let private_key =
            Ed25519Priv::generated_from_seed_indexed(run.seed, node_id as u64);
        Ed25519Pub::from_private(&private_key)
    })
    .collect();

    // Generate keys / seeds and update config
    // Create new server
    // server.run.await

    hotshot_orchestrator::run_orchestrator::<Ed25519Pub, StaticElectionConfig>(run).await;
}
