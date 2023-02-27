//! Network access compatibility
//!
//! Contains types and traits used by `HotShot` to abstract over network access

#[cfg(feature = "async-std-executor")]
use async_std::future::TimeoutError;
use libp2p_networking::network::NetworkNodeHandleError;
#[cfg(feature = "tokio-executor")]
use tokio::time::error::Elapsed as TimeoutError;
#[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
std::compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}

use super::{election::Membership, node_implementation::NodeType, signature_key::SignatureKey};
use crate::{data::ProposalType, message::Message, vote::VoteType};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use snafu::Snafu;
use std::{collections::BTreeSet, time::Duration};

impl From<NetworkNodeHandleError> for NetworkError {
    fn from(error: NetworkNodeHandleError) -> Self {
        match error {
            NetworkNodeHandleError::SerializationError { source } => {
                NetworkError::FailedToSerialize { source }
            }
            NetworkNodeHandleError::DeserializationError { source } => {
                NetworkError::FailedToDeserialize { source }
            }
            NetworkNodeHandleError::TimeoutError { source } => NetworkError::Timeout { source },
            NetworkNodeHandleError::Killed => NetworkError::ShutDown,
            source => NetworkError::Libp2p { source },
        }
    }
}

/// for any errors we decide to add to memory network
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum MemoryNetworkError {
    /// stub
    Stub,
}

/// Centralized server specific errors
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum CentralizedServerNetworkError {
    /// The centralized server could not find a specific message.
    NoMessagesInQueue,
}

/// Centralized web server specific errors
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum CentralizedWebServerNetworkError {
    /// The injected consensus data is incorrect
    IncorrectConsensusData,
    /// The client returned an error
    ClientError,
}

/// the type of transmission
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TransmitType {
    /// directly transmit
    Direct,
    /// broadcast the message to all
    Broadcast,
}

/// Error type for networking
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum NetworkError {
    /// Libp2p specific errors
    Libp2p {
        /// source of error
        source: NetworkNodeHandleError,
    },
    /// memory network specific errors
    MemoryNetwork {
        /// source of error
        source: MemoryNetworkError,
    },
    /// Centralized server specific errors
    CentralizedServer {
        /// source of error
        source: CentralizedServerNetworkError,
    },

    /// Centralized web server specific errors
    CentralizedWebServer {
        /// source of error
        source: CentralizedWebServerNetworkError,
    },
    /// unimplemented functionality
    UnimplementedFeature,
    /// Could not deliver a message to a specified recipient
    CouldNotDeliver,
    /// Attempted to deliver a message to an unknown node
    NoSuchNode,
    /// Failed to serialize a network message
    FailedToSerialize {
        /// Originating bincode error
        source: bincode::Error,
    },
    /// Failed to deserealize a network message
    FailedToDeserialize {
        /// originating bincode error
        source: bincode::Error,
    },
    /// A timeout occurred
    Timeout {
        /// Source of error
        source: TimeoutError,
    },
    /// Error sending output to consumer of NetworkingImplementation
    /// TODO this should have more information
    ChannelSend,
    /// The underlying connection has been shut down
    ShutDown,
    /// unable to cancel a request, the request has already been cancelled
    UnableToCancel,
}

/// API for interacting directly with a consensus committee
/// intended to be implemented for both DA and for validating consensus committees
#[async_trait]
pub trait CommunicationChannel<
    TYPES: NodeType,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
    MEMBERSHIP: Membership<TYPES>,
>: Clone + Send + Sync + 'static
{
    /// Blocks until node is successfully initialized
    /// into the network
    async fn wait_for_ready(&self);

    /// checks if the network is ready
    /// nonblocking
    async fn is_ready(&self) -> bool;

    /// Shut down this network. Afterwards this network should no longer be used.
    ///
    /// This should also cause other functions to immediately return with a [`NetworkError`]
    async fn shut_down(&self) -> ();

    /// broadcast message to those listening on the communication channel
    /// blocking
    async fn broadcast_message(
        &self,
        message: Message<TYPES, PROPOSAL, VOTE>,
        election: &MEMBERSHIP,
    ) -> Result<(), NetworkError>;

    /// Sends a direct message to a specific node
    /// blocking
    async fn direct_message(
        &self,
        message: Message<TYPES, PROPOSAL, VOTE>,
        recipient: TYPES::SignatureKey,
    ) -> Result<(), NetworkError>;

    /// Moves out the entire queue of received messages of 'transmit_type`
    ///
    /// Will unwrap the underlying `NetworkMessage`
    /// blocking
    async fn recv_msgs(
        &self,
        transmit_type: TransmitType,
    ) -> Result<Vec<Message<TYPES, PROPOSAL, VOTE>>, NetworkError>;

    /// look up a node
    /// blocking
    async fn lookup_node(&self, pk: TYPES::SignatureKey) -> Result<(), NetworkError>;

    /// Injects consensus data such as view number into the networking implementation
    /// blocking
    async fn inject_consensus_info(&self, tuple: (u64, bool, bool)) -> Result<(), NetworkError>;
}

/// common traits we would like our network messages to implement
pub trait NetworkMsg:
    Serialize + for<'a> Deserialize<'a> + Clone + std::fmt::Debug + Sync + Send + 'static
{
}

/// represents a networking implmentration
/// exposes low level API for interacting with a network
/// intended to be implemented for libp2p, the centralized server,
/// and memory network
#[async_trait]
pub trait ConnectedNetwork<RECVMSG: NetworkMsg, SENDMSG: NetworkMsg, K: SignatureKey + 'static>:
    Clone + Send + Sync + 'static
{
    /// Blocks until the network is successfully initialized
    async fn wait_for_ready(&self);

    /// checks if the network is ready
    /// nonblocking
    async fn is_ready(&self) -> bool;

    /// Blocks until the network is shut down
    /// then returns true
    async fn shut_down(&self);

    /// broadcast message to some subset of nodes
    /// blocking
    async fn broadcast_message(
        &self,
        message: SENDMSG,
        recipients: BTreeSet<K>,
    ) -> Result<(), NetworkError>;

    /// Sends a direct message to a specific node
    /// blocking
    async fn direct_message(&self, message: SENDMSG, recipient: K) -> Result<(), NetworkError>;

    /// Moves out the entire queue of received messages of 'transmit_type`
    ///
    /// Will unwrap the underlying `NetworkMessage`
    /// blocking
    async fn recv_msgs(&self, transmit_type: TransmitType) -> Result<Vec<RECVMSG>, NetworkError>;

    /// look up a node
    /// blocking
    async fn lookup_node(&self, pk: K) -> Result<(), NetworkError>;

    /// Injects consensus data such as view number into the networking implementation
    /// blocking
    /// Ideally we would pass in the `Time` type, but that requires making the entire trait generic over NodeType
    async fn inject_consensus_info(&self, tuple: (u64, bool, bool)) -> Result<(), NetworkError>;
}

/// Describes additional functionality needed by the test network implementation
pub trait TestableNetworkingImplementation<
    TYPES: NodeType,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
    MEMBERSHIP: Membership<TYPES>,
>: CommunicationChannel<TYPES, PROPOSAL, VOTE, MEMBERSHIP>
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
