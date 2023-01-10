pub mod config;

use async_lock::RwLock;
use hotshot_types::traits::election::ElectionConfig;
use hotshot_types::traits::signature_key::SignatureKey;
use std::default;
use std::io;
use tide_disco::Api;
use tide_disco::App;

use tide_disco::api::ApiError;
use tide_disco::error::ServerError;
use tide_disco::method::ReadState;
use tide_disco::method::WriteState;

use async_trait::async_trait;
use futures::FutureExt;

use crate::config::NetworkConfig;

type State<KEY, ELECTION> = RwLock<OrchestratorState<KEY, ELECTION>>;

// TODO Can probably get rid of the extra generic stuff ED
#[derive(Default, Clone)]
struct OrchestratorState<KEY, ELECTION> {
    latest_index: u16,
    config: NetworkConfig<KEY, ELECTION>,
    start: bool,
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
    fn post_getconfig(&mut self) -> Result<(NetworkConfig<KEY, ELECTION>), ServerError>;
    fn get_start(&self) -> Result<(bool), ServerError>;
    fn post_ready(&mut self) -> Result<(), ServerError>;
    fn post_run_results(&mut self) -> Result<(), ServerError>;
}

// You specify a default type when declaring a generic type with the <PlaceholderType=ConcreteType> syntax. Rust docs
// ED TODO Do we need the static here?
impl<KEY, ELECTION> OrchestratorApi<KEY, ELECTION> for OrchestratorState<KEY, ELECTION>
where
    KEY: serde::Serialize + Clone,
    ELECTION: serde::Serialize + Clone,
{
    fn post_getconfig(&mut self) -> Result<(NetworkConfig<KEY, ELECTION>), ServerError> {
        let mut config = self.config.clone();
        config.node_index = self.latest_index.into();

        self.latest_index += 1;
        Ok((config))
    }

    fn get_start(&self) -> Result<(bool), ServerError> {
        Ok((self.start))
    }

    fn post_ready(&mut self) -> Result<(), ServerError> {
        self.nodes_connected += 1; 
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
    // let mut api = match &options.api_path {
    //     Some(path) => Api::<State, ServerError>::from_file(path)?,
    //     None => {
    //         let toml = toml::from_str(include_str!("../api.toml")).map_err(|err| {
    //             ApiError::CannotReadToml {
    //                 reason: err.to_string(),
    //             }
    //         })?;
    //         Api::<State, ServerError>::new(toml)?
    //     }
    // };

    let mut api = Api::<State, ServerError>::from_file("orchestrator/api.toml").unwrap();
    api.post("post_getconfig", |req, state| {
        async move {
            state.post_getconfig()
        }
        .boxed()
    })?
    .post("postready", |req, state| {
        async move {
            state.post_ready()
        }
        .boxed()
    })?
    .get("getstart", |req, state| {
        async move {
            state.get_start()
        }
        .boxed()
    })?
    .post("results", |req, state| {
        async move {
            state.post_run_results()
        }
        .boxed()
    })?;
    Ok(api)
}

pub async fn run_orchestrator<KEY, ELECTION>(network_config: NetworkConfig<KEY, ELECTION>) -> io::Result<()>
where
    KEY: SignatureKey + 'static + serde::Serialize,
    ELECTION: ElectionConfig + 'static + serde::Serialize,
{
    let api = define_api().unwrap();

    let state: RwLock<OrchestratorState<KEY, ELECTION>> = RwLock::new(OrchestratorState::new(network_config));
    let mut app = App::<RwLock<OrchestratorState<KEY, ELECTION>>, ServerError>::with_state(state);
    app.register_module("api", api).unwrap();
    app.serve(format!("http://0.0.0.0:{}", 8080)).await
}
