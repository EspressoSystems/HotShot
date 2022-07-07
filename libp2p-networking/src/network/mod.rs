/// networking behaviours wrapping libp2p's behaviours
pub mod behaviours;
mod def;
pub mod error;
mod node;

pub use self::{
    def::NetworkDef,
    error::NetworkError,
    node::{
        network_node_handle_error, MeshParams, NetworkNode, NetworkNodeConfig,
        NetworkNodeConfigBuilder, NetworkNodeConfigBuilderError, NetworkNodeHandle,
        NetworkNodeHandleError,
    },
};

use self::{
    behaviours::direct_message_codec::DirectMessageResponse,
    node::network_node_handle_error::TimeoutSnafu,
};
use async_std::{prelude::FutureExt as _, task::spawn};
use bincode::Options;
use futures::{channel::oneshot::Sender, select, Future, FutureExt};
use libp2p::{
    build_multiaddr,
    core::{muxing::StreamMuxerBox, transport::Boxed, upgrade},
    dns,
    gossipsub::TopicHash,
    identity::Keypair,
    mplex, noise,
    request_response::ResponseChannel,
    tcp,
    yamux::{self},
    Multiaddr, PeerId, Transport,
};
use rand::{seq::IteratorRandom, thread_rng};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use std::{collections::HashSet, fmt::Debug, str::FromStr, sync::Arc, time::Duration};
use tracing::{info, info_span, instrument, Instrument};

/// metadata about connections
#[derive(Default, Debug, Clone)]
pub struct ConnectionData {
    /// set of currently connecting peers
    pub connected_peers: HashSet<PeerId>,
    /// set of peers that were at one point connected
    pub connecting_peers: HashSet<PeerId>,
    /// set of known peers
    pub known_peers: HashSet<PeerId>,
    /// set of peers that are immune to pruning
    pub ignored_peers: HashSet<PeerId>,
}

/// this is mostly to estimate how many network connections
/// a node should allow
#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum NetworkNodeType {
    /// bootstrap node accepts all connections
    Bootstrap,
    /// regular node has a limit to the
    /// number of connections to accept
    Regular,
    /// conductor node is never pruned
    Conductor,
}

impl FromStr for NetworkNodeType {
    type Err = String;

    fn from_str(input: &str) -> Result<NetworkNodeType, Self::Err> {
        match input {
            "Conductor" => Ok(NetworkNodeType::Conductor),
            "Regular" => Ok(NetworkNodeType::Regular),
            "Bootstrap" => Ok(NetworkNodeType::Bootstrap),
            _ => Err(
                "Couldn't parse node type. Must be one of Conductor, Bootstrap, Regular"
                    .to_string(),
            ),
        }
    }
}

/// serialize an arbitrary message
/// # Errors
/// when unable to serialize a message
pub fn serialize_msg<T: Serialize>(msg: &T) -> Result<Vec<u8>, Box<bincode::ErrorKind>> {
    let bincode_options = bincode::DefaultOptions::new()/* .with_limit(MAX_MSG_SIZE as u64) */;
    bincode_options.serialize(&msg)
}

/// deserialize an arbitrary message
/// # Errors
/// when unable to deserialize a message
pub fn deserialize_msg<'a, T: Deserialize<'a>>(
    msg: &'a [u8],
) -> Result<T, Box<bincode::ErrorKind>> {
    let bincode_options = bincode::DefaultOptions::new()/* .with_limit(MAX_MSG_SIZE as u64) */;
    bincode_options.deserialize(msg)
}

impl Default for NetworkNodeType {
    fn default() -> Self {
        Self::Bootstrap
    }
}

/// Actions to send from the client to the swarm
#[derive(Debug)]
pub enum ClientRequest {
    /// kill the swarm
    Shutdown,
    /// broadcast a serialized message
    GossipMsg(String, Vec<u8>),
    /// subscribe to a topic
    Subscribe(String, Option<Sender<()>>),
    /// unsubscribe from a topic
    Unsubscribe(String, Sender<()>),
    /// client request to send a direct serialized message
    DirectRequest(PeerId, Vec<u8>),
    /// client request to send a direct reply to a message
    DirectResponse(ResponseChannel<DirectMessageResponse>, Vec<u8>),
    /// disable or enable pruning of connections
    Pruning(bool),
    /// prune a peer
    Prune(PeerId),
    /// add vec of known peers or addresses
    AddKnownPeers(Vec<(Option<PeerId>, Multiaddr)>),
    /// Ignore peers. Only here for debugging purposes.
    /// Allows us to have nodes that are never pruned
    IgnorePeers(Vec<PeerId>),
    /// Put(Key, Value) into DHT
    /// relay success back on channel
    PutDHT {
        /// Key to publish under
        key: Vec<u8>,
        /// Value to publish under
        value: Vec<u8>,
        /// Channel to notify caller of result of publishing
        notify: Sender<()>,
    },
    /// Get(Key, Chan)
    GetDHT {
        /// Key to search for
        key: Vec<u8>,
        /// Channel to notify caller of value (or failure to find value)
        notify: Sender<Vec<u8>>,
    },
    /// Request the number of connected peers
    GetConnectedPeerNum(Sender<usize>),
    /// Request the set of connected peers
    GetConnectedPeers(Sender<HashSet<PeerId>>),
}

