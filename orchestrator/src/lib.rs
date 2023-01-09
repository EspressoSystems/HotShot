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


#[derive(Default)]
struct OrchestratorState<KEY: SignatureKey + 'static, ELECTION: ElectionConfig + 'static> {
    latest_index: u16,
    config: NetworkConfig<KEY, ELECTION>,
    start: bool, 
    pub nodes_connected: u64
}

impl<KEY: SignatureKey + 'static, ELECTION: ElectionConfig + 'static>
    OrchestratorState<KEY, ELECTION>
{
    pub fn new() -> Self {
        OrchestratorState {
            latest_index: 0,
            config: NetworkConfig::default(),
            start: false, 
            nodes_connected: 0
        }
    }

    pub fn update_connections(&mut self) {
        self.nodes_connected += 1; 
    }
}

// /// Trait defining methods needed for the `WebServerState`
// pub trait WebServerDataSource {
//     fn get_proposals(&self, view_number: u128) -> Result<Option<Vec<Vec<u8>>>, Error>;
//     fn get_votes(&self, view_number: u128, index: u128) -> Result<Option<Vec<Vec<u8>>>, Error>;
//     fn get_transactions(&self, index: u128) -> Result<Option<Vec<Vec<u8>>>, Error>;
//     fn post_vote(&mut self, view_number: u128, vote: Vec<u8>) -> Result<(), Error>;
//     fn post_proposal(&mut self, view_number: u128, proposal: Vec<u8>) -> Result<(), Error>;
//     fn post_transaction(&mut self, txn: Vec<u8>) -> Result<(), Error>;
// }

// impl WebServerDataSource for WebServerState {
//     /// Return all proposals the server has received for a particular view
//     fn get_proposals(&self, view_number: u128) -> Result<Option<Vec<Vec<u8>>>, Error> {
//         match self.proposals.get(&view_number) {
//             Some(proposals) => Ok(Some(proposals.clone())),
//             None => Ok(None),
//         }
//     }

/// Sets up all API routes
// ED TODO update
fn define_api<State>() -> Result<Api<State, ServerError>, ApiError>
where
    State: 'static + Send + Sync + ReadState + WriteState,
    <State as ReadState>::State: Send + Sync, //+ WebServerDataSource,
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

    let mut api = Api::<State, ServerError>::from_file(
        "orchestrator/api.toml",
    )
    .unwrap();
    api.get("getconfig", |req, state| {
        async move {
            println!("THIS IS A CONFIG!");
            Ok(())
        }
        .boxed()
    })?;
    Ok(api)
}

pub async fn run_orchestrator<KEY, ELECTION>() -> io::Result<()>
where
    KEY: SignatureKey + 'static + serde::Serialize,
    ELECTION: ElectionConfig + 'static + serde::Serialize,
{

    let api = define_api().unwrap(); 


    // let mut api = Api::<RwLock<OrchestratorState<KEY, ELECTION>>, ServerError>::from_file(
    //     "orchestrator/api.toml",
    // )
    // .unwrap();

    // TODO ED separate out into get config and node_is_ready put command
    // api.post("getconfig", |req, state| {
    //     async move {
    //         println!("Here is a config file! ");
    //         state.update_connections();  
    //         println!("There are this many nodes connected: {}", state.nodes_connected);
    //         // TODO ED actually send the correctly generated config file with the right key index
    //         let config: NetworkConfig<KEY, ELECTION> = NetworkConfig::default();
    //         Ok(config)
    //     }
    //     .boxed()
    // });

    // api.get("start", |req, state| {
    //     async move {
    //         println!("Sending start command! ");
    //         Ok(state.start)
    //     }
    //     .boxed()
    // });

    let state: RwLock<OrchestratorState<KEY, ELECTION>> = RwLock::new(OrchestratorState::new());
    let mut app = App::<RwLock<OrchestratorState<KEY, ELECTION>>, ServerError>::with_state(state);
    app.register_module("api", api).unwrap();
    app.serve(format!("http://0.0.0.0:{}", 8080)).await
}
