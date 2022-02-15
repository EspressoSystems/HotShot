//! Network access abstraction
//!
//! This module contains a trait abstracting over network access, as well as implementations of that
//! trait. Currently this includes [`MemoryNetwork`](memory_network::MemoryNetwork), an in memory
//! testing-only implementation, and [`WNetwork`](w_network::WNetwork), a prototype/testing
//! websockets implementation.
//!
//! In future, this module will contain a production ready networking implementation, very probably
//! one libp2p based.

pub mod memory_network;
pub mod w_network;

pub use phaselock_types::traits::network::{
    BoxedFuture, ChannelSendSnafu, CouldNotDeliverSnafu, ExecutorSnafu, FailedToBindListenerSnafu,
    FailedToDeserializeSnafu, FailedToSerializeSnafu, IdentityHandshakeSnafu, ListenerSendSnafu,
    NetworkError, NetworkReliability, NetworkingImplementation, NoSocketsSnafu, NoSuchNodeSnafu,
    OtherSnafu, ShutDownSnafu, SocketDecodeSnafu, WebSocketSnafu,
};
