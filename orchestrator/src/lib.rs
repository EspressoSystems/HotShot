pub mod config;

use async_lock::RwLock;
use hotshot_types::traits::election::ElectionConfig;
use hotshot_types::traits::signature_key::SignatureKey;
use std::io;
use std::io::ErrorKind;
use std::net::IpAddr;
use std::net::SocketAddr;
use tide_disco::Api;
use tide_disco::App;
use tracing::log::error;

use tide_disco::api::ApiError;
use tide_disco::error::ServerError;
use tide_disco::method::ReadState;
use tide_disco::method::WriteState;

use futures::FutureExt;

use crate::config::NetworkConfig;

#[derive(Default, Clone)]
struct OrchestratorState<KEY, ELECTION> {
    /// Tracks the latest node index we have generated a configuration for
    latest_index: u16,
    /// The network configuration
    config: NetworkConfig<KEY, ELECTION>,
    /// Whether nodes should start their HotShot instances
    /// Will be set to true once all nodes post they are ready to start
    start: bool,
    /// The total nodes that have posted they are ready to start
    pub nodes_connected: u64,
}

impl<KEY: SignatureKey + 'static, ELECTION: ElectionConfig + 'static>
    OrchestratorState<KEY, ELECTION>
{
    pub fn new(network_config: NetworkConfig<KEY, ELECTION>) -> Self {
        OrchestratorState {
            latest_index: 0,
            config: network_config,
            start: false,
            nodes_connected: 0,
        }
    }
}

pub trait OrchestratorApi<KEY, ELECTION> {
    fn post_identity(&mut self, identity: &str) -> Result<u16, ServerError>; 
    fn post_getconfig(&mut self, node_index: u16) -> Result<NetworkConfig<KEY, ELECTION>, ServerError>;
    fn get_start(&self) -> Result<bool, ServerError>;
    fn post_ready(&mut self) -> Result<(), ServerError>;
    fn post_run_results(&mut self) -> Result<(), ServerError>;
}

impl<KEY, ELECTION> OrchestratorApi<KEY, ELECTION> for OrchestratorState<KEY, ELECTION>
where
    KEY: serde::Serialize + Clone,
    ELECTION: serde::Serialize + Clone,
{
    fn post_identity(&mut self, identity: &str) -> Result<u16, ServerError> {

        // TODO ED Move this constant out of function / add it to the config file 
        let NUM_BOOTSTRAP_NODES = 7;
        let node_index = self.latest_index; 
        self.latest_index += 1;

        // TODO ED Store identity for bootstrap nodes if needed 
        if self.config.libp2p_config.clone().is_some() {
            if self.config.libp2p_config.as_mut().unwrap().bootstrap_nodes.len() < NUM_BOOTSTRAP_NODES {
                let addr = SocketAddr::new("0.0.0.0".parse().unwrap(), 4444);
            }
            // self.config.libp2p_config.
            // Vec<(SocketAddr, Vec<u8, Global>)
        }

        // TODO https://github.com/EspressoSystems/HotShot/issues/850
        if usize::from(node_index) >= self.config.config.total_nodes.get() {
            return Err(ServerError { status: tide_disco::StatusCode::BadRequest, message: "Network has reached capacity".to_string() })
        }
        Ok(node_index)
    }


    fn post_getconfig(&mut self, node_index: u16) -> Result<NetworkConfig<KEY, ELECTION>, ServerError> {
        let mut config = self.config.clone();
        if config.libp2p_config.is_some() {

        }
        // TODO ED Needs to take in their node index as an argument
        // config.node_index = self.latest_index.into();

        // self.latest_index += 1;
        // TODO ED
        Ok(config)
    }

    fn get_start(&self) -> Result<bool, ServerError> {
        Ok(self.start)
    }

    // Assumes nodes do not post 'ready' twice
    fn post_ready(&mut self) -> Result<(), ServerError> {
        self.nodes_connected += 1;
        error!("Nodes connected: {}", self.nodes_connected);
        if self.nodes_connected >= self.config.config.known_nodes.len().try_into().unwrap() {
            self.start = true;
        }
        Ok(())
    }

    fn post_run_results(&mut self) -> Result<(), ServerError> {
        Ok(())
    }
}

/// Sets up all API routes
fn define_api<KEY, ELECTION, State>() -> Result<Api<State, ServerError>, ApiError>
where
    State: 'static + Send + Sync + ReadState + WriteState,
    <State as ReadState>::State: Send + Sync + OrchestratorApi<KEY, ELECTION>,
    KEY: serde::Serialize,
    ELECTION: serde::Serialize,
{
    let mut api = Api::<State, ServerError>::from_file("orchestrator/api.toml")
        .expect("api.toml file is not found");
    api.post("postidentity", |req, state| {
        async move { 
            // println!("{:?}", req.);
            let identity = req.string_param("identity")?; 
            state.post_identity(identity) }.boxed()
    })?
    .post("post_getconfig", |req, state| {
        async move { 
            let node_index = req.integer_param("node_index")?; 

            state.post_getconfig(node_index) }.boxed()
    })?
    .post("postready", |_req, state| {
        async move { state.post_ready() }.boxed()
    })?
    .get("getstart", |_req, state| {
        async move { state.get_start() }.boxed()
    })?
    .post("postresults", |_req, state| {
        async move { state.post_run_results() }.boxed()
    })?;
    Ok(api)
}

/// Runs the orchestrator
pub async fn run_orchestrator<KEY, ELECTION>(
    network_config: NetworkConfig<KEY, ELECTION>,
    host: IpAddr,
    port: u16,
) -> io::Result<()>
where
    KEY: SignatureKey + 'static + serde::Serialize,
    ELECTION: ElectionConfig + 'static + serde::Serialize,
{
    let api = define_api().map_err(|_e| io::Error::new(ErrorKind::Other, "Failed to define api"));

    let state: RwLock<OrchestratorState<KEY, ELECTION>> =
        RwLock::new(OrchestratorState::new(network_config));
    let mut app = App::<RwLock<OrchestratorState<KEY, ELECTION>>, ServerError>::with_state(state);
    app.register_module("api", api.unwrap())
        .expect("Error registering api");
    app.serve(format!("http://{host}:{port}")).await
}

#[cfg(test)]
mod test {
    use super::*;
    use async_compatibility_layer::art::async_spawn;
    use portpicker::pick_unused_port;
    use surf_disco::error::ClientError;

    type State = RwLock<WebServerState>;
    type Error = ServerError;

    #[cfg_attr(
        feature = "tokio-executor",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(feature = "async-std-executor", async_std::test)]
    async fn test_identity_api() {

        // TODO ED

     }
}