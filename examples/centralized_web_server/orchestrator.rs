use clap::Parser;
use jf_primitives::signatures::BLSSignatureScheme;
use std::fs;
use ark_bls12_381::Parameters as Param381;

use hotshot::{
    traits::election::{static_committee::StaticElectionConfig, vrf::JfPubKey},
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
        config_file: String,
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
    let (host, port, config_file) = match args {
        Args::Web {
            host,
            port,
            config_file,
        } => (host, port, config_file),
        _ => panic!(), // TODO ED implement other options
    };
    // Read config from file
    // TODO ED make a cli argument
    // let config_file = "./orchestrator/default-run-config.toml";
    let config: String =
        fs::read_to_string(config_file).expect("Should have been able to read the file");
    let run = toml::from_str::<NetworkConfigFile>(&config).expect("Invalid TOML");
    let mut run: NetworkConfig<JfPubKey<BLSSignatureScheme<Param381>>, StaticElectionConfig> = run.into();
    // run.centralized_web_server_config = Some(CentralizedWebServerConfig {
    //     host: web_host.as_str().parse().unwrap(),
    //     port: web_port,
    // });

    run.config.known_nodes = (0..run.config.total_nodes.get())
    .map(|node_id| {
        JfPubKey::<BLSSignatureScheme<Param381>>::generated_from_seed_indexed(
            run.seed,
            node_id.try_into().unwrap(),
        )
        .0
    })
    .collect();

    hotshot_orchestrator::run_orchestrator::< JfPubKey::<BLSSignatureScheme<Param381>>, StaticElectionConfig>(
        run,
        host.as_str().parse().unwrap(),
        port,
    )
    .await;
}
