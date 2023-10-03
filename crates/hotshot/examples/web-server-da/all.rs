pub mod types;

use crate::infra::OrchestratorArgs;
use crate::types::{ThisMembership, ThisRun};
use crate::{
    infra_da::run_orchestrator_da,
    types::{DANetwork, NodeImpl, QuorumNetwork, ViewSyncNetwork},
};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

#[path = "../infra/mod.rs"]
pub mod infra;
#[path = "../infra/modDA.rs"]
pub mod infra_da;

use async_compatibility_layer::{art::async_spawn, channel::oneshot};
use hotshot::demos::sdemo::SDemoTypes;
use hotshot_orchestrator::client::ValidatorArgs;
use tracing::error;

#[cfg_attr(async_executor_impl = "tokio", tokio::main)]
#[cfg_attr(async_executor_impl = "async-std", async_std::main)]
async fn main() {
    // web servers
    let (server_shutdown_sender_cdn, server_shutdown_cdn) = oneshot();
    let (server_shutdown_sender_da, server_shutdown_da) = oneshot();
    let (server_shutdown_sender_view_sync, server_shutdown_view_sync) = oneshot();
    let _sender = Arc::new(server_shutdown_sender_cdn);
    let _sender = Arc::new(server_shutdown_sender_da);
    let _sender = Arc::new(server_shutdown_sender_view_sync);

    async_spawn(async move {
        if let Err(e) = hotshot_web_server::run_web_server::<
            <SDemoTypes as hotshot_types::traits::node_implementation::NodeType>::SignatureKey,
        >(Some(server_shutdown_cdn), 9000)
        .await
        {
            error!("Problem starting cdn web server: {:?}", e);
        }
        error!("cdn");
    });
    async_spawn(async move {
        if let Err(e) = hotshot_web_server::run_web_server::<
            <SDemoTypes as hotshot_types::traits::node_implementation::NodeType>::SignatureKey,
        >(Some(server_shutdown_da), 9001)
        .await
        {
            error!("Problem starting da web server: {:?}", e);
        }
        error!("da");
    });
    async_spawn(async move {
        if let Err(e) = hotshot_web_server::run_web_server::<
            <SDemoTypes as hotshot_types::traits::node_implementation::NodeType>::SignatureKey,
        >(Some(server_shutdown_view_sync), 9002)
        .await
        {
            error!("Problem starting view sync web server: {:?}", e);
        }
        error!("vs");
    });

    // web server orchestrator
    async_spawn(run_orchestrator_da::<
        SDemoTypes,
        ThisMembership,
        DANetwork,
        QuorumNetwork,
        ViewSyncNetwork,
        NodeImpl,
    >(OrchestratorArgs {
        host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        port: 4444,
        config_file: "./crates/orchestrator/default-web-server-run-config.toml".to_string(),
    }));

    // multi validator run
    let mut nodes = Vec::new();
    for _ in 0..10 {
        let node = async_spawn(async move {
            infra_da::main_entry_point::<
                SDemoTypes,
                ThisMembership,
                DANetwork,
                QuorumNetwork,
                ViewSyncNetwork,
                NodeImpl,
                ThisRun,
            >(ValidatorArgs {
                host: "127.0.0.1".to_string(),
                port: 4444,
                public_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            })
            .await
        });
        nodes.push(node);
    }
    let _result = futures::future::join_all(nodes).await;
}
