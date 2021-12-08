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
use rand::distributions::Distribution;
use std::time::Duration;

pub mod memory_network;
pub mod w_network;


/// A boxed future trait object with a static lifetime
pub type BoxedFuture<T> = BoxFuture<'static, T>;

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
}

#[derive(Debug, Copy, Clone)]
pub struct ReliabilityConfig {
    /// probability that packet is kept = keep_numerator / keep_denominator
    keep_numerator: u32,
    keep_denominator: u32,
    /// packet delay is obtained by sampling from a uniform distribution
    /// between delay_low_ms and delay_high_ms inclusive
    delay_low_ms: u64,
    delay_high_ms: u64
}

impl Default for ReliabilityConfig {
    // disable all chance of failure
    fn default() -> Self {
        ReliabilityConfig {
            keep_numerator: 1,
            keep_denominator: 1,
            delay_low_ms: 0,
            delay_high_ms: 0
        }
    }
}

impl ReliabilityConfig {
    pub fn sample_keep(&self) -> bool {
        assert!(self.keep_numerator <= self.keep_denominator);
        // cannot fail to unwrap if assert passes
        rand::distributions::Bernoulli::from_ratio(self.keep_numerator, self.keep_denominator)
            .unwrap().sample(&mut rand::thread_rng())
    }
    pub fn sample_delay(&self) -> Duration {
        Duration::from_millis(rand::distributions::Uniform::new_inclusive(self.delay_low_ms, self.delay_high_ms)
                              .sample(&mut rand::thread_rng()))
    }
}
