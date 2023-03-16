pub mod config;

use async_lock::RwLock;
use hotshot_types::traits::election::ElectionConfig;
use hotshot_types::traits::signature_key::SignatureKey;
use libp2p_core::identity;
use std::io;
use std::io::ErrorKind;
use std::net::IpAddr;
use std::net::SocketAddr;
use tide_disco::Api;
use tide_disco::App;
use tracing::log::error;

use tide_disco::api::ApiError;
use tide_disco::error::ServerError;
use tide_disco::method::ReadState;
use tide_disco::method::WriteState;

use futures::FutureExt;

use crate::config::NetworkConfig;

use blake3;
use libp2p::{
    identity::{
        ed25519::{Keypair as EdKeypair, SecretKey},
        Keypair,
    },
    multiaddr::{self, Protocol},
    Multiaddr, PeerId,
};
use libp2p_networking::network::{MeshParams, NetworkNodeConfigBuilder, NetworkNodeType};

// /// yeesh maybe we should just implement SignatureKey for this...
pub fn libp2p_generate_indexed_identity(seed: [u8; 32], index: u64) -> Keypair {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&seed);
    hasher.update(&index.to_le_bytes());
    let new_seed = *hasher.finalize().as_bytes();
    let sk_bytes = SecretKey::from_bytes(new_seed).unwrap();
    let ed_kp = <EdKeypair as From<SecretKey>>::from(sk_bytes);
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

    /// Whether this is for a libp2p network configuration
    is_libp2p: bool,
}

impl<KEY: SignatureKey + 'static, ELECTION: ElectionConfig + 'static>
    OrchestratorState<KEY, ELECTION>
{
    pub fn new(network_config: NetworkConfig<KEY, ELECTION>) -> Self {
        OrchestratorState {
            latest_index: 0,
            config: network_config.clone(),
            start: false,
            nodes_connected: 0,
            is_libp2p: network_config.libp2p_config.is_some(),
        }
    }
}

pub trait OrchestratorApi<KEY, ELECTION> {
    fn post_identity(&mut self, identity: &str) -> Result<u16, ServerError>;
    fn post_getconfig(
        &mut self,
        node_index: u16,
    ) -> Result<NetworkConfig<KEY, ELECTION>, ServerError>;
    fn get_start(&self) -> Result<bool, ServerError>;
    fn post_ready(&mut self) -> Result<(), ServerError>;
    fn post_run_results(&mut self) -> Result<(), ServerError>;
}

impl<KEY, ELECTION> OrchestratorApi<KEY, ELECTION> for OrchestratorState<KEY, ELECTION>
where
    KEY: serde::Serialize + Clone,
    ELECTION: serde::Serialize + Clone,
{
    fn post_identity(&mut self, identity: &str) -> Result<u16, ServerError> {
        // TODO ED Move this constant out of function / add it to the config file
        let NUM_BOOTSTRAP_NODES = 5;
        let node_index = self.latest_index;
        self.latest_index += 1;

        // TODO ED Store identity for bootstrap nodes if needed
        if self.config.libp2p_config.clone().is_some() {
            if self
                .config
                .libp2p_config
                .as_mut()
                .unwrap()
                .bootstrap_nodes
                .len()
                < NUM_BOOTSTRAP_NODES
            {
                // TODO ED clean this up so not so many dots
                // println!("{:?}", identity);
                // let mut addr = identity.parse::<SocketAddr>().unwrap();
                let mut addr = identity.parse::<IpAddr>().unwrap();

                // println!("{:?}", addr);
                // addr.set_port(self.config.libp2p_config.as_mut().unwrap().base_port + node_index);
                // println!("{:?}", addr);
                let socketaddr = SocketAddr::new(addr, 9000 + node_index);
                // let addr = SocketAddr::new(identity.parse::<SocketAddr>().unwrap(), self.config.libp2p_config.as_mut().unwrap().base_port + node_index);
                // TODO ED just pass the public key in the future
                let keypair = libp2p_generate_indexed_identity(self.config.seed, node_index.into());
                self.config
                    .libp2p_config
                    .as_mut()
                    .unwrap()
                    .bootstrap_nodes
                    .push((socketaddr, keypair.to_protobuf_encoding().unwrap()));

                // let addr = SocketAddr::new("0.0.0.0".parse().unwrap(), 4444);
            }
            // self.config.libp2p_config.
            // Vec<(SocketAddr, Vec<u8, Global>)
        }

        // TODO https://github.com/EspressoSystems/HotShot/issues/850
        // TODO ED move this above
        if usize::from(node_index) >= self.config.config.total_nodes.get() {
            return Err(ServerError {
                status: tide_disco::StatusCode::BadRequest,
                message: "Network has reached capacity".to_string(),
            });
        }
        Ok(node_index)
    }

    fn post_getconfig(
        &mut self,
        node_index: u16,
    ) -> Result<NetworkConfig<KEY, ELECTION>, ServerError> {
        let NUM_BOOTSTRAP_NODES = 5;
        let mut config = self.config.clone();
        if config.libp2p_config.is_some() {
            if self
                .config
                .libp2p_config
                .as_mut()
                .unwrap()
                .bootstrap_nodes
                .len()
                < NUM_BOOTSTRAP_NODES
            {
                // return error until we have the bootstrap nodes ready TODO ED
                // TODO ED Make custom error
                return Err(ServerError {
                    status: tide_disco::StatusCode::BadRequest,
                    message: "Not enough bootstrap nodes have registered".to_string(),
                });
            }
        }
        // TODO ED Needs to take in their node index as an argument
        // config.node_index = self.latest_index.into();

        // self.latest_index += 1;
        // TODO ED
        Ok(config)
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
    let mut api = Api::<State, ServerError>::from_file("orchestrator/api.toml")
        .expect("api.toml file is not found");
    api.post("postidentity", |req, state| {
        async move {
            // println!("{:?}", req.);
            // let identity = req.string_param("identity")?;
            // let identity = req.remote();
            let identity = req.string_param("identity");

            println!("{:?}", identity);
            // TODO ED error check the unwrap
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
        async move { state.post_run_results() }.boxed()
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
    // TODO ED If libp2p run, generate libp2p keys

    let api = define_api().map_err(|_e| io::Error::new(ErrorKind::Other, "Failed to define api"));

    let state: RwLock<OrchestratorState<KEY, ELECTION>> =
        RwLock::new(OrchestratorState::new(network_config));
    let mut app = App::<RwLock<OrchestratorState<KEY, ELECTION>>, ServerError>::with_state(state);
    app.register_module("api", api.unwrap())
        .expect("Error registering api");
    app.serve(format!("http://{host}:{port}")).await
}
