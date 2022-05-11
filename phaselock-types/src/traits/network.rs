//! Network access abstraction
//!
//! Contains types and traits used by `PhaseLock` to abstract over network access

use async_trait::async_trait;
use async_tungstenite::tungstenite::error as werror;
use serde::{de::DeserializeOwned, Serialize};
use snafu::Snafu;
use std::time::Duration;

use crate::PubKey;

use super::signature_key::SignatureKey;

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
#[async_trait]
pub trait NetworkingImplementation<M, K>: Send + Sync
where
    M: Serialize + DeserializeOwned + Send + Clone + 'static,
    K: SignatureKey + 'static,
{
    /// Broadcasts a message to the network
    ///
    /// Should provide that the message eventually reach all non-faulty nodes
    async fn broadcast_message(&self, message: M) -> Result<(), NetworkError>;

    /// Sends a direct message to a specific node
    async fn message_node(&self, message: M, recipient: PubKey<K>) -> Result<(), NetworkError>;

    /// Moves out the entire queue of received broadcast messages, should there be any
    ///
    /// Provided as a future to allow the backend to do async locking
    async fn broadcast_queue(&self) -> Result<Vec<M>, NetworkError>;

    /// Provides a future for the next received broadcast
    ///
    /// Will unwrap the underlying `NetworkMessage`
    async fn next_broadcast(&self) -> Result<M, NetworkError>;

    /// Moves out the entire queue of received direct messages to this node
    async fn direct_queue(&self) -> Result<Vec<M>, NetworkError>;

    /// Provides a future for the next received direct message to this node
    ///
    /// Will unwrap the underlying `NetworkMessage`
    async fn next_direct(&self) -> Result<M, NetworkError>;

    /// Node's currently known to the networking implementation
    ///
    /// Kludge function to work around leader election
    async fn known_nodes(&self) -> Vec<PubKey<K>>;

    /// Object safe clone
    fn obj_clone(&self) -> Box<dyn NetworkingImplementation<M, K> + 'static>;

    /// Returns a list of changes in the network that have been observed. Calling this function will clear the internal list.
    async fn network_changes(&self) -> Result<Vec<NetworkChange<K>>, NetworkError>;

    /// Shut down this network. Afterwards this network should no longer be used.
    ///
    /// This should also cause other functions to immediately return with a [`NetworkError`]
    async fn shut_down(&self) -> ();
}

/// Changes that can occur in the network
#[derive(Debug)]
pub enum NetworkChange<K: SignatureKey> {
    /// A node is connected
    NodeConnected(PubKey<K>),

    /// A node is disconnected
    NodeDisconnected(PubKey<K>),
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
