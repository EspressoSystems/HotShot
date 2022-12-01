use async_std::sync::RwLock;
use clap::Args;
use futures::FutureExt;
use std::collections::HashMap;
use std::io;
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
/// State that tracks proposals and votes the server receives
/// Data is stored as a `Vec<u8>` to not incur overhead from deserializing
struct WebServerState {
    proposals: HashMap<u128, Vec<Vec<u8>>>,
    votes: HashMap<u128, Vec<Vec<u8>>>,
    transactions: HashMap<u128, Vec<u8>>,
    num_txn: u128,
}

impl WebServerState {
    fn new() -> Self {
        Self {
            num_txn: 0,
            ..Default::default()
        }
    }
}

/// Trait defining methods needed for the `WebServerState`
pub trait WebServerDataSource {
    fn get_proposals(&self, view_number: u128) -> Result<Vec<Vec<u8>>, Error>;
    fn get_votes(&self, view_number: u128) -> Result<Vec<Vec<u8>>, Error>;
    fn get_transactions(&self, index: u128) -> Result<Vec<Vec<u8>>, Error>;
    fn post_vote(&mut self, view_number: u128, vote: Vec<u8>) -> Result<(), Error>;
    fn post_proposal(&mut self, view_number: u128, proposal: Vec<u8>) -> Result<(), Error>;
    fn post_transaction(&mut self, txn: Vec<u8>) -> Result<(), Error>;
}

impl WebServerDataSource for WebServerState {
    /// Return all proposals the server has received for a particular view
    fn get_proposals(&self, view_number: u128) -> Result<Vec<Vec<u8>>, Error> {
        match self.proposals.get(&view_number) {
            Some(proposals) => Ok(proposals.clone()),
            None => Err(ServerError {
                status: StatusCode::NotFound,
                message: format!("No proposals for view {}", view_number),
            }),
        }
    }

    /// Return all votes the server has received for a particular view
    fn get_votes(&self, view_number: u128) -> Result<Vec<Vec<u8>>, Error> {
        match self.votes.get(&view_number) {
            Some(votes) => Ok(votes.clone()),
            None => Err(ServerError {
                status: StatusCode::NotFound,
                message: format!("No votes for view {}", view_number),
            }),
        }
    }

    /// Return all transactions from provided index to most recent
    fn get_transactions(&self, index: u128) -> Result<Vec<Vec<u8>>, Error> {
        let mut txns = vec![];
        for i in index..self.num_txn {
            println!("getting txn {:?}", i);
            if let Some(txn) = self.transactions.get(&i) {
                txns.push(txn.clone())
            }
        }
        Ok(txns)
    }

    /// Stores a received vote in the `WebServerState`
    fn post_vote(&mut self, view_number: u128, vote: Vec<u8>) -> Result<(), Error> {
        self.votes
            .entry(view_number)
            .and_modify(|current_votes| current_votes.push(vote.clone()))
            .or_insert_with(|| vec![vote]);
        Ok(())
    }
    /// Stores a received proposal in the `WebServerState`
    fn post_proposal(&mut self, view_number: u128, proposal: Vec<u8>) -> Result<(), Error> {
        self.proposals
            .entry(view_number)
            .and_modify(|current_proposals| current_proposals.push(proposal.clone()))
            .or_insert_with(|| vec![proposal]);
        Ok(())
    }
    /// Stores a received group of transactions in the `WebServerState`
    fn post_transaction(&mut self, txn: Vec<u8>) -> Result<(), Error> {
        println!("posting txn {:?}", self.num_txn);
        self.transactions.insert(self.num_txn, txn);
        self.num_txn += 1;
        //TODO: @kaley: we will want to have batch transaction posting (vs one at a time), a la:
        // let starting_index = self.num_txn + 1;
        // self.num_txn += txns.len() as u128;
        // self.transactions.extend((starting_index..self.num_txn).zip(txns));
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

/// Sets up all API routes
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
            let view_number: u128 = req.integer_param("view_number")?;
            state.get_proposals(view_number)
        }
        .boxed()
    })?
    .get("getvotes", |req, state| {
        async move {
            let view_number: u128 = req.integer_param("view_number")?;
            state.get_votes(view_number)
        }
        .boxed()
    })?
    .get("gettransactions", |req, state| {
        async move {
            let index: u128 = req.integer_param("index")?;
            state.get_transactions(index)
        }
        .boxed()
    })?
    .post("postvote", |req, state| {
        async move {
            let view_number: u128 = req.integer_param("view_number")?;
            // Using body_bytes because we don't want to deserialize; body_auto or body_json deserializes
            let vote = req.body_bytes();
            // Add vote to state
            state.post_vote(view_number, vote)
        }
        .boxed()
    })?
    .post("postproposal", |req, state| {
        async move {
            let view_number: u128 = req.integer_param("view_number")?;
            // Using body_bytes because we don't want to deserialize; body_auto or body_json deserializes
            let proposal = req.body_bytes();
            // Add proposal to state
            state.post_proposal(view_number, proposal)
        }
        .boxed()
    })?
    .post("posttransaction", |req, state| {
        async move {
            // Using body_bytes because we don't want to deserialize; body_auto or body_json deserializes
            let txns = req.body_bytes();
            // Add proposal to state
            state.post_transaction(txns)
        }
        .boxed()
    })?;
    Ok(api)
}

