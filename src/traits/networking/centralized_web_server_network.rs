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

use async_compatibility_layer::async_primitives::subscribable_rwlock::SubscribableRwLock;
use async_compatibility_layer::{
    art::{async_block_on, async_sleep, async_spawn, split_stream},
    channel::{oneshot, unbounded, OneShotSender, UnboundedReceiver, UnboundedSender},
};

// TODO ED Do we really need this?
use hotshot_centralized_web_server::{self, config};
use hotshot_types::traits::state::ConsensusTime;

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
use serde::{de::DeserializeOwned, Deserialize, Serialize};
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
use surf_disco::error::ClientError;
use tracing::{error, instrument};

use super::NetworkingMetrics;

#[derive(Clone)]
pub struct CentralizedWebCommChannel<
    TYPES: NodeType,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
    ELECTION: Election<TYPES>,
>(
    CentralizedWebServerNetwork<
        TYPES::SignatureKey,
        TYPES::ElectionConfigType,
        TYPES,
        PROPOSAL,
        VOTE,
    >,
    PhantomData<(PROPOSAL, VOTE, ELECTION)>,
);
impl<
        TYPES: NodeType,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        ELECTION: Election<TYPES>,
    > CentralizedWebCommChannel<TYPES, PROPOSAL, VOTE, ELECTION>
{
    /// Create new communication channel
    pub fn new(
        network: CentralizedWebServerNetwork<
            TYPES::SignatureKey,
            TYPES::ElectionConfigType,
            TYPES,
            PROPOSAL,
            VOTE,
        >,
    ) -> Self {
        Self(network, PhantomData::default())
    }

    fn parse_message(
        &self,
        message: Message<TYPES, PROPOSAL, VOTE>,
    ) -> WebServerNetworkMessage<TYPES, PROPOSAL, VOTE> {
        // Plan:
        // create NetworkMsg using match for endpoint
        // Best way to map endpoints?  Enum?  Import config from web server
        let view_number: TYPES::Time = message.get_view_number().into();

        // Returns the endpoint we need, maybe should return an option?  For internal trigger? Return error for now?
        let endpoint = match message.clone().kind {
            hotshot_types::message::MessageKind::Consensus(message_kind) => match message_kind {
                hotshot_types::message::ConsensusMessage::Proposal(_) => {
                    config::post_proposal_route((*view_number).into())
                }
                hotshot_types::message::ConsensusMessage::Vote(_) => {
                    // We shouldn't ever reach this TODO ED
                    config::post_vote_route((*view_number).into())
                }
                hotshot_types::message::ConsensusMessage::InternalTrigger(_) => {
                    // TODO ED Remove this once we are sure this is never hit
                    panic!();
                    // return Err(NetworkError::UnimplementedFeature)
                    "InternalTrigger".to_string()
                }
            },
            hotshot_types::message::MessageKind::Data(message_kind) => match message_kind {
                hotshot_types::message::DataMessage::SubmitTransaction(_, _) => {
                    config::post_transactions_route()
                }
            },
        };

        let network_msg: WebServerNetworkMessage<TYPES, PROPOSAL, VOTE> =
            WebServerNetworkMessage { message, endpoint };
        network_msg

        // TODO ED Current web server doesn't have a concept of recipients
    }
}

#[derive(Clone, Debug)]
pub struct CentralizedWebServerNetwork<
    KEY: SignatureKey,
    ELECTIONCONFIG: ElectionConfig,
    TYPES: NodeType,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
> {
    /// The inner state
    // TODO ED What's the point of inner?
    inner: Arc<Inner<KEY, ELECTIONCONFIG, TYPES, PROPOSAL, VOTE>>,
    /// An optional shutdown signal. This is only used when this connection is created through the `TestableNetworkingImplementation` API.
    server_shutdown_signal: Option<Arc<OneShotSender<()>>>,
}

impl<
        KEY: SignatureKey,
        ELECTIONCONFIG: ElectionConfig,
        TYPES: NodeType,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
    > CentralizedWebServerNetwork<KEY, ELECTIONCONFIG, TYPES, PROPOSAL, VOTE>
{
    async fn send_message_to_web_server<
        M: NetworkMsg + WebServerNetworkMessageTrait<TYPES, PROPOSAL, VOTE>,
    >(
        &self,
        message: M,
    ) -> Result<(), NetworkError> {
        let result: Result<(), ClientError> = self
            .inner
            .client
            // TODO ED update this to get actual message
            .post(&message.get_endpoint())
            .body_binary(&message.get_message())
            .unwrap()
            .send()
            .await;
        // TODO ED Actually return result
        Ok(())
    }
}

