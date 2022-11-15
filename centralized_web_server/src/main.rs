// use tide_disco::{http::StatusCode, Api, App, Error, RequestError};
// use std::io;

use std::collections::HashMap;

use async_std::sync::RwLock;
use clap::Args;
use futures::FutureExt;
use std::path::PathBuf;
use tide_disco::api::ApiError;
use tide_disco::error::ServerError;
use tide_disco::method::ReadState;
use tide_disco::method::WriteState;
use tide_disco::Api;
use tide_disco::App;
use tide_disco::StatusCode;

type State = RwLock<WebServerState>;
type Error = ServerError;

#[derive(Clone, Default)]
struct WebServerState {
    proposals: HashMap<u128, Vec<u8>>,
    votes: HashMap<u128, Vec<Vec<u8>>>,
}

impl WebServerState {
    fn new() -> Self {
        WebServerState::default()
    }
}

pub trait WebServerDataSource {
    fn get_proposal(&self, view_number: u128) -> Result<Vec<u8>, Error>;
    fn put_vote(&mut self, view_number: u128, vote: Vec<u8>) -> Result<(), Error>;
    fn put_proposal(&mut self, view_number: u128, proposal: Vec<u8>) -> Result<(), Error>;
}

impl WebServerDataSource for WebServerState {
    fn get_proposal(&self, view_number: u128) -> Result<Vec<u8>, Error> {
        match self.proposals.get(&view_number) {
            Some(proposal) => Ok(proposal.clone()),
            None => Err(ServerError {
                status: StatusCode::BadRequest,
                message: format!("No proposal for view {}", view_number),
            }),
        }
    }
    fn put_vote(&mut self, view_number: u128, vote: Vec<u8>) -> Result<(), Error> {
        // TODO KALEY: security check for votes needed?
        self.votes
            .entry(view_number)
            .and_modify(|current_votes| current_votes.push(vote.clone()))
            .or_insert(vec![vote]);
        Ok(())
    }

    fn put_proposal(&mut self, view_number: u128, proposal: Vec<u8>) -> Result<(), Error> {
        //TODO KALEY: security check for valid proposal from correct node?
        self.proposals.insert(view_number, proposal);
        Ok(())
    }
}

#[derive(Args, Default)]
pub struct Options {
    #[arg(
        long = "centralized-web-server-api-path",
        env = "CENTRALIZED_WEB_SERVER_API_PATH"
    )]
    pub api_path: Option<PathBuf>,
}

fn define_api<State>(options: &Options) -> Result<Api<State, Error>, ApiError>
where
    State: 'static + Send + Sync + ReadState + WriteState,
    <State as ReadState>::State: Send + Sync + WebServerDataSource,
{
    let mut api = match &options.api_path {
        Some(path) => Api::<State, Error>::from_file(path)?,
        None => {
            let toml = toml::from_str(include_str!("../api.toml")).map_err(|err| {
                ApiError::CannotReadToml {
                    reason: err.to_string(),
                }
            })?;
            Api::<State, Error>::new(toml)?
        }
    };
    api.get("getproposal", |req, state| {
        async move {
            let view_number: u128 = req.integer_param("view_number").unwrap();
            state.get_proposal(view_number)
        }
        .boxed()
    })?
    .post("vote", |req, state| {
        async move {
            let view_number: u128 = req.integer_param("view_number").unwrap();
            // using body_bytes because we don't want to deserialize.  body_auto or body_json deserailizes
            let vote = req.body_bytes();
            // Add vote to state
            state.put_vote(view_number, vote.clone())
        }
        .boxed()
    })?
    .post("postproposal", |req, state| {
        async move {
            let view_number: u128 = req.integer_param("view_number").unwrap();
            // using body_bytes because we don't want to deserialize.  body_auto or body_json deserailizes
            let proposal = req.body_bytes();
            // Add proposal to state
            state.put_proposal(view_number, proposal.clone())
        }
        .boxed()
    })?;
    Ok(api)
}

#[async_std::main]
async fn main() -> () {
    // let mut app = tide_disco::new();
    // app.at("/orders/shoes").post(order_shoes);
    // app.listen("127.0.0.1:8080").await?;
    // let spec =
    //     toml::from_slice(&std::fs::read("./centralized_web_server/api.toml").unwrap()).unwrap();
    let options = Options::default();
    let api = define_api(&options).unwrap();
    let mut app = App::<State, Error>::with_state(State::default());

    app.register_module("api", api).unwrap();
    app.serve("http://0.0.0.0:8080").await;
    ()
}