/// events generated by the swarm that we wish
/// to relay to the client
#[derive(Debug)]
pub enum NetworkEvent {
    /// Recv-ed a broadcast
    GossipMsg(Vec<u8>, TopicHash),
    /// Recv-ed a direct message from a node
    DirectRequest(Vec<u8>, PeerId, ResponseChannel<DirectMessageResponse>),
    /// Recv-ed a direct response from a node (that hopefully was initiated by this node)
    DirectResponse(Vec<u8>, PeerId),
    /// Report that kademlia has successfully bootstrapped into the network
    IsBootstrapped,
}

/// bind all interfaces on port `port`
/// TODO something more general
pub fn gen_multiaddr(port: u16) -> Multiaddr {
    build_multiaddr!(Ip4([0, 0, 0, 0]), Tcp(port))
}

/// Generate authenticated transport, copied from `development_transport`
/// <http://noiseprotocol.org/noise.html#payload-security-properties> for definition of XX
/// # Errors
/// could not sign the noise key with `identity`
#[instrument(skip(identity))]
pub async fn gen_transport(
    identity: Keypair,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>, NetworkError> {
    let transport = {
        let dns_tcp = dns::DnsConfig::system(tcp::TcpTransport::new(
            tcp::GenTcpConfig::new().nodelay(true),
        ));
        dns_tcp
            .await
            .map_err(|e| NetworkError::TransportLaunch { source: e })?
    };

    // keys for signing messages
    let noise_keys = noise::Keypair::<noise::X25519Spec>::new()
        .into_authentic(&identity)
        .expect("Signing libp2p-noise static DH keypair failed.");
    let _yamux_cfg = yamux::YamuxConfig::default();
    // yamux_cfg.set_max_buffer_size(1024 * 1024 * 32);
    // yamux_cfg.set_receive_window_size((16384 * 32) as u32);
    // yamux_cfg.set_window_update_mode(yamux::WindowUpdateMode::OnRead);
    let mplex_cfg = mplex::MplexConfig::default();
    // mplex_cfg.set_max_num_streams(1024);
    // mplex_cfg.set_max_buffer_size(usize::MAX);
    // mplex_cfg.set_split_send_size(8*8*8*1024);

    Ok(transport
        .upgrade(upgrade::Version::V1)
        // authentication: messages are signed
        .authenticate(noise::NoiseConfig::xx(noise_keys).into_authenticated())
        // muxxing streams
        // useful because only one connection opened
        // https://docs.libp2p.io/concepts/stream-multiplexing/
        .multiplex(
            mplex_cfg, // yamux_cfg
        )
        // .timeout(std::time::Duration::from_secs(20))
        .boxed())
}

/// Glue function that listens for events from the Swarm corresponding to `handle`
/// and calls `event_handler` when an event is observed.
/// The idea is that this function can be used independent of the actual state
/// we use
#[allow(clippy::panic)]
#[instrument(skip(event_handler))]
pub async fn spawn_handler<S: 'static + Send + Default + Debug, Fut>(
    handle: Arc<NetworkNodeHandle<S>>,
    event_handler: impl (Fn(NetworkEvent, Arc<NetworkNodeHandle<S>>) -> Fut)
        + std::marker::Sync
        + std::marker::Send
        + 'static,
) where
    Fut: Future<Output = Result<(), NetworkNodeHandleError>> + std::marker::Send + 'static,
{
    let recv_kill = handle.recv_kill();
    let recv_event = handle.recv_network();
    spawn(
        async move {
            loop {
                select!(
                    _ = recv_kill.recv_async().fuse() => {
                        handle.mark_killed().await;
                        break;
                    },
                    event = recv_event.recv_async().fuse() => {
                        event_handler(event.map_err(|_| NetworkNodeHandleError::RecvError)?, handle.clone()).await?;
                    },
                );
            }
            Ok::<(), NetworkNodeHandleError>(())
        }
        .instrument(info_span!("Libp2p Counter Handler")),
    );
}

/// a single node, connects them to each other
/// and waits for connections to propagate to all nodes.
#[instrument]
pub async fn spin_up_swarm<S: std::fmt::Debug + Default>(
    timeout_len: Duration,
    known_nodes: Vec<(Option<PeerId>, Multiaddr)>,
    config: NetworkNodeConfig,
    idx: usize,
    handle: &Arc<NetworkNodeHandle<S>>,
) -> Result<(), NetworkNodeHandleError> {
    info!("known_nodes{:?}", known_nodes);
    handle.add_known_peers(known_nodes).await?;
    NetworkNodeHandle::wait_to_connect(handle.clone(), 4, handle.recv_network(), idx)
        .timeout(timeout_len)
        .await
        .context(TimeoutSnafu)??;
    handle.subscribe("global".to_string()).await?;

    Ok(())
}

/// Given a slice of handles assumed to be larger than 0,
/// chooses one
/// # Panics
/// panics if handles is of length 0
pub fn get_random_handle<S>(handles: &[Arc<NetworkNodeHandle<S>>]) -> Arc<NetworkNodeHandle<S>> {
    handles.iter().choose(&mut thread_rng()).unwrap().clone()
}
