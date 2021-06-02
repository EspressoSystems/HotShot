use crate::PubKey;

use async_tungstenite::tungstenite::error as werror;
use futures_lite::future::Boxed as BoxedFuture;
use serde::{de::DeserializeOwned, Serialize};
use snafu::Snafu;

/// In memory network simulator
pub mod memory_network;
/// Websockets based networking implementation
pub mod w_network;

/// Error type for networking
#[derive(Debug, Snafu)]
#[snafu(visibility = "pub(crate)")]
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
    WError {
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
        inner: Box<dyn std::error::Error + Send>,
    },
}

/// Describes, generically, the behaviors a networking implementation must have
pub trait NetworkingImplementation<M>: Send + Sync
where
    M: Serialize + DeserializeOwned + Send + Clone + 'static,
{
    /// Broadcasts a message to the network
    ///
    /// Should provide that the message eventually reach all non-faulty nodes
    fn broadcast_message(&self, message: M) -> BoxedFuture<Result<(), NetworkError>>;
    /// Sends a direct message to a specific node
    fn message_node(&self, message: M, recipient: PubKey) -> BoxedFuture<Result<(), NetworkError>>;
    /// Moves out the entire queue of received broadcast messages, should there be any
    ///
    /// Provided as a future to allow the backend to do async locking
    fn broadcast_queue(&self) -> BoxedFuture<Result<Vec<M>, NetworkError>>;
    /// Provides a future for the next received broadcast
    ///
    /// Will unwrap the underlying `NetworkMessage`
    fn next_broadcast(&self) -> BoxedFuture<Result<Option<M>, NetworkError>>;
    /// Moves out the entire queue of received direct messages to this node
    fn direct_queue(&self) -> BoxedFuture<Result<Vec<M>, NetworkError>>;
    /// Provides a future for the next received direct message to this node
    ///
    /// Will unwrap the underlying `NetworkMessage`
    fn next_direct(&self) -> BoxedFuture<Result<Option<M>, NetworkError>>;
    /// Node's currently known to the networking implementation
    ///
    /// Kludge function to work around leader election
    fn known_nodes(&self) -> BoxedFuture<Vec<PubKey>>;
    /// Object safe clone
    fn obj_clone(&self) -> Box<dyn NetworkingImplementation<M> + 'static>;
}
