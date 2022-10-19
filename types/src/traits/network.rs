//! Network access abstraction
//!
//! Contains types and traits used by `HotShot` to abstract over network access

cfg_if::cfg_if! {
    if #[cfg(feature = "async-std-executor")] {
        use async_std::future::TimeoutError;
    } else if #[cfg(feature = "tokio-executor")] {
        use tokio::time::error::Elapsed as TimeoutError;
    } else {
        std::compile_error!{"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}
    }
}
use super::node_implementation::NodeTypes;
use crate::{message::Message, traits::signature_key::SignatureKey};
use async_trait::async_trait;
use async_tungstenite::tungstenite::error as werror;
use serde::{Deserialize, Serialize};
use snafu::Snafu;
use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

/// Error type for networking
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum NetworkError {
    /// DHT error
    DHTError,
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
        source: std::io::Error,
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
    /// A timeout occurred
    Timeout {
        /// Source of error
        source: TimeoutError,
    },
    /// Channel error
    ChannelSend,
    /// Could not complete handshake
    IdentityHandshake,
    /// The underlying connection has been shut down
    ShutDown,
    /// An underlying channel has disconnected
    ChannelDisconnected {
        /// Source of error
        source: hotshot_utils::channel::RecvError,
    },
    /// An underlying unbounded channel has disconnected
    UnboundedChannelDisconnected {
        /// Source of error
        source: hotshot_utils::channel::UnboundedRecvError,
    },
    /// The centralized server could not find a specific message.
    NoMessagesInQueue,
}

/// Describes, generically, the behaviors a networking implementation must have
#[async_trait]
pub trait NetworkingImplementation<TYPES: NodeTypes>: Clone + Send + Sync + 'static {
    /// Returns true when node is successfully initialized
    /// into the network
    ///
    /// Blocks until node is ready
    async fn ready(&self) -> bool;

    /// Broadcasts a message to the network
    ///
    /// Should provide that the message eventually reach all non-faulty nodes
    async fn broadcast_message(&self, message: Message<TYPES>) -> Result<(), NetworkError>;

    /// Sends a direct message to a specific node
    async fn message_node(
        &self,
        message: Message<TYPES>,
        recipient: TYPES::SignatureKey,
    ) -> Result<(), NetworkError>;

    /// Moves out the entire queue of received broadcast messages, should there be any
    ///
    /// Provided as a future to allow the backend to do async locking
    async fn broadcast_queue(&self) -> Result<Vec<Message<TYPES>>, NetworkError>;

    /// Provides a future for the next received broadcast
    ///
    /// Will unwrap the underlying `NetworkMessage`
    async fn next_broadcast(&self) -> Result<Message<TYPES>, NetworkError>;

    /// Moves out the entire queue of received direct messages to this node
    async fn direct_queue(&self) -> Result<Vec<Message<TYPES>>, NetworkError>;

    /// Provides a future for the next received direct message to this node
    ///
    /// Will unwrap the underlying `NetworkMessage`
    async fn next_direct(&self) -> Result<Message<TYPES>, NetworkError>;

    /// Node's currently known to the networking implementation
    ///
    /// Kludge function to work around leader election
    async fn known_nodes(&self) -> Vec<TYPES::SignatureKey>;

    /// Returns a list of changes in the network that have been observed. Calling this function will clear the internal list.
    async fn network_changes(
        &self,
    ) -> Result<Vec<NetworkChange<TYPES::SignatureKey>>, NetworkError>;

    /// Shut down this network. Afterwards this network should no longer be used.
    ///
    /// This should also cause other functions to immediately return with a [`NetworkError`]
    async fn shut_down(&self) -> ();

    /// Insert `value` into the shared store under `key`.
    async fn put_record(
        &self,
        key: impl Serialize + Send + Sync + 'static,
        value: impl Serialize + Send + Sync + 'static,
    ) -> Result<(), NetworkError>;

    /// Get value stored in shared store under `key`
    async fn get_record<V: for<'a> Deserialize<'a>>(
        &self,
        key: impl Serialize + Send + Sync + 'static,
    ) -> Result<V, NetworkError>;

    /// notifies the network of the next leader
    /// so it can prepare. Does not block
    async fn notify_of_subsequent_leader(
        &self,
        pk: TYPES::SignatureKey,
        cancelled: Arc<AtomicBool>,
    );
}

/// Describes additional functionality needed by the test network implementation
pub trait TestableNetworkingImplementation<TYPES: NodeTypes>:
    NetworkingImplementation<TYPES>
{
    /// generates a network given an expected node count
    fn generator(
        expected_node_count: usize,
        num_bootstrap: usize,
    ) -> Box<dyn Fn(u64) -> Self + 'static>;

    /// Get the number of messages in-flight.
    ///
    /// Some implementations will not be able to tell how many messages there are in-flight. These implementations should return `None`.
    fn in_flight_message_count(&self) -> Option<usize>;
}

/// Changes that can occur in the network
#[derive(Debug)]
pub enum NetworkChange<P: SignatureKey> {
    /// A node is connected
    NodeConnected(P),

    /// A node is disconnected
    NodeDisconnected(P),
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
