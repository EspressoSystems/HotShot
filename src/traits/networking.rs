//! Network access abstraction
//!
//! This module contains a trait abstracting over network access, as well as implementations of that
//! trait. Currently this includes
//! - [`MemoryNetwork`](memory_network::MemoryNetwork), an in memory testing-only implementation
//! - [`WNetwork`](w_network::WNetwork), a prototype/testing websockets implementation.
//! - [`Libp2pNetwork`](libp2p_network::Libp2pNetwork), a production-ready networking impelmentation built on top of libp2p-rs.

pub mod libp2p_network;
pub mod memory_network;
pub mod w_network;

pub use hotshot_types::traits::network::{
    ChannelSendSnafu, CouldNotDeliverSnafu, ExecutorSnafu, FailedToBindListenerSnafu,
    FailedToDeserializeSnafu, FailedToSerializeSnafu, IdentityHandshakeSnafu, ListenerSendSnafu,
    NetworkError, NetworkReliability, NetworkingImplementation, NoSocketsSnafu, NoSuchNodeSnafu,
    OtherSnafu, ShutDownSnafu, SocketDecodeSnafu, WebSocketSnafu,
};
