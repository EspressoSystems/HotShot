// use tide_disco::{http::StatusCode, Api, App, Error, RequestError};
// use std::io;

use std::collections::HashMap;

use async_lock::RwLock;
use clap::Args;
use futures::FutureExt;
use std::path::PathBuf;
use tide_disco::api::ApiError;
use tide_disco::error::ServerError;
use tide_disco::method::ReadState;
use tide_disco::method::WriteState;
use tide_disco::Api;
use tide_disco::App;

type State = RwLock<WebServerState>;
type Error = ServerError;

#[derive(Clone, Default)]
struct WebServerState {
    proposals: HashMap<u128, Vec<u8>>,
    votes: HashMap<u128, Vec<Vec<u8>>>,
}

impl WebServerState {
    fn new() -> Result<Self, ServerError> {
        Ok(WebServerState::default())
    }
}

pub trait WebServerDataSource {
    fn put_vote(&mut self, view_number: u128, vote: Vec<u8>) -> Result<(), ServerError>;
    fn put_proposal(&mut self, view_number: u128, proposal: Vec<u8>) -> Result<(), ServerError>;
}

impl WebServerDataSource for WebServerState {
    fn put_vote(&mut self, view_number: u128, vote: Vec<u8>) -> Result<(), ServerError> {
        // TODO use entry here so we can modify votes vector in place
        // KALEY: should we be adding to votes per view, i.e. 
        self.votes.entry(view_number).and_modify(|current_votes| current_votes.push(vote.clone())).or_insert(vec![vote]);
        //self.votes.insert(view_number, vote);
        Ok(())
    }

    fn put_proposal(&mut self, view_number: u128, proposal: Vec<u8>) -> Result<(), ServerError> {
        //TODO KALEY: security check for valid proposal from correct node?
        self.proposals.insert(view_number, proposal);
        Ok(())
    }
}

#[derive(Args, Default)]
pub struct Options {
    // TODO update below to be relevant to the centralized server
    #[arg(long = "availability-api-path", env = "ESPRESSO_AVAILABILITY_API_PATH")]
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
            Ok(format!(
                "You requested the proposal for view {:?}",
                view_number
            ))
        }
        .boxed()
    })?
    .post("vote", |req, state| {
        async move {
            let view_number: u128 = req.integer_param("view_number").unwrap();
            // using body_bytes because we don't want to deserialize.  body_auto or body_json deserailizes
            let vote = req.body_bytes();
            // Add vote to state
            // *state.votes.get(view_number);
            state.put_vote(view_number, vote.clone());
            //(*state).put_vote(view_number, vote);

            Ok(format!(
                "You voted for view {:?}.  \n This is the vote you sent: {:?}",
                view_number, vote
            ))
        }
        .boxed()
    })?
    .post("postproposal", |req, state| async move {
        let view_number: u128 = req.integer_param("view_number").unwrap();
        // using body_bytes because we don't want to deserialize.  body_auto or body_json deserailizes
        let proposal = req.body_bytes();
        // Add proposal to state
        state.put_proposal(view_number, proposal.clone());

        Ok(format!(
            "You sent a proposal for view {:?}.  \n This is the proposal you sent: {:?}",
            view_number, proposal
    )) }.boxed())?;
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
    let mut api = define_api(&options).unwrap();
    let mut app = App::<RwLock<WebServerState>, Error>::with_state(State::default());
    // api.get("proposal", |req, state| {
    //     async move { Ok("Hello, world!") }.boxed()
    // })
    // .unwrap();

    app.register_module("api", api);
    app.serve("http://0.0.0.0:8080").await;

    ()
}