// #[async_std::main]
// TODO ED: update this function to be more library-like
// (we need it to be a library to import into the node client)
// TODO ED: make a bin file that runs the web server similarly
// to the other centralized server 
pub async fn main() -> io::Result<()> {
    let options = Options::default();
    let api = define_api(&options).unwrap();
    let mut app = App::<State, Error>::with_state(State::default());

    app.register_module("api", api).unwrap();
    app.serve("http://0.0.0.0:8080").await
}

#[cfg(test)]
mod test {
    use super::*;
    use async_std::task::spawn;
    use portpicker::pick_unused_port;
    use tide_disco::StatusCode;

    type State = RwLock<WebServerState>;
    type Error = ServerError;

    #[async_std::test]
    async fn test_web_server() {
        let port = pick_unused_port().unwrap();
        let base_url = format!("0.0.0.0:{port}");
        let options = Options::default();
        let api = define_api(&options).unwrap();
        let mut app = App::<State, Error>::with_state(State::new(WebServerState::new()));

        app.register_module("api", api).unwrap();
        let _handle = spawn(app.serve(base_url.clone()));

        let base_url = format!("http://{base_url}").parse().unwrap();
        let client = surf_disco::Client::<ServerError>::new(base_url);
        assert!(client.connect(None).await);

        // Test posting and getting proposals
        let prop1 = "prop1";
        client
            .post::<()>("api/proposal/1")
            .body_binary(&prop1)
            .unwrap()
            .send()
            .await
            .unwrap();
        let resp = client
            .get::<Vec<Vec<u8>>>("api/proposal/1")
            .send()
            .await
            .unwrap();
        let res1: &str = bincode::deserialize(&resp[0]).unwrap();
        assert_eq!(res1, prop1);

        let prop2 = "prop2";
        client
            .post::<()>("api/proposal/2")
            .body_binary(&prop2)
            .unwrap()
            .send()
            .await
            .unwrap();
        let resp = client
            .get::<Vec<Vec<u8>>>("api/proposal/2")
            .send()
            .await
            .unwrap();

        let res2: &str = bincode::deserialize(&resp[0]).unwrap();
        assert_eq!(res2, prop2);
        assert_ne!(res1, res2);

        assert_eq!(
            client.get::<Vec<u8>>("api/proposal/3").send().await,
            Err(ServerError {
                status: StatusCode::NotFound,
                message: String::from("No proposals for view 3")
            })
        );

        // Test posting and getting votes
        let vote1 = "vote1";
        client
            .post::<()>("api/votes/1")
            .body_binary(&vote1)
            .unwrap()
            .send()
            .await
            .unwrap();

        let vote2 = "vote2";
        client
            .post::<()>("api/votes/1")
            .body_binary(&vote2)
            .unwrap()
            .send()
            .await
            .unwrap();
        let resp = client
            .get::<Vec<Vec<u8>>>("api/votes/1")
            .send()
            .await
            .unwrap();
        let res1: &str = bincode::deserialize(&resp[0]).unwrap();
        let res2: &str = bincode::deserialize(&resp[1]).unwrap();
        assert_eq!(vote1, res1);
        assert_eq!(vote2, res2);

        //test posting/getting transactions
        let txns1 = "abc";
        let txns2 = "def";
        client
            .post::<()>("api/transactions")
            .body_binary(&txns1)
            .unwrap()
            .send()
            .await
            .unwrap();
        client
            .post::<()>("api/transactions")
            .body_binary(&txns2)
            .unwrap()
            .send()
            .await
            .unwrap();
        let resp = client
            .get::<Vec<Vec<u8>>>("api/transactions/0")
            .send()
            .await
            .unwrap();

        let txn_resp1: &str = bincode::deserialize(&resp[0]).unwrap();
        let txn_resp2: &str = bincode::deserialize(&resp[1]).unwrap();
        assert_eq!(txns1, txn_resp1);
        assert_eq!(txns2, txn_resp2)
    }
}