#[derive(Debug, Default, Clone)]
pub struct ConsensusInfo {
    view_number: u64,
    is_current_leader: bool,
    is_next_leader: bool,
}

#[derive(Debug)]
struct Inner<
    KEY: SignatureKey,
    ELECTIONCONFIG: ElectionConfig,
    TYPES: NodeType,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
> {
    // TODO ED Get rid of phantom if can
    phantom: PhantomData<(KEY, ELECTIONCONFIG)>,
    // Current view number so we can poll accordingly
    // TODO ED Should we keep these as three objects or one?
    // view_number: Arc<SubscribableRwLock<<TYPES as NodeType>::Time>>,
    // is_current_leader: Arc<SubscribableRwLock<bool>>,
    // is_next_leader: Arc<SubscribableRwLock<bool>>,
    consensus_info: Arc<SubscribableRwLock<ConsensusInfo>>,

    // TODO Do we ever use this?
    own_key: TYPES::SignatureKey,
    // // Queue for broadcasted messages (mainly transactions and proposals)
    broadcast_poll_queue: Arc<RwLock<Vec<Message<TYPES, PROPOSAL, VOTE>>>>,
    // // Queue for direct messages (mainly votes)
    // Should this be channels? TODO ED
    direct_poll_queue: Arc<RwLock<Vec<Message<TYPES, PROPOSAL, VOTE>>>>,
    // TODO ED the same as connected?
    running: AtomicBool,
    // The network is connected to the web server and ready to go
    connected: AtomicBool,
    client: surf_disco::Client<ClientError>,
    wait_between_polls: Duration,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]
pub struct WebServerNetworkMessage<
    TYPES: NodeType,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
> {
    message: Message<TYPES, PROPOSAL, VOTE>,
    endpoint: String,
}

// Ideally you'd want it to be generic over any network msg, but for now this is fine
pub trait WebServerNetworkMessageTrait<
    TYPES: NodeType,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
>
{
    fn get_endpoint(&self) -> String;
    fn get_message(&self) -> Message<TYPES, PROPOSAL, VOTE>;
}

impl<TYPES: NodeType, PROPOSAL: ProposalType<NodeType = TYPES>, VOTE: VoteType<TYPES>>
    WebServerNetworkMessageTrait<TYPES, PROPOSAL, VOTE>
    for WebServerNetworkMessage<TYPES, PROPOSAL, VOTE>
{
    // TODO ED String doesn't impl copy?
    fn get_endpoint(&self) -> String {
        self.endpoint.clone()
    }

    fn get_message(&self) -> Message<TYPES, PROPOSAL, VOTE> {
        self.message.clone()
    }
}

impl<TYPES: NodeType, PROPOSAL: ProposalType<NodeType = TYPES>, VOTE: VoteType<TYPES>> NetworkMsg
    for WebServerNetworkMessage<TYPES, PROPOSAL, VOTE>
{
}

