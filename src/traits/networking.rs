//! Network access abstraction
//!
//! This module contains a trait abstracting over network access, as well as implementations of that
//! trait. Currently this includes
//! - [`MemoryNetwork`](memory_network::MemoryNetwork), an in memory testing-only implementation
//! - [`WNetwork`](w_network::WNetwork), a prototype/testing websockets implementation.
//! - [`Libp2pNetwork`](libp2p_network::Libp2pNetwork), a production-ready networking impelmentation built on top of libp2p-rs.

pub mod centralized_server_network;
pub mod centralized_web_server_network;
pub mod libp2p_network;
pub mod memory_network;
#[cfg(feature = "async-std-executor")]
pub mod w_network;

pub use hotshot_types::traits::network::{
    ChannelDisconnectedSnafu, ChannelSendSnafu, CouldNotDeliverSnafu, ExecutorSnafu,
    FailedToBindListenerSnafu, FailedToDeserializeSnafu, FailedToSerializeSnafu,
    IdentityHandshakeSnafu, ListenerSendSnafu, NetworkError, NetworkReliability,
    NetworkingImplementation, NoSocketsSnafu, NoSuchNodeSnafu, OtherSnafu, ShutDownSnafu,
    SocketDecodeSnafu, WebSocketSnafu,
};

use hotshot_types::traits::metrics::{Counter, Gauge, Metrics};

/// Contains the metrics that we're interested in from the networking interfaces
pub(self) struct NetworkingMetrics {
    /// A [`Gauge`] which tracks how many peers are connected
    pub connected_peers: Box<dyn Gauge>,
    /// A [`Counter`] which tracks how many messages have been received
    pub incoming_message_count: Box<dyn Counter>,
    /// A [`Counter`] which tracks how many messages have been send
    pub outgoing_message_count: Box<dyn Counter>,
    /// A [`Counter`] which tracks how many messages failed to send
    pub message_failed_to_send: Box<dyn Counter>,
    /// A [`Gauge`] which tracks how many connected entries there are in the gossipsub mesh
    pub gossipsub_mesh_connected: Box<dyn Gauge>,
    // /// A [`Gauge`] which tracks how many kademlia entries there are
    // pub kademlia_entries: Box<dyn Gauge>,
    // /// A [`Gauge`] which tracks how many kademlia buckets there are
    // pub kademlia_buckets: Box<dyn Gauge>,
}

impl NetworkingMetrics {
    /// Create a new instance of this [`NetworkingMetrics`] struct, setting all the counters and gauges
    #[allow(clippy::needless_pass_by_value)] // with the metrics API is it more ergonomic to pass a `Box<dyn Metrics>` around
    pub(self) fn new(metrics: Box<dyn Metrics>) -> Self {
        Self {
            connected_peers: metrics.create_gauge(String::from("connected_peers"), None),
            incoming_message_count: metrics
                .create_counter(String::from("incoming_message_count"), None),
            outgoing_message_count: metrics
                .create_counter(String::from("outgoing_message_count"), None),
            message_failed_to_send: metrics
                .create_counter(String::from("message_failed_to_send"), None),
            gossipsub_mesh_connected: metrics
                .create_gauge(String::from("gossipsub_mesh_connected"), None),
            // kademlia_entries: metrics.create_gauge(String::from("kademlia_entries"), None),
            // kademlia_buckets: metrics.create_gauge(String::from("kademlia_buckets"), None),
        }
    }
}
