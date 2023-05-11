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

use self::behaviours::{
    dht::DHTEvent, direct_message::DMEvent, direct_message_codec::DirectMessageResponse,
    gossip::GossipEvent,
};
use bincode::Options;
use futures::channel::oneshot::Sender;
use hotshot_utils::bincode::bincode_opts;
use libp2p::{
    build_multiaddr,
    core::{muxing::StreamMuxerBox, transport::Boxed, upgrade::Version},
    gossipsub::TopicHash,
    identify::Event as IdentifyEvent,
    identity::Keypair,
    request_response::ResponseChannel,
    tcp,
    yamux::{WindowUpdateMode, YamuxConfig},
    Multiaddr, Transport,
};
use libp2p_identity::PeerId;
use rand::seq::IteratorRandom;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fmt::Debug, str::FromStr, sync::Arc, time::Duration};
use tracing::{info, instrument};

#[cfg(feature = "async-std-executor")]
use libp2p::dns::DnsConfig;
#[cfg(feature = "tokio-executor")]
use libp2p::dns::TokioDnsConfig as DnsConfig;
#[cfg(feature = "async-std-executor")]
use tcp::async_io::Transport as TcpTransport;
#[cfg(feature = "tokio-executor")]
use tcp::tokio::Transport as TcpTransport;
#[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
std::compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}

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

/// Serialize an arbitrary message
/// # Errors
/// When unable to serialize a message
pub fn serialize_msg<T: Serialize>(msg: &T) -> Result<Vec<u8>, Box<bincode::ErrorKind>> {
    bincode_opts().serialize(&msg)
}

/// Deserialize an arbitrary message
/// # Errors
/// When unable to deserialize a message
pub fn deserialize_msg<'a, T: Deserialize<'a>>(
    msg: &'a [u8],
) -> Result<T, Box<bincode::ErrorKind>> {
    bincode_opts().deserialize(msg)
}

impl Default for NetworkNodeType {
    fn default() -> Self {
        Self::Bootstrap
    }
}

/// Actions to send from the client to the swarm
#[derive(Debug)]
pub enum ClientRequest {
    /// Start the bootstrap process to kademlia
    BeginBootstrap,
    /// kill the swarm
    Shutdown,
    /// broadcast a serialized message
    GossipMsg(String, Vec<u8>),
    /// subscribe to a topic
    Subscribe(String, Option<Sender<()>>),
    /// unsubscribe from a topic
    Unsubscribe(String, Sender<()>),
    /// client request to send a direct serialized message
    DirectRequest {
        /// peer id
        pid: PeerId,
        /// msg contents
        contents: Vec<u8>,
        /// number of retries
        retry_count: u8,
    },
    /// client request to send a direct reply to a message
    DirectResponse(ResponseChannel<DirectMessageResponse>, Vec<u8>),
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
        /// number of retries to make
        retry_count: u8,
    },
    /// Request the number of connected peers
    GetConnectedPeerNum(Sender<usize>),
    /// Request the set of connected peers
    GetConnectedPeers(Sender<HashSet<PeerId>>),
    /// Print the routing  table to stderr, debugging only
    GetRoutingTable(Sender<()>),
    /// Get address of peer
    LookupPeer(PeerId, Sender<()>),
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

#[derive(Debug)]
/// internal representation of the network events
/// only used for event processing before relaying to client
pub enum NetworkEventInternal {
    /// a DHT event
    DHTEvent(DHTEvent),
    /// a identify event. Is boxed because this event is much larger than the other ones so we want
    /// to store it on the heap.
    IdentifyEvent(Box<IdentifyEvent>),
    /// a gossip  event
    GossipEvent(GossipEvent),
    /// a direct message event
    DMEvent(DMEvent),
}

/// Bind all interfaces on port `port`
/// NOTE we may want something more general in the fture.
#[must_use]
pub fn gen_multiaddr(port: u16) -> Multiaddr {
    build_multiaddr!(Ip4([0, 0, 0, 0]), Tcp(port))
}

/// Generate authenticated transport, copied from `development_transport`
/// <http://noiseprotocol.org/noise.html#payload-security-properties> for definition of XX
/// and multiplexing from example
/// <https://github.com/mxinden/libp2p-lookup/blob/master/src/main.rs>
/// # Errors
/// could not sign the noise key with `identity`
#[instrument(skip(identity))]
pub async fn gen_transport(
    identity: Keypair,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>, NetworkError> {
    let transport = async move {
        let dns_tcp = DnsConfig::system(TcpTransport::new(tcp::Config::new().nodelay(true)));

        #[cfg(feature = "async-std-executor")]
        return dns_tcp
            .await
            .map_err(|e| NetworkError::TransportLaunch { source: e });

        #[cfg(feature = "tokio-executor")]
        return dns_tcp.map_err(|e| NetworkError::TransportLaunch { source: e });

        #[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
        compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}
    }
    .await?;

    let multiplexing_config = {
        let mut yamux_config = YamuxConfig::default();
        yamux_config.set_window_update_mode(WindowUpdateMode::on_read());
        yamux_config
    };

    Ok(transport
        .upgrade(Version::V1)
        .authenticate(libp2p_noise::Config::new(&identity).unwrap())
        .multiplex(multiplexing_config)
        .timeout(std::time::Duration::from_secs(20))
        .boxed())
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
    handle.wait_to_connect(4, idx, timeout_len).await?;
    handle.subscribe("global".to_string()).await?;

    Ok(())
}

/// Given a slice of handles assumed to be larger than 0,
/// chooses one
/// # Panics
/// panics if handles is of length 0
pub fn get_random_handle<S>(
    handles: &[Arc<NetworkNodeHandle<S>>],
    rng: &mut dyn rand::RngCore,
) -> Arc<NetworkNodeHandle<S>> {
    handles.iter().choose(rng).unwrap().clone()
}
