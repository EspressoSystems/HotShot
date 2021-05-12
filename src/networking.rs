use crate::PubKey;

use async_tungstenite::tungstenite::error as werror;
use futures_lite::future::Boxed as BoxedFuture;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use snafu::Snafu;

mod memory_network;
mod w_network;

/// Error type for networking
#[derive(Debug, Snafu)]
#[snafu(visibility = "pub(crate)")]
pub enum NetworkError {
    #[snafu(display("A listener attempted to send a message"))]
    ListenerSend,
    CouldNotDeliver,
    NoSuchNode,
    FailedToSerialize {
        source: bincode::Error,
    },
    FailedToDeserialize {
        sourec: bincode::Error,
    },
    WError {
        source: werror::Error,
    },
    ExecutorError {
        source: async_std::io::Error,
    },
    SocketDecodeError {
        input: String,
        source: std::io::Error,
    },
    FailedToBindListener {
        source: std::io::Error,
    },
    NoSocketsError {
        input: String,
    },
    Other {
        inner: Box<dyn std::error::Error + Send>,
    },
}

#[derive(Clone, Serialize, Deserialize)]
pub enum NetworkMessage<M> {
    Broadcast {
        message: M,
        sender: PubKey,
    },
    Direct {
        message: M,
        sender: PubKey,
        tag: PubKey,
    },
}

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
