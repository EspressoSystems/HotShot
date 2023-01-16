pub mod config;

use async_compatibility_layer::channel::OneShotReceiver;
use async_lock::RwLock;
use clap::Args;
use config::WEB_SERVER_PORT;
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

type State = RwLock<WebServerState>;
type Error = ServerError;

//how many views to keep in memory
const MAX_VIEWS: usize = 10;
//how many transactions to keep in memory
const MAX_TXNS: usize = 10000;

#[derive(Default)]
/// State that tracks proposals and votes the server receives
/// Data is stored as a `Vec<u8>` to not incur overhead from deserializing
struct WebServerState {
    //view number -> proposals
    proposals: HashMap<u128, Vec<Vec<u8>>>,
    //view for oldest proposals in memory
    oldest_proposal: u128,
    //view number -> Vec(index, vote)
    votes: HashMap<u128, Vec<(u128, Vec<u8>)>>,
    //view number -> highest vote index for that view number
    vote_index: HashMap<u128, u128>,
    //view number of oldest votes in memory
    oldest_vote: u128,
    //index -> transaction
    transactions: HashMap<u128, Vec<u8>>,
    //highest txn index
    num_txns: u128,
    //shutdown signal
    shutdown: Option<OneShotReceiver<()>>,
}

impl WebServerState {
    fn new() -> Self {
        Self {
            num_txns: 0,
            oldest_vote: 0,
            oldest_proposal: 0,
            shutdown: None,
            ..Default::default()
        }
    }
    pub fn with_shutdown_signal(mut self, shutdown_listener: Option<OneShotReceiver<()>>) -> Self {
        if self.shutdown.is_some() {
            panic!("A shutdown signal is already registered and can not be registered twice");
        }
        self.shutdown = shutdown_listener;
        self
    }
}

/// Trait defining methods needed for the `WebServerState`
pub trait WebServerDataSource {
    fn get_proposals(&self, view_number: u128) -> Result<Option<Vec<Vec<u8>>>, Error>;
    fn get_votes(&self, view_number: u128, index: u128) -> Result<Option<Vec<Vec<u8>>>, Error>;
    fn get_transactions(&self, index: u128) -> Result<Option<Vec<Vec<u8>>>, Error>;
    fn post_vote(&mut self, view_number: u128, vote: Vec<u8>) -> Result<(), Error>;
    fn post_proposal(&mut self, view_number: u128, proposal: Vec<u8>) -> Result<(), Error>;
    fn post_transaction(&mut self, txn: Vec<u8>) -> Result<(), Error>;
}

impl WebServerDataSource for WebServerState {
    /// Return all proposals the server has received for a particular view
    fn get_proposals(&self, view_number: u128) -> Result<Option<Vec<Vec<u8>>>, Error> {
        println!("Getting proposal for view {}", view_number);
        match self.proposals.get(&view_number) {
            Some(proposals) => Ok(Some(proposals.clone())),
            None => Ok(None),
        }
    }

    /// Return all votes the server has received for a particular view from provided index to most recent
    fn get_votes(&self, view_number: u128, index: u128) -> Result<Option<Vec<Vec<u8>>>, Error> {
        let votes = self.votes.get(&view_number);
        let mut ret_votes = vec![];
        if let Some(votes) = votes {
            for i in index..*self.vote_index.get(&view_number).unwrap() {
                println!("getting vote {:?} for view {:?}", i, view_number);
                ret_votes.push(votes[i as usize].1.clone());
            }
        }
        if ret_votes.len() > 0 {
            Ok(Some(ret_votes))
        } else {
            Ok(None)
        }
    }

    /// Return all transactions from provided index to most recent
    fn get_transactions(&self, index: u128) -> Result<Option<Vec<Vec<u8>>>, Error> {
        let mut txns = vec![];
        for i in index..self.num_txns {
            println!("getting txn {:?}", i);
            if let Some(txn) = self.transactions.get(&i) {
                txns.push(txn.clone())
            }
        }
        if txns.len() > 0 {
            Ok(Some(txns))
        } else {
            Ok(None)
        }
    }

