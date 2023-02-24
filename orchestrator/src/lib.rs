pub mod config;

use async_lock::RwLock;
use hotshot_types::traits::election::ElectionConfig;
use hotshot_types::traits::signature_key::SignatureKey;
use std::io;
use std::net::IpAddr;
use tide_disco::Api;
use tide_disco::App;

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
    fn post_getconfig(&mut self) -> Result<NetworkConfig<KEY, ELECTION>, ServerError>;
    fn get_start(&self) -> Result<bool, ServerError>;
    fn post_ready(&mut self) -> Result<(), ServerError>;
    fn post_run_results(&mut self) -> Result<(), ServerError>;
}

impl<KEY, ELECTION> OrchestratorApi<KEY, ELECTION> for OrchestratorState<KEY, ELECTION>
where
    KEY: serde::Serialize + Clone,
    ELECTION: serde::Serialize + Clone,
{
    fn post_getconfig(&mut self) -> Result<NetworkConfig<KEY, ELECTION>, ServerError> {
        let mut config = self.config.clone();
        config.node_index = self.latest_index.into();

        self.latest_index += 1;
        Ok(config)
    }

    fn get_start(&self) -> Result<bool, ServerError> {
        Ok(self.start)
    }

    // Assumes nodes do not post 'ready' twice
    fn post_ready(&mut self) -> Result<(), ServerError> {
        self.nodes_connected += 1;
        println!("Nodes connected: {}", self.nodes_connected);
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
    let mut api = Api::<State, ServerError>::from_file("orchestrator/api.toml").unwrap();
    api.post("post_getconfig", |_req, state| {
        async move { state.post_getconfig() }.boxed()
    })?
    .post("postready", |_req, state| {
        async move { state.post_ready() }.boxed()
    })?
    .get("getstart", |_req, state| {
        async move { state.get_start() }.boxed()
    })?
    .post("results", |_req, state| {
        async move { state.post_run_results() }.boxed()
    })?;
    Ok(api)
}

pub async fn run_orchestrator<KEY, ELECTION>(
    network_config: NetworkConfig<KEY, ELECTION>,
    host: IpAddr,
    port: u16,
) -> io::Result<()>
where
    KEY: SignatureKey + 'static + serde::Serialize,
    ELECTION: ElectionConfig + 'static + serde::Serialize,
{
    let api = define_api().unwrap();

    let state: RwLock<OrchestratorState<KEY, ELECTION>> =
        RwLock::new(OrchestratorState::new(network_config));
    let mut app = App::<RwLock<OrchestratorState<KEY, ELECTION>>, ServerError>::with_state(state);
    app.register_module("api", api).unwrap();
    app.serve(format!("http://{}:{}", host, port)).await
}
