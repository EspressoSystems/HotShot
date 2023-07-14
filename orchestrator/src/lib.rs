pub mod config;

use async_lock::RwLock;
use hotshot_types::traits::election::ElectionConfig;
use hotshot_types::traits::signature_key::SignatureKey;
use std::io;
use std::vec::Vec;
use std::io::ErrorKind;
use std::net::IpAddr;
use std::net::SocketAddr;
use tide_disco::Api;
use tide_disco::App;

use surf_disco::error::ClientError;
use tide_disco::api::ApiError;
use tide_disco::error::ServerError;
use tide_disco::method::ReadState;
use tide_disco::method::WriteState;
use serde::{Deserialize, Serialize};

use futures::FutureExt;

use crate::config::NetworkConfig;

use libp2p::identity::{
    ed25519::{Keypair as EdKeypair, SecretKey},
    Keypair,
};

/// yeesh maybe we should just implement SignatureKey for this...
pub fn libp2p_generate_indexed_identity(seed: [u8; 32], index: u64) -> Keypair {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&seed);
    hasher.update(&index.to_le_bytes());
    let new_seed = *hasher.finalize().as_bytes();
    let sk_bytes = SecretKey::try_from_bytes(new_seed).unwrap();
    let ed_kp = <EdKeypair as From<SecretKey>>::from(sk_bytes);
    #[allow(deprecated)]
    Keypair::Ed25519(ed_kp)
}

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
    /// connection to the web server
    client: Option<surf_disco::Client<ClientError>>,


    // Statistics Variables: track statistics for benchmarking
    pub stat_viewtime: Vec<u64>,
    pub stat_throughput: Vec<u64>,
    pub stat_runduration: Vec<u64>,
    pub stat_nodes_collected: u64,
}

// Statistics struct
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct StatisticsStruct{
    pub stat_viewtime: Vec<u64>,
    pub stat_throughput: Vec<u64>,
    pub stat_runduration: Vec<u64>, 
}

// Average Function
fn average(numbers: &mut Vec<u64>) -> u64 {
    numbers.iter().sum::<u64>() as u64 / numbers.len() as u64
}    

impl<KEY: SignatureKey + 'static, ELECTION: ElectionConfig + 'static>
    OrchestratorState<KEY, ELECTION>
{
    pub fn new(network_config: NetworkConfig<KEY, ELECTION>) -> Self {
        let mut web_client = None;
        if network_config.web_server_config.is_some() {
            let base_url = "http://0.0.0.0/9000".to_string().parse().unwrap();
            web_client = Some(surf_disco::Client::<ClientError>::new(base_url));
        }
        OrchestratorState {
            latest_index: 0,
            config: network_config,
            start: false,
            nodes_connected: 0,
            client: web_client,

            // Statistics variables
            stat_viewtime: Vec::new(),
            stat_throughput: Vec::new(),
            stat_runduration: Vec::new(),
            stat_nodes_collected: 0,
        }
        
    }
}

pub trait OrchestratorApi<KEY, ELECTION> {
    fn post_identity(&mut self, identity: IpAddr) -> Result<u16, ServerError>;
    fn post_getconfig(
        &mut self,
        node_index: u16,
    ) -> Result<NetworkConfig<KEY, ELECTION>, ServerError>;
    fn get_start(&self) -> Result<bool, ServerError>;
    fn post_ready(&mut self) -> Result<(), ServerError>;
    fn post_run_results(&mut self, _data: StatisticsStruct) -> Result<(), ServerError>;
}