impl<
        K: SignatureKey + 'static,
        E: ElectionConfig + 'static,
        TYPES: NodeType + 'static,
        PROPOSAL: ProposalType<NodeType = TYPES> + 'static,
        VOTE: VoteType<TYPES> + 'static,
    > CentralizedWebServerNetwork<K, E, TYPES, PROPOSAL, VOTE>
{
    // TODO ED change to new
    pub fn create(
        host: String,
        port: u16,
        wait_between_polls: Duration,
        key: TYPES::SignatureKey,
    ) -> Self {
        // TODO ED Clean this up
        let base_url = format!("{host}:{port}");
        println!("{:?}", base_url);

        let base_url = format!("http://{base_url}").parse().unwrap();
        let client = surf_disco::Client::<ClientError>::new(base_url);

        let inner = Arc::new(Inner {
            phantom: PhantomData::default(),
            // Assuming this is initialized to zero
            // view_number: Arc::new(SubscribableRwLock::new(TYPES::Time::new(0))),
            // is_current_leader: Arc::new(SubscribableRwLock::new(false)),
            // is_next_leader: Arc::new(SubscribableRwLock::new(false)),
            consensus_info: Arc::new(SubscribableRwLock::new(ConsensusInfo::default())),
            broadcast_poll_queue: Default::default(),
            direct_poll_queue: Default::default(),
            running: AtomicBool::new(true),
            connected: AtomicBool::new(false),
            client,
            wait_between_polls,
            own_key: key,
        });
        inner.connected.store(true, Ordering::Relaxed);

        // async_spawn({
        //     let inner = Arc::clone(&inner);
        //     async move {
        //         while inner.running.load(Ordering::Relaxed) {
        //             if let Err(e) = CentralizedWebServerNetwork::<K, E, TYPES, PROPOSAL, VOTE>::run_background_receive(Arc::clone(&inner)).await {
        //                 error!(?e, "background thread exited");
        //             }
        //             inner.connected.store(false, Ordering::Relaxed);
        //         }
        //     }
        // });
        Self {
            inner,
            server_shutdown_signal: None,
        }
    }

    // TODO ED Move to inner impl?
    async fn run_background_receive(
        inner: Arc<Inner<K, E, TYPES, PROPOSAL, VOTE>>,
    ) -> Result<(), ClientError> {
        // TODO ED Change running variable if this function closes
        Ok(())
    }
}

async fn poll_generic_endpoint(client: surf_disco::Client<ClientError>) {}

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
        <CentralizedWebServerNetwork<_, _, _, _, _> as ConnectedNetwork<
            WebServerNetworkMessage<TYPES, PROPOSAL, VOTE>,
            TYPES::SignatureKey,
        >>::wait_for_ready(&self.0)
        .await;
    }

    /// checks if the network is ready
    /// nonblocking
    async fn is_ready(&self) -> bool {
        <CentralizedWebServerNetwork<_, _, _, _, _> as ConnectedNetwork<
            WebServerNetworkMessage<TYPES, PROPOSAL, VOTE>,
            TYPES::SignatureKey,
        >>::is_ready(&self.0)
        .await
    }

    /// Shut down this network. Afterwards this network should no longer be used.
    ///
    /// This should also cause other functions to immediately return with a [`NetworkError`]
    async fn shut_down(&self) -> () {
        <CentralizedWebServerNetwork<_, _, _, _, _> as ConnectedNetwork<
            WebServerNetworkMessage<TYPES, PROPOSAL, VOTE>,
            TYPES::SignatureKey,
        >>::shut_down(&self.0)
        .await;
    }

    /// broadcast message to those listening on the communication channel
    /// blocking
    async fn broadcast_message(
        &self,
        message: Message<TYPES, PROPOSAL, VOTE>,
        election: &ELECTION,
    ) -> Result<(), NetworkError> {
        let network_msg = self.parse_message(message);
        self.0.broadcast_message(network_msg, BTreeSet::new()).await
    }

    /// Sends a direct message to a specific node
    /// blocking
    async fn direct_message(
        &self,
        message: Message<TYPES, PROPOSAL, VOTE>,
        recipient: TYPES::SignatureKey,
    ) -> Result<(), NetworkError> {
        let network_msg = self.parse_message(message);
        self.0.direct_message(network_msg, recipient).await
    }

    /// Moves out the entire queue of received messages of 'transmit_type`
    ///
    /// Will unwrap the underlying `NetworkMessage`
    /// blocking
    async fn recv_msgs(
        &self,
        transmit_type: TransmitType,
    ) -> Result<Vec<Message<TYPES, PROPOSAL, VOTE>>, NetworkError> {
        Ok(Vec::new())
    }

    /// look up a node
    /// blocking
    async fn lookup_node(&self, pk: TYPES::SignatureKey) -> Result<(), NetworkError> {
        Ok(())
    }

    async fn inject_consensus_info(&self, tuple: (u64, bool, bool)) -> Result<(), NetworkError> {
        <CentralizedWebServerNetwork<_, _, _, _, _> as ConnectedNetwork<
            WebServerNetworkMessage<TYPES, PROPOSAL, VOTE>,
            TYPES::SignatureKey,
        >>::inject_consensus_info(&self.0, tuple)
        .await
    }
}

