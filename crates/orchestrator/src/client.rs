use std::{fs, net::IpAddr, time::Duration};

use crate::config::NetworkConfig;
use async_compatibility_layer::art::async_sleep;
use clap::Parser;
use futures::{Future, FutureExt};

use hotshot_types::traits::node_implementation::NodeType;
use surf_disco::{error::ClientError, Client};
use tracing::error;

/// Loads the node's index and config from a file
#[allow(clippy::type_complexity)]
fn load_index_and_config_from_file<TYPES>(
    file: String,
) -> Option<(
    u16,
    NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
)>
where
    TYPES: NodeType,
{
    let data = match fs::read(file) {
        Ok(data) => data,
        Err(e) => {
            error!("Failed to load index and config from file: {}", e);
            None?
        }
    };

    match bincode::deserialize(&data) {
        Ok(data) => Some(data),
        Err(e) => {
            error!("Failed to deserialize index and config from file: {}", e);
            None
        }
    }
}

/// Saves the node's index and config to a file
fn save_index_and_config_to_file<TYPES>(
    node_index: u16,
    config: NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    file: String,
) where
    TYPES: NodeType,
{
    // serialize
    let serialized = match bincode::serialize(&(node_index, config)) {
        Ok(data) => data,
        Err(e) => {
            error!("Failed to serialize index and config to file: {}", e);
            return;
        }
    };

    // write
    if let Err(e) = fs::write(file, serialized) {
        error!("Failed to write index and config to file: {}", e);
    }
}

/// Holds the client connection to the orchestrator
pub struct OrchestratorClient {
    client: surf_disco::Client<ClientError>,
    file: Option<String>,
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
    pub url: String,
    /// The port the orchestrator runs on
    pub port: u16,
    /// This node's public IP address, for libp2p
    /// If no IP address is passed in, it will default to 127.0.0.1
    pub public_ip: Option<IpAddr>,
    /// An optional network config file to save to/load from
    /// Allows for rejoining the network on a complete state loss
    #[arg(short, long)]
    pub config_file: Option<String>,
}

impl OrchestratorClient {
    /// Creates the client that connects to the orchestrator
    pub async fn new(args: ValidatorArgs) -> Self {
        let base_url = format!("{0}:{1}", args.url, args.port).parse().unwrap();
        let client = surf_disco::Client::<ClientError>::new(base_url);
        // TODO ED: Add healthcheck wait here
        OrchestratorClient {
            client,
            file: args.config_file,
        }
    }

    /// Gets the node config and index first from a file, or the orchestrator if
    /// 1. no file is specified
    /// 2. there is an issue with the file
    /// Saves file to disk if successfully retrieved from the orchestrator
    pub async fn config_from_file_or_orchestrator<TYPES: NodeType>(
        &self,
        identity: String,
    ) -> (
        u16,
        NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    ) {
        if let Some(file) = &self.file {
            error!("Attempting to retrieve node configuration from file");
            // load from file, if fail load from orchestrator and save
            match load_index_and_config_from_file::<TYPES>(file.clone()) {
                Some(res) => res,
                None => {
                    error!(
                        "Failed to load node configuration from file; trying to load from orchestrator"
                    );
                    // load from orchestrator
                    let (node_index, run_config) = self
                        .config_from_orchestrator::<TYPES>(identity.to_string())
                        .await;

                    // save to file
                    save_index_and_config_to_file::<TYPES>(
                        node_index,
                        run_config.clone(),
                        file.to_string(),
                    );

                    (node_index, run_config)
                }
            }
        } else {
            // load from orchestrator
            error!("Attempting to load node configuration from orchestrator");
            self.config_from_orchestrator::<TYPES>(identity.to_string())
                .await
        }
    }

    /// Sends an identify message to the orchestrator and attempts to get its config
    /// Returns both the node_index and the run configuration from the orchestrator
    /// Will block until both are returned
    #[allow(clippy::type_complexity)]
    pub async fn config_from_orchestrator<TYPES: NodeType>(
        &self,
        identity: String,
    ) -> (
        u16,
        NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    ) {
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
                let config: Result<
                    NetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
                    ClientError,
                > = client
                    .post(&format!("api/config/{node_index}"))
                    .send()
                    .await;
                config
            }
            .boxed()
        };

        (node_index, self.wait_for_fn_from_orchestrator(f).await)
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
