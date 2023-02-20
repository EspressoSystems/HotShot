//! A network implementation that connects to a centralized web server.
//!
//! To run the web server, see the `./centralized_web_server/` folder in this repo.
//!

// TODO ED Remove once ready to merge
#![allow(dead_code, unused, deprecated)]

#[cfg(feature = "async-std-executor")]
use async_std::net::TcpStream;
use nll::nll_todo::nll_todo;
#[cfg(feature = "tokio-executor")]
use tokio::net::TcpStream;
#[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
std::compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}

use async_compatibility_layer::{
    art::{async_block_on, async_sleep, async_spawn, split_stream},
    channel::{oneshot, unbounded, OneShotSender, UnboundedReceiver, UnboundedSender},
};
use async_lock::{RwLock, RwLockUpgradableReadGuard};
use async_trait::async_trait;
use bincode::Options;
use futures::{future::BoxFuture, FutureExt};
use hotshot_types::{
    data::ProposalType,
    message::{Message, VoteType},
    traits::{
        election::{Election, ElectionConfig},
        metrics::{Metrics, NoMetrics},
        network::{
            CentralizedServerNetworkError, CommunicationChannel, ConnectedNetwork,
            FailedToDeserializeSnafu, FailedToSerializeSnafu, NetworkError, NetworkMsg,
            TestableNetworkingImplementation, TransmitType,
        },
        node_implementation::NodeType,
        signature_key::{ed25519::Ed25519Pub, SignatureKey, TestableSignatureKey},
    },
};
use hotshot_utils::bincode::bincode_opts;
use serde::{de::DeserializeOwned, Serialize};
use snafu::ResultExt;
use std::{
    cmp,
    collections::{hash_map::Entry, BTreeSet, HashMap},
    marker::PhantomData,
    net::{Ipv4Addr, SocketAddr},
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tracing::{error, instrument};

use super::NetworkingMetrics;

#[derive(Clone, Debug)]
pub struct CentralizedWebServerNetwork<TYPES: NodeTypes> {
    /// The inner state
    inner: Arc<Inner<TYPES>>,
    /// An optional shutdown signal. This is only used when this connection is created through the `TestableNetworkingImplementation` API.
    server_shutdown_signal: Option<Arc<OneShotSender<()>>>,
}

#[async_trait]
impl<
        TYPES: NodeType,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        ELECTION: Election<TYPES>,
    > CommunicationChannel<TYPES, PROPOSAL, VOTE, ELECTION>
    for CentralizedWebCommChannel<TYPES, PROPOSAL, VOTE, ELECTION>
{
    /// Blocks until node is successfully initialized
    /// into the network
    async fn wait_for_ready(&self) {
        nll_todo();
    }

    /// checks if the network is ready
    /// nonblocking
    async fn is_ready(&self) -> bool {
        nll_todo();
    }

    /// Shut down this network. Afterwards this network should no longer be used.
    ///
    /// This should also cause other functions to immediately return with a [`NetworkError`]
    async fn shut_down(&self) -> () {
        nll_todo();
    }

    /// broadcast message to those listening on the communication channel
    /// blocking
    async fn broadcast_message(
        &self,
        message: Message<TYPES, PROPOSAL, VOTE>,
        election: &ELECTION,
    ) -> Result<(), NetworkError> {
        nll_todo();
    }

    /// Sends a direct message to a specific node
    /// blocking
    async fn direct_message(
        &self,
        message: Message<TYPES, PROPOSAL, VOTE>,
        recipient: TYPES::SignatureKey,
    ) -> Result<(), NetworkError> {
        nll_todo();
    }

    /// Moves out the entire queue of received messages of 'transmit_type`
    ///
    /// Will unwrap the underlying `NetworkMessage`
    /// blocking
    async fn recv_msgs(
        &self,
        transmit_type: TransmitType,
    ) -> Result<Vec<Message<TYPES, PROPOSAL, VOTE>>, NetworkError> {
        nll_todo();
    }

    /// look up a node
    /// blocking
    async fn lookup_node(&self, pk: TYPES::SignatureKey) -> Result<(), NetworkError> {
        nll_todo();
    }
}

#[async_trait]
impl<M: NetworkMsg, K: SignatureKey + 'static, E: ElectionConfig + 'static> ConnectedNetwork<M, K>
    for CentralizedWebServerNetwork<K, E>
{
    /// Blocks until the network is successfully initialized
    async fn wait_for_ready(&self) {
        nll_todo();
    }

    /// checks if the network is ready
    /// nonblocking
    async fn is_ready(&self) -> bool {
        nll_todo();
    }

    /// Blocks until the network is shut down
    /// then returns true
    async fn shut_down(&self) {
        nll_todo();
    }

    /// broadcast message to some subset of nodes
    /// blocking
    async fn broadcast_message(
        &self,
        message: M,
        recipients: BTreeSet<K>,
    ) -> Result<(), NetworkError> {
        nll_todo();
    }

    /// Sends a direct message to a specific node
    /// blocking
    async fn direct_message(&self, message: M, recipient: K) -> Result<(), NetworkError> {
        nll_todo();
    }

    /// Moves out the entire queue of received messages of 'transmit_type`
    ///
    /// Will unwrap the underlying `NetworkMessage`
    /// blocking
    async fn recv_msgs(&self, transmit_type: TransmitType) -> Result<Vec<M>, NetworkError> {
        nll_todo();
    }

    /// look up a node
    /// blocking
    async fn lookup_node(&self, pk: K) -> Result<(), NetworkError> {
        nll_todo();
    }
}