impl<KEY, ELECTION> OrchestratorApi<KEY, ELECTION> for OrchestratorState<KEY, ELECTION>
where
    KEY: serde::Serialize + Clone + SignatureKey,
    ELECTION: serde::Serialize + Clone + Send,
{
    fn post_identity(&mut self, identity: IpAddr) -> Result<u16, ServerError> {
        let node_index = self.latest_index;
        self.latest_index += 1;

        // TODO https://github.com/EspressoSystems/HotShot/issues/850
        if usize::from(node_index) >= self.config.config.total_nodes.get() {
            return Err(ServerError {
                status: tide_disco::StatusCode::BadRequest,
                message: "Network has reached capacity".to_string(),
            });
        }

        //add new node's key to stake table
        if self.config.web_server_config.clone().is_some() {
            let new_key = KEY::generated_from_seed_indexed(self.config.seed, node_index.into()).0;
            let client_clone = self.client.clone().unwrap();
            async move {
                client_clone
                    .post::<()>("api/staketable")
                    .body_binary(&new_key)
                    .unwrap()
                    .send()
                    .await
            }
            .boxed();
        }

        if self.config.libp2p_config.clone().is_some() {
            let libp2p_config_clone = self.config.libp2p_config.clone().unwrap();
            // Designate node as bootstrap node and store its identity information
            if libp2p_config_clone.bootstrap_nodes.len()
                < libp2p_config_clone.num_bootstrap_nodes.try_into().unwrap()
            {
                let port_index = match libp2p_config_clone.index_ports {
                    true => node_index,
                    false => 0,
                };
                let socketaddr =
                    SocketAddr::new(identity, libp2p_config_clone.base_port + port_index);
                let keypair = libp2p_generate_indexed_identity(self.config.seed, node_index.into());
                self.config
                    .libp2p_config
                    .as_mut()
                    .unwrap()
                    .bootstrap_nodes
                    .push((socketaddr, keypair.to_protobuf_encoding().unwrap()));
            }
        }
        Ok(node_index)
    }

    // Assumes nodes will set their own index that they received from the
    // 'identity' endpoint
    fn post_getconfig(
        &mut self,
        _node_index: u16,
    ) -> Result<NetworkConfig<KEY, ELECTION>, ServerError> {
        if self.config.libp2p_config.is_some() {
            let libp2p_config = self.config.clone().libp2p_config.unwrap();
            if libp2p_config.bootstrap_nodes.len()
                < libp2p_config.num_bootstrap_nodes.try_into().unwrap()
            {
                return Err(ServerError {
                    status: tide_disco::StatusCode::BadRequest,
                    message: "Not enough bootstrap nodes have registered".to_string(),
                });
            }
        }
        Ok(self.config.clone())
    }

    fn get_start(&self) -> Result<bool, ServerError> {
        println!("{}", self.start);
        if !self.start {
            return Err(ServerError {
                status: tide_disco::StatusCode::BadRequest,
                message: "Network is not ready to start".to_string(),
            });
        }
        Ok(self.start)
    }

    // Assumes nodes do not post 'ready' twice
    // TODO ED Add a map to verify which nodes have posted they're ready
    fn post_ready(&mut self) -> Result<(), ServerError> {
        self.nodes_connected += 1;
        println!("Nodes connected: {}", self.nodes_connected);
        if self.nodes_connected >= self.config.config.known_nodes.len().try_into().unwrap() {
            self.start = true;
        }
        Ok(())
    }

    fn post_run_results(&mut self, _data: StatisticsStruct) -> Result<(), ServerError> {

        //1. continue to gather the node data until all nodes have passed the data (Option datatype)
        self.stat_nodes_collected += 1;
        println!("Nodes statistics collected: {}", self.stat_nodes_collected);
        
        let mut data_cloned =  _data.clone();
        //For reference, we are assuming that 0th index: view time, 1th index: run duration, 2nd index: throughput
        //2. save the nodes data into the struct creatd (results_struct)
        let _ = &self.stat_viewtime.append(&mut data_cloned.stat_viewtime);
        let _= &self.stat_runduration.append(&mut data_cloned.stat_runduration);
        let _= &self.stat_throughput.append(&mut data_cloned.stat_throughput);

        // All nodes have completed there protocol and sent statistics
        if self.stat_nodes_collected >= self.config.config.known_nodes.len().try_into().unwrap(){
            //3. average the statistics 
            let avg_viewtime = average(&mut self.stat_viewtime);
            let avg_runduration = average(&mut self.stat_runduration);
            let avg_throughput = average(&mut self.stat_throughput);

            //4. print out the statistics average 
            println!("Statistics: ViewTime -- {:.?}", avg_viewtime);
            println!("Statistics: Run Duration -- {:.?}", avg_runduration);
            println!("Statistics: Throuput -- {:.?}", avg_throughput);
        }

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
    let mut api = Api::<State, ServerError>::from_file("../orchestrator/api.toml")
        .expect("api.toml file is not found");
    api.post("postidentity", |req, state| {
        async move {
            let identity = req.string_param("identity")?.parse::<IpAddr>();
            if identity.is_err() {
                return Err(ServerError {
                    status: tide_disco::StatusCode::BadRequest,
                    message: "Identity is not a properly formed IP address".to_string(),
                });
            }
            state.post_identity(identity.unwrap())
        }
        .boxed()
    })?
    .post("post_getconfig", |req, state| {
        async move {
            let node_index = req.integer_param("node_index")?;
            state.post_getconfig(node_index)
        }
        .boxed()
    })?
    .post("postready", |_req, state| {
        async move { state.post_ready() }.boxed()
    })?
    .get("getstart", |_req, state| {
        async move { state.get_start() }.boxed()
    })?
    .post("postresults", |_req, state| {
        async move { 
            let data = _req.body_auto().unwrap();
            state.post_run_results(data) 
        }.boxed()
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