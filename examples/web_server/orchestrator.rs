use ark_bls12_381::Parameters as Param381;
use clap::Parser;
use jf_primitives::signatures::BLSSignatureScheme;
use std::fs;

use hotshot::{
    traits::election::{static_committee::StaticElectionConfig, vrf::JfPubKey},
    types::SignatureKey,
};
use hotshot_orchestrator::{
    self,
    config::{NetworkConfig, NetworkConfigFile},
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

#[async_std::main]
pub async fn main() {
    let args = Args::parse();
    // TODO ED: Implement libp2p and old server for this orchestrator
    let (host, port, config_file) = match args {
        Args::Web {
            host,
            port,
            config_file,
        } => (host, port, config_file),
        _ => panic!(),
    };

    let config: String =
        fs::read_to_string(config_file).expect("Should have been able to read the file");
    let run = toml::from_str::<NetworkConfigFile>(&config).expect("Invalid TOML");
    let mut run: NetworkConfig<JfPubKey<BLSSignatureScheme<Param381>>, StaticElectionConfig> =
        run.into();

    run.config.known_nodes = (0..run.config.total_nodes.get())
        .map(|node_id| {
            JfPubKey::<BLSSignatureScheme<Param381>>::generated_from_seed_indexed(
                run.seed,
                node_id.try_into().unwrap(),
            )
            .0
        })
        .collect();

    let _result = hotshot_orchestrator::run_orchestrator::<
        JfPubKey<BLSSignatureScheme<Param381>>,
        StaticElectionConfig,
    >(run, host.as_str().parse().unwrap(), port)
    .await;
}
