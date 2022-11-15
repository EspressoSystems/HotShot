// use tide_disco::{http::StatusCode, Api, App, Error, RequestError};
// use std::io;

use std::collections::HashMap;

use clap::Args;
use futures::FutureExt;
use tide_disco::method::WriteState;
use std::path::PathBuf;
use tide_disco::api::ApiError;
use tide_disco::error::ServerError;
use tide_disco::method::ReadState;
use tide_disco::Api;
use tide_disco::App;
use async_lock::RwLock;

// type State = WebServerState;
type Error = ServerError;

#[derive(Clone, Default)]
struct WebServerState {
    proposals: HashMap<u64, Vec<u8>>,
    votes: HashMap<u64, Vec<Vec<u8>>>,
}

impl WebServerState {
    fn new() -> Result<Self, ServerError> {
        Ok(WebServerState::default())
    }
}

#[derive(Args, Default)]
pub struct Options {
    #[arg(long = "availability-api-path", env = "ESPRESSO_AVAILABILITY_API_PATH")]
    pub api_path: Option<PathBuf>,
}

fn define_api<State>(options: &Options) -> Result<Api<State, Error>, ApiError>
where
    State: 'static + Send + Sync + ReadState + WriteState,
    for<'a> &'a <State as ReadState>::State: Send + Sync,
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
        async move { Ok("Hello, world!") }.boxed()
    })?
    .post("postproposal",  |req, state| {
        async move { Ok(()) }.boxed()
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
    let mut api = define_api(&options).unwrap();
    let mut app = App::<RwLock<WebServerState>, Error>::with_state(RwLock::new(WebServerState::default()));
    // api.get("proposal", |req, state| {
    //     async move { Ok("Hello, world!") }.boxed()
    // })
    // .unwrap();
 

    app.register_module("api", api);
    app.serve("http://0.0.0.0:8080").await;

    ()
}
