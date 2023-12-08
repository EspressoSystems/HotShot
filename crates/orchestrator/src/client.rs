use std::{net::IpAddr, time::Duration};

use crate::config::NetworkConfig;
use async_compatibility_layer::art::async_sleep;
use clap::Parser;
use futures::{Future, FutureExt};

use hotshot_types::traits::{election::ElectionConfig, signature_key::SignatureKey};
use surf_disco::{error::ClientError, Client};
use tide_disco::Url;

/// Holds the client connection to the orchestrator
pub struct OrchestratorClient {
    client: surf_disco::Client<ClientError>,
    pub identity: String,
}

// VALIDATOR

#[derive(Parser, Debug, Clone)]
#[command(
    name = "Multi-machine consensus",
    about = "Simulates consensus among multiple machines"
)]
/// Arguments passed to the validator
pub struct ValidatorArgs {
    /// The address the orchestrator runs on
    pub url: Url,
    /// This node's public IP address, for libp2p
    /// If no IP address is passed in, it will default to 127.0.0.1
    pub public_ip: Option<IpAddr>,
    /// An optional network config file to save to/load from
    /// Allows for rejoining the network on a complete state loss
    #[arg(short, long)]
    pub network_config_file: Option<String>,
}

impl OrchestratorClient {
    /// Creates the client that will connect to the orchestrator
    pub async fn new(args: ValidatorArgs, identity: String) -> Self {
        let client = surf_disco::Client::<ClientError>::new(args.url);
        // TODO ED: Add healthcheck wait here
        OrchestratorClient { client, identity }
    }

    /// Sends an identify message to the orchestrator and attempts to get its config
    /// Returns both the node_index and the run configuration from the orchestrator
    /// Will block until both are returned
    #[allow(clippy::type_complexity)]
    pub async fn get_config<K: SignatureKey, E: ElectionConfig>(
        &self,
        identity: String,
    ) -> NetworkConfig<K, E> {
        // get the node index
        let identity = identity.as_str();
        let identity = |client: Client<ClientError>| {
            async move {
                let node_index: Result<u16, ClientError> = client
                    .post(&format!("api/identity/{identity}"))
                    .send()
                    .await;
                node_index
            }
            .boxed()
        };
        let node_index = self.wait_for_fn_from_orchestrator(identity).await;

        // get the corresponding config
        let f = |client: Client<ClientError>| {
            async move {
                let config: Result<NetworkConfig<K, E>, ClientError> = client
                    .post(&format!("api/config/{node_index}"))
                    .send()
                    .await;
                config
            }
            .boxed()
        };

        let mut config = self.wait_for_fn_from_orchestrator(f).await;

        config.node_index = node_index as u64;

        config
    }

    /// Tells the orchestrator this validator is ready to start
    /// Blocks until the orchestrator indicates all nodes are ready to start
    pub async fn wait_for_all_nodes_ready(&self, node_index: u64) -> bool {
        let send_ready_f = |client: Client<ClientError>| {
            async move {
                let result: Result<_, ClientError> = client
                    .post("api/ready")
                    .body_json(&node_index)
                    .unwrap()
                    .send()
                    .await;
                result
            }
            .boxed()
        };
        self.wait_for_fn_from_orchestrator::<_, _, ()>(send_ready_f)
            .await;

        let wait_for_all_nodes_ready_f = |client: Client<ClientError>| {
            async move { client.get("api/start").send().await }.boxed()
        };
        self.wait_for_fn_from_orchestrator(wait_for_all_nodes_ready_f)
            .await
    }

    /// Generic function that waits for the orchestrator to return a non-error
    /// Returns whatever type the given function returns
    async fn wait_for_fn_from_orchestrator<F, Fut, GEN>(&self, f: F) -> GEN
    where
        F: Fn(Client<ClientError>) -> Fut,
        Fut: Future<Output = Result<GEN, ClientError>>,
    {
        loop {
            let client = self.client.clone();
            let res = f(client).await;
            match res {
                Ok(x) => break x,
                Err(_x) => {
                    async_sleep(Duration::from_millis(250)).await;
                }
            }
        }
    }
}