#[async_trait]
impl<
        M: NetworkMsg,
        K: SignatureKey + 'static,
        E: ElectionConfig + 'static,
        TYPES: NodeType + 'static,
        PROPOSAL: ProposalType<NodeType = TYPES> + 'static,
        VOTE: VoteType<TYPES> + 'static,
    > ConnectedNetwork<M, K> for CentralizedWebServerNetwork<K, E, TYPES, PROPOSAL, VOTE>
// Make this a trait?
where
    M: WebServerNetworkMessageTrait<TYPES, PROPOSAL, VOTE> + NetworkMsg,
{
    /// Blocks until the network is successfully initialized
    async fn wait_for_ready(&self) {
        // TODO ED Also add check that we're running?
        while !self.inner.connected.load(Ordering::Relaxed) {
            async_sleep(Duration::from_secs(1)).await;
        }
    }

    /// checks if the network is ready
    /// nonblocking
    async fn is_ready(&self) -> bool {
        nll_todo()
    }

    /// Blocks until the network is shut down
    /// then returns true
    async fn shut_down(&self) {
        self.inner.running.store(false, Ordering::Relaxed);
    }

    /// broadcast message to some subset of nodes
    /// blocking
    async fn broadcast_message(
        &self,
        message: M,
        recipients: BTreeSet<K>,
    ) -> Result<(), NetworkError> {
        let result = self.send_message_to_web_server(message).await;

        // TODO ED Match result

        Ok(())
    }

    /// Sends a direct message to a specific node
    /// blocking
    async fn direct_message(&self, message: M, recipient: K) -> Result<(), NetworkError> {
        let result = self.send_message_to_web_server(message).await;

        // TODO ED Match result

        Ok(())
    }

    /// Moves out the entire queue of received messages of 'transmit_type`
    ///
    /// Will unwrap the underlying `NetworkMessage`
    /// blocking
    async fn recv_msgs(&self, transmit_type: TransmitType) -> Result<Vec<M>, NetworkError> {
        // TODO ED Implement
        Ok((Vec::new()))
    }

    /// look up a node
    /// blocking
    async fn lookup_node(&self, pk: K) -> Result<(), NetworkError> {
        Ok(())
    }

    async fn inject_consensus_info(&self, tuple: (u64, bool, bool)) -> Result<(), NetworkError> {
        let (view_number, is_current_leader, is_next_leader) = tuple;

        let new_consensus_info = ConsensusInfo {
            view_number,
            is_current_leader,
            is_next_leader,
        };
        self.inner.consensus_info.modify(|old_consensus_info| {
            // This should never happen
            if new_consensus_info.view_number < old_consensus_info.view_number {
                panic!(); 
            }
            *old_consensus_info = new_consensus_info;
        }).await;

        Ok(())
    }
}
impl<
        TYPES: NodeType,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        ELECTION: Election<TYPES>,
    > TestableNetworkingImplementation<TYPES, PROPOSAL, VOTE, ELECTION>
    for CentralizedWebCommChannel<TYPES, PROPOSAL, VOTE, ELECTION>
where
    TYPES::SignatureKey: TestableSignatureKey,
{
    fn generator(
        expected_node_count: usize,
        _num_bootstrap: usize,
    ) -> Box<dyn Fn(u64) -> Self + 'static> {
        let (server_shutdown_sender, server_shutdown) = oneshot();
        let sender = Arc::new(server_shutdown_sender);
        // Start web server
        // TODO may have a race condition if this doesn't start fully before below:
        async_spawn(hotshot_centralized_web_server::run_web_server(Some(
            server_shutdown,
        )));

        let known_nodes = (0..expected_node_count as u64)
            .map(|id| {
                TYPES::SignatureKey::from_private(&TYPES::SignatureKey::generate_test_key(id))
            })
            .collect::<Vec<_>>();

        // Start each node's web server client
        Box::new(move |id| {
            let sender = Arc::clone(&sender);
            let mut network = CentralizedWebServerNetwork::create(
                "0.0.0.0".to_string(),
                9000,
                Duration::from_millis(100),
                known_nodes[id as usize].clone(),
            );
            network.server_shutdown_signal = Some(sender);
            CentralizedWebCommChannel::new(network)
        })
    }

    fn in_flight_message_count(&self) -> Option<usize> {
        nll_todo()
    }
}
