pub mod types;

use crate::infra::load_config_from_file;
use crate::infra::{ConfigArgs, OrchestratorArgs};
use crate::types::ThisRun;
use crate::{
    infra::run_orchestrator,
    types::{DANetwork, NodeImpl, QuorumNetwork, ViewSyncNetwork},
};
use std::net::{IpAddr, Ipv4Addr};
#[path = "../infra/mod.rs"]
pub mod infra;

use async_compatibility_layer::art::async_spawn;
use clap::Parser;
use hotshot_orchestrator::client::ValidatorArgs;
use hotshot_orchestrator::config::NetworkConfig;
use hotshot_testing::state_types::TestTypes;
use hotshot_types::traits::node_implementation::NodeType;
use server::{Config, Server};
use surf_disco::Url;
use types::VIDNetwork;

#[cfg_attr(async_executor_impl = "tokio", tokio::main)]
#[cfg_attr(async_executor_impl = "async-std", async_std::main)]
async fn main() {
    // use configfile args
    let args = ConfigArgs::parse();

    let config: NetworkConfig<
        <TestTypes as NodeType>::SignatureKey,
        <TestTypes as NodeType>::ElectionConfigType,
    > = load_config_from_file::<TestTypes>(args.config_file.clone());

    async_spawn(async move {
        // create the server
        let server = Server::new(Config {
            bind_address: config.push_cdn_address.clone().unwrap(),
            public_advertise_address: config.push_cdn_address.clone().unwrap(),
            private_advertise_address: config.push_cdn_address.unwrap(),
            redis_url: "redis://127.0.0.1:6379".to_string(),
            redis_password: "".to_string(),
            tls_cert_path: None,
            tls_key_path: None,
            signing_key: None,
        })
        .unwrap();

        // run the server
        server.run().await.unwrap();
    });

    let orchestrator_url = Url::parse("http://localhost:4444").unwrap();

    // web server orchestrator
    async_spawn(run_orchestrator::<
        TestTypes,
        DANetwork,
        QuorumNetwork,
        ViewSyncNetwork,
        VIDNetwork,
        NodeImpl,
    >(OrchestratorArgs {
        url: orchestrator_url.clone(),
        config_file: args.config_file.clone(),
    }));

    // multi validator run
    let mut nodes = Vec::new();
    for _ in 0..(config.config.total_nodes.get()) {
        let orchestrator_url = orchestrator_url.clone();
        let node = async_spawn(async move {
            infra::main_entry_point::<
                TestTypes,
                DANetwork,
                QuorumNetwork,
                ViewSyncNetwork,
                VIDNetwork,
                NodeImpl,
                ThisRun,
            >(ValidatorArgs {
                url: orchestrator_url,
                public_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                network_config_file: None,
            })
            .await
        });
        nodes.push(node);
    }
    let _result = futures::future::join_all(nodes).await;
}