    /// Stores a received vote in the `WebServerState`
    fn post_vote(&mut self, view_number: u128, vote: Vec<u8>) -> Result<(), Error> {
        println!("Posting vote for view {}", view_number);

        // only keep vote history for MAX_VIEWS number of views
        if self.votes.len() >= MAX_VIEWS {
            self.votes.remove(&self.oldest_vote);
            while !self.votes.contains_key(&self.oldest_vote) {
                self.oldest_vote += 1;
            }
        }
        let highest_index = self.vote_index.entry(view_number).or_insert(0);
        self.votes
            .entry(view_number)
            .and_modify(|current_votes| current_votes.push((*highest_index, vote.clone())))
            .or_insert_with(|| vec![(*highest_index, vote)]);
        self.vote_index
            .entry(view_number)
            .and_modify(|index| *index += 1);
        Ok(())
    }
    /// Stores a received proposal in the `WebServerState`
    fn post_proposal(&mut self, view_number: u128, proposal: Vec<u8>) -> Result<(), Error> {
        println!("posting proposal for view {}", view_number);

        // only keep proposal history for MAX_VIEWS number of view
        if self.proposals.len() >= MAX_VIEWS {
            self.proposals.remove(&self.oldest_proposal);
            while !self.proposals.contains_key(&self.oldest_proposal) {
                self.oldest_proposal += 1;
            }
        }
        self.proposals
            .entry(view_number)
            .and_modify(|current_proposals| current_proposals.push(proposal.clone()))
            .or_insert_with(|| vec![proposal]);
        Ok(())
    }
    /// Stores a received group of transactions in the `WebServerState`
    fn post_transaction(&mut self, txn: Vec<u8>) -> Result<(), Error> {
        println!("posting txn {:?}", self.num_txns);
        //only keep MAX_TXNS in memory
        if self.transactions.len() >= MAX_TXNS {
            self.transactions
                .remove(&(self.num_txns - MAX_TXNS as u128));
        }
        self.transactions.insert(self.num_txns, txn);
        self.num_txns += 1;
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
            let index: u128 = req.integer_param("index")?;
            state.get_votes(view_number, index)
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
pub async fn run_web_server(shutdown_listener: Option<OneShotReceiver<()>>) -> io::Result<()> {
    let options = Options::default();
    let api = define_api(&options).unwrap();
    let state = State::new(WebServerState::new().with_shutdown_signal(shutdown_listener));
    let mut app = App::<State, Error>::with_state(state);

    app.register_module("api", api).unwrap();
    app.serve(format!("http://0.0.0.0:{}", WEB_SERVER_PORT))
        .await
}

#[cfg(test)]
mod test {
    use crate::config::{
        get_proposal_route, get_transactions_route, get_vote_route, post_proposal_route,
        post_transactions_route, post_vote_route,
    };

    use super::*;
    use async_std::task::spawn;
    use config;
    use portpicker::pick_unused_port;
    use surf_disco::error::ClientError;
    use tide_disco::StatusCode;

    type State = RwLock<WebServerState>;
    type Error = ServerError;
    
    #[async_std::test]
    //KALEY TODO: refactor into smaller tests
    async fn test_web_server() {
        let port = pick_unused_port().unwrap();
        let base_url = format!("0.0.0.0:{port}");
        let options = Options::default();
        let api = define_api(&options).unwrap();
        let mut app = App::<State, Error>::with_state(State::new(WebServerState::new()));

        app.register_module("api", api).unwrap();
        let _handle = spawn(app.serve(base_url.clone()));

        let base_url = format!("http://{base_url}").parse().unwrap();
        let client = surf_disco::Client::<ClientError>::new(base_url);
        assert!(client.connect(None).await);

        // Test posting and getting proposals
        let prop1 = "prop1";
        client
            .post::<()>(&post_proposal_route(1))
            .body_binary(&prop1)
            .unwrap()
            .send()
            .await
            .unwrap();
        let resp = client
            .get::<Option<Vec<Vec<u8>>>>(&get_proposal_route(1))
            .send()
            .await
            .unwrap()
            .unwrap();
        let res1: &str = bincode::deserialize(&resp[0]).unwrap();
        assert_eq!(res1, prop1);

        let prop2 = "prop2";
        client
            .post::<()>(&post_proposal_route(2))
            .body_binary(&prop2)
            .unwrap()
            .send()
            .await
            .unwrap();
        let resp = client
            .get::<Option<Vec<Vec<u8>>>>(&get_proposal_route(2))
            .send()
            .await
            .unwrap()
            .unwrap();

        let res2: &str = bincode::deserialize(&resp[0]).unwrap();
        assert_eq!(res2, prop2);
        assert_ne!(res1, res2);

        assert_eq!(
            client
                .get::<Option<Vec<u8>>>(&get_proposal_route(3))
                .send()
                .await,
            Ok(None)
        );

        // Test posting and getting votes
        let vote1 = "vote1";
        client
            .post::<()>(&post_vote_route(1))
            .body_binary(&vote1)
            .unwrap()
            .send()
            .await
            .unwrap();

        let vote2 = "vote2";
        client
            .post::<()>(&post_vote_route(1))
            .body_binary(&vote2)
            .unwrap()
            .send()
            .await
            .unwrap();
        let resp = client
            .get::<Option<Vec<Vec<u8>>>>(&get_vote_route(1, 0))
            .send()
            .await
            .unwrap()
            .unwrap();
        let res1: &str = bincode::deserialize(&resp[0]).unwrap();
        let res2: &str = bincode::deserialize(&resp[1]).unwrap();
        assert_eq!(vote1, res1);
        assert_eq!(vote2, res2);
        //check for proper indexing
        let resp = client
            .get::<Option<Vec<Vec<u8>>>>(&get_vote_route(1, 1))
            .send()
            .await
            .unwrap()
            .unwrap();
        let res: &str = bincode::deserialize(&resp[0]).unwrap();
        assert_eq!(vote2, res);

        //test posting/getting transactions
        let txns1 = "abc";
        let txns2 = "def";
        client
            .post::<()>(&post_transactions_route())
            .body_binary(&txns1)
            .unwrap()
            .send()
            .await
            .unwrap();
        client
            .post::<()>(&post_transactions_route())
            .body_binary(&txns2)
            .unwrap()
            .send()
            .await
            .unwrap();
        let resp = client
            .get::<Option<Vec<Vec<u8>>>>(&get_transactions_route(0))
            .send()
            .await
            .unwrap()
            .unwrap();

        let txn_resp1: &str = bincode::deserialize(&resp[0]).unwrap();
        let txn_resp2: &str = bincode::deserialize(&resp[1]).unwrap();
        assert_eq!(txns1, txn_resp1);
        assert_eq!(txns2, txn_resp2)
    }
}
