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
    fn get_proposals(&self, view_number: u128) -> Result<Vec<u8>, Error>;
    fn get_votes(&self, view_number: u128) -> Result<Vec<Vec<u8>>, Error>;
    fn post_vote(&mut self, view_number: u128, vote: Vec<u8>) -> Result<(), Error>;
    fn post_proposal(&mut self, view_number: u128, proposal: Vec<u8>) -> Result<(), Error>;
}

impl WebServerDataSource for WebServerState {
    fn get_proposals(&self, view_number: u128) -> Result<Vec<u8>, Error> {
        match self.proposals.get(&view_number) {
            Some(proposals) => Ok(proposals.clone()),
            None => Err(ServerError {
                status: StatusCode::BadRequest,
                message: format!("No proposal for view {}", view_number),
            }),
        }
    }

    fn get_votes(&self, view_number: u128) -> Result<Vec<Vec<u8>>, Error> {
        match self.votes.get(&view_number) {
            Some(votes) => Ok(votes.clone()),
            None => Err(ServerError {
                status: StatusCode::BadRequest,
                message: format!("No votes for view {}", view_number),
            }),
        }
    }

    fn post_vote(&mut self, view_number: u128, vote: Vec<u8>) -> Result<(), Error> {
        // TODO KALEY: security check for votes needed?
        self.votes
            .entry(view_number)
            .and_modify(|current_votes| current_votes.push(vote.clone()))
            .or_insert(vec![vote]);
        Ok(())
    }

    fn post_proposal(&mut self, view_number: u128, proposal: Vec<u8>) -> Result<(), Error> {
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
            state.get_proposals(view_number)
        }
        .boxed()
    })?
    .get("getvotes", |req, state| {
        async move {
            let view_number: u128 = req.integer_param("view_number").unwrap();
            state.get_votes(view_number)
        }
        .boxed()
    })?
    .post("postvote", |req, state| {
        async move {
            let view_number: u128 = req.integer_param("view_number").unwrap();
            // using body_bytes because we don't want to deserialize.  body_auto or body_json deserailizes
            let vote = req.body_bytes();
            // Add vote to state
            state.post_vote(view_number, vote.clone())
        }
        .boxed()
    })?
    .post("postproposal", |req, state| {
        async move {
            let view_number: u128 = req.integer_param("view_number").unwrap();
            // using body_bytes because we don't want to deserialize.  body_auto or body_json deserailizes
            let proposal = req.body_bytes();
            // Add proposal to state
            state.post_proposal(view_number, proposal.clone())
        }
        .boxed()
    })?;
    Ok(api)
}

#[async_std::main]
async fn main() -> () {
    let options = Options::default();
    let api = define_api(&options).unwrap();
    let mut app = App::<State, Error>::with_state(State::default());

    app.register_module("api", api).unwrap();
    app.serve("http://0.0.0.0:8080").await;
    ()
}

#[cfg(test)]
mod test {
    use super::*;
    use async_std::task::spawn;
    use portpicker::pick_unused_port;
    use tide_disco::StatusCode::BadRequest;

    type State = RwLock<WebServerState>;
    type Error = ServerError;

    #[async_std::test]
    async fn test_web_server() {
        let port = pick_unused_port().unwrap();
        let base_url = format!("0.0.0.0:{port}");
        let options = Options::default();
        let api = define_api(&options).unwrap();
        let mut app = App::<State, Error>::with_state(State::default());

        app.register_module("api", api).unwrap();
        let handle = spawn(app.serve(base_url.clone()));

        let base_url = format!("http://{base_url}").parse().unwrap();
        let client = surf_disco::Client::<ServerError>::new(base_url);
        assert!(client.connect(None).await);

        let prop1 = "test";
        client
            .post::<()>("api/postproposal/1")
            .body_binary(&prop1)
            .unwrap()
            .send()
            .await
            .unwrap();
        let resp = client
            .get::<Vec<u8>>("api/getproposal/1")
            .send()
            .await
            .unwrap();
        let res1: &str = bincode::deserialize(&resp).unwrap();
        assert_eq!(res1, prop1);

        let prop2 = "test2";
        client
            .post::<()>("api/postproposal/2")
            .body_binary(&prop2)
            .unwrap()
            .send()
            .await
            .unwrap();
        let resp = client
            .get::<Vec<u8>>("api/getproposal/2")
            .send()
            .await
            .unwrap();
        let res2: &str = bincode::deserialize(&resp).unwrap();
        assert_eq!(res2, prop2);
        assert_ne!(res1, res2);

        assert_eq!(
            client.get::<Vec<u8>>("api/getproposal/3").send().await,
            Err(ServerError {
                status: BadRequest,
                message: String::from("No proposal for view 3")
            })
        );

        // Test votes
        let vote1 = "vote1";
        client
            .post::<()>("api/postvote/1")
            .body_binary(&vote1)
            .unwrap()
            .send()
            .await
            .unwrap();
        let resp = client
            .get::<Vec<u8>>("api/getvotes/1")
            .send()
            .await
            .unwrap();
        let res1: Vec<Vec<u8>> = bincode::deserialize(&resp).unwrap();

    }
}
