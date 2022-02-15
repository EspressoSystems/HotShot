//! Network access abstraction
//!
//! This module contains a trait abstracting over network access, as well as implementations of that
//! trait. Currently this includes [`MemoryNetwork`](memory_network::MemoryNetwork), an in memory
//! testing-only implementation, and [`WNetwork`](w_network::WNetwork), a prototype/testing
//! websockets implementation.
//!
//! In future, this module will contain a production ready networking implementation, very probably
//! one libp2p based.

use crate::PubKey;

use async_tungstenite::tungstenite::error as werror;
use futures::future::BoxFuture;
use serde::{de::DeserializeOwned, Serialize};
use snafu::Snafu;
use std::time::Duration;

pub mod memory_network;
pub mod w_network;

pub use phaselock_types::traits::network::{
    BoxedFuture, ChannelSendSnafu, CouldNotDeliverSnafu, ExecutorSnafu, FailedToBindListenerSnafu,
    FailedToDeserializeSnafu, FailedToSerializeSnafu, IdentityHandshakeSnafu, ListenerSendSnafu,
    NetworkError, NetworkingImplementation, NoSocketsSnafu, NoSuchNodeSnafu, OtherSnafu,
    ShutDownSnafu, SocketDecodeSnafu, WebSocketSnafu,
};
