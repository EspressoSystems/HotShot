//! Network access abstraction
//!
//! Contains types and traits used by `PhaseLock` to abstract over network access

use async_tungstenite::tungstenite::error as werror;
use futures::future::BoxFuture;
use serde::{de::DeserializeOwned, Serialize};
use snafu::Snafu;
use std::time::Duration;

use crate::PubKey;

/// A boxed future trait object with a static lifetime
pub type BoxedFuture<T> = BoxFuture<'static, T>;

/// Error type for networking
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum NetworkError {
    /// A Listener failed to send a message
    ListenerSend,
    /// Could not deliver a message to a specified recipient
    CouldNotDeliver,
    /// Attempted to deliver a message to an unknown node
    NoSuchNode,
    /// Failed to serialize a message
    FailedToSerialize {
        /// Originating bincode error
        source: bincode::Error,
    },
    /// Failed to deserealize a message
    FailedToDeserialize {
        /// originating bincode error
        source: bincode::Error,
    },
    /// WebSockets specific error
    WebSocket {
        /// Originating websockets error
        source: werror::Error,
    },
    /// Error orginiating from within the executor
    ExecutorError {
        /// Originating async_std error
        source: async_std::io::Error,
    },
    /// Failed to decode a socket specification
    SocketDecodeError {
        /// Input that was given
        input: String,
        /// Originating io error
        source: std::io::Error,
    },
    /// Failed to bind a listener socket
    FailedToBindListener {
        /// originating io error
        source: std::io::Error,
    },
    /// No sockets were open
    NoSocketsError {
        /// Input that was given
        input: String,
    },
    /// Generic error type for compatibility if needed
    Other {
        /// Originating error
        inner: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Channel error
    ChannelSend,
    /// Could not complete handshake
    IdentityHandshake,
    /// The underlying connection has been shut down
    ShutDown,
}

/// Describes, generically, the behaviors a networking implementation must have
pub trait NetworkingImplementation<M>: Send + Sync
where
    M: Serialize + DeserializeOwned + Send + Clone + 'static,
{
    /// Broadcasts a message to the network
    ///
    /// Should provide that the message eventually reach all non-faulty nodes
    fn broadcast_message(&self, message: M) -> BoxFuture<'_, Result<(), NetworkError>>;

    /// Sends a direct message to a specific node
    fn message_node(
        &self,
        message: M,
        recipient: PubKey,
    ) -> BoxFuture<'_, Result<(), NetworkError>>;

    /// Moves out the entire queue of received broadcast messages, should there be any
    ///
    /// Provided as a future to allow the backend to do async locking
    fn broadcast_queue(&self) -> BoxFuture<'_, Result<Vec<M>, NetworkError>>;

    /// Provides a future for the next received broadcast
    ///
    /// Will unwrap the underlying `NetworkMessage`
    fn next_broadcast(&self) -> BoxFuture<'_, Result<M, NetworkError>>;

    /// Moves out the entire queue of received direct messages to this node
    fn direct_queue(&self) -> BoxFuture<'_, Result<Vec<M>, NetworkError>>;

    /// Provides a future for the next received direct message to this node
    ///
    /// Will unwrap the underlying `NetworkMessage`
    fn next_direct(&self) -> BoxFuture<'_, Result<M, NetworkError>>;

    /// Node's currently known to the networking implementation
    ///
    /// Kludge function to work around leader election
    fn known_nodes(&self) -> BoxFuture<'_, Vec<PubKey>>;

    /// Object safe clone
    fn obj_clone(&self) -> Box<dyn NetworkingImplementation<M> + 'static>;

    /// Returns a list of changes in the network that have been observed. Calling this function will clear the internal list.
    fn network_changes(&self) -> BoxFuture<'_, Result<Vec<NetworkChange>, NetworkError>>;

    /// Shut down this network. Afterwards this network should no longer be used.
    ///
    /// This should also cause other functions to immediately return with a [`NetworkError`]
    fn shut_down(&self) -> BoxFuture<'_, ()>;
}

/// Changes that can occur in the network
#[derive(Debug)]
pub enum NetworkChange {
    /// A node is connected
    NodeConnected(PubKey),

    /// A node is disconnected
    NodeDisconnected(PubKey),
}

/// interface describing how reliable the network is
pub trait NetworkReliability: std::fmt::Debug + Sync + std::marker::Send {
    /// Sample from bernoulli distribution to decide whether
    /// or not to keep a packet
    /// # Panics
    ///
    /// Panics if `self.keep_numerator > self.keep_denominator`
    ///
    fn sample_keep(&self) -> bool;
    /// sample from uniform distribution to decide whether
    /// or not to keep a packet
    fn sample_delay(&self) -> Duration;
}
