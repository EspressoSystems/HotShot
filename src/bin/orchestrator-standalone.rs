use std::fs;

use hotshot::{
    traits::election::static_committee::StaticElectionConfig,
    types::{
        ed25519::{Ed25519Priv, Ed25519Pub},
        SignatureKey,
    },
};
use hotshot_orchestrator::{self, config::{NetworkConfig, NetworkConfigFile}};

#[async_std::main]
pub async fn main()  {
    // Read config from file
    let config_file = "./orchestrator/default-run-config.toml";
    let config: String = fs::read_to_string(config_file).expect("Should have been able to read the file");
    let run = toml::from_str::<NetworkConfigFile>(&config).expect("Invalid TOML");
    let mut run: NetworkConfig<Ed25519Pub, StaticElectionConfig> = run.into();

    // Generate keys / seeds and update config
    // Create new server
    // server.run.await

    hotshot_orchestrator::run_orchestrator::<Ed25519Pub, StaticElectionConfig>(run).await;
}
