//! A network implementation that connects to a centralized web server.
//!
//! To run the web server, see the `./centralized_web_server/` folder in this repo.
//!

#[cfg(feature = "async-std-executor")]
#[cfg(feature = "tokio-executor")]
#[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
std::compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}

use async_compatibility_layer::async_primitives::subscribable_rwlock::ReadView;
use async_compatibility_layer::async_primitives::subscribable_rwlock::SubscribableRwLock;
use async_compatibility_layer::{
    art::{async_sleep, async_spawn},
    channel::{oneshot, OneShotSender},
};

use hotshot_centralized_web_server::{self, config};

use async_lock::RwLock;
use async_trait::async_trait;
use hotshot_types::{
    data::ProposalType,
    message::Message,
    traits::{
        election::{ElectionConfig, Membership},
        network::{
            CentralizedWebServerNetworkError, CommunicationChannel, ConnectedNetwork, NetworkError,
            NetworkMsg, TestableNetworkingImplementation, TransmitType,
        },
        node_implementation::NodeType,
        signature_key::{SignatureKey, TestableSignatureKey},
    },
    vote::VoteType,
};
use serde::{Deserialize, Serialize};

use std::{
    collections::BTreeSet,
    marker::PhantomData,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use surf_disco::error::ClientError;
use tracing::{error, info};

/// Represents the communication channel abstraction for the web server
#[derive(Clone)]
pub struct CentralizedWebCommChannel<
    TYPES: NodeType,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
    MEMBERSHIP: Membership<TYPES>,
>(
    CentralizedWebServerNetwork<
        Message<TYPES, PROPOSAL, VOTE>,
        TYPES::SignatureKey,
        TYPES::ElectionConfigType,
        TYPES,
        PROPOSAL,
        VOTE,
    >,
    PhantomData<MEMBERSHIP>,
);
impl<
        TYPES: NodeType,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        MEMBERSHIP: Membership<TYPES>,
    > CentralizedWebCommChannel<TYPES, PROPOSAL, VOTE, MEMBERSHIP>
{
    /// Create new communication channel
    pub fn new(
        network: CentralizedWebServerNetwork<
            Message<TYPES, PROPOSAL, VOTE>,
            TYPES::SignatureKey,
            TYPES::ElectionConfigType,
            TYPES,
            PROPOSAL,
            VOTE,
        >,
    ) -> Self {
        Self(network, PhantomData::default())
    }

    /// Parses a message to find the appropriate endpoint
    /// Returns a `SendMsg` containing the endpoint
    fn parse_post_message(
        &self,
        message: Message<TYPES, PROPOSAL, VOTE>,
    ) -> Result<SendMsg<Message<TYPES, PROPOSAL, VOTE>>, CentralizedWebServerNetworkError> {
        let view_number: TYPES::Time = message.get_view_number();

        let endpoint = match message.clone().kind {
            hotshot_types::message::MessageKind::Consensus(message_kind) => match message_kind {
                hotshot_types::message::ConsensusMessage::Proposal(_) => {
                    config::post_proposal_route(*view_number)
                }
                hotshot_types::message::ConsensusMessage::Vote(_) => {
                    config::post_vote_route(*view_number)
                }
                hotshot_types::message::ConsensusMessage::InternalTrigger(_) => {
                    return Err(CentralizedWebServerNetworkError::EndpointError)
                }
            },
            hotshot_types::message::MessageKind::Data(message_kind) => match message_kind {
                hotshot_types::message::DataMessage::SubmitTransaction(_, _) => {
                    config::post_transactions_route()
                }
            },
        };

        let network_msg: SendMsg<Message<TYPES, PROPOSAL, VOTE>> = SendMsg {
            message: Some(message),
            endpoint,
        };
        Ok(network_msg)
    }
}

/// The centralized web server network state
#[derive(Clone, Debug)]
pub struct CentralizedWebServerNetwork<
    M: NetworkMsg,
    KEY: SignatureKey,
    ELECTIONCONFIG: ElectionConfig,
    TYPES: NodeType,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
> {
    /// The inner, core state of the centralized web server network
    inner: Arc<Inner<M, KEY, ELECTIONCONFIG, TYPES, PROPOSAL, VOTE>>,
    /// An optional shutdown signal. This is only used when this connection is created through the `TestableNetworkingImplementation` API.
    server_shutdown_signal: Option<Arc<OneShotSender<()>>>,
}

impl<
        M: NetworkMsg,
        KEY: SignatureKey,
        ELECTIONCONFIG: ElectionConfig,
        TYPES: NodeType,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
    > CentralizedWebServerNetwork<M, KEY, ELECTIONCONFIG, TYPES, PROPOSAL, VOTE>
{
    /// Post a message to the web server and return the result
    async fn post_message_to_web_server(&self, message: SendMsg<M>) -> Result<(), NetworkError> {
        let result: Result<(), ClientError> = self
            .inner
            .client
            .post(&message.get_endpoint())
            .body_binary(&message.get_message())
            .unwrap()
            .send()
            .await;
        result.map_err(|_e| NetworkError::CentralizedWebServer {
            source: CentralizedWebServerNetworkError::ClientError,
        })
    }
}

/// Consensus info that is injected from `HotShot`
#[derive(Debug, Default, Clone, Copy)]
pub struct ConsensusInfo {
    /// The latest view number
    view_number: u64,
    /// Whether this node is the leader of `view_number`
    _is_current_leader: bool,
    /// Whether this node is the leader of the next view
    is_next_leader: bool,
}

/// Represents the core of centralized web server networking
#[derive(Debug)]
struct Inner<
    M: NetworkMsg,
    KEY: SignatureKey,
    ELECTIONCONFIG: ElectionConfig,
    TYPES: NodeType,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
> {
    /// Phantom data for generic types
    phantom: PhantomData<(KEY, ELECTIONCONFIG, PROPOSAL, VOTE)>,
    /// Consensus data about the current view number, leader, and next leader
    consensus_info: Arc<SubscribableRwLock<ConsensusInfo>>,
    /// Our own key
    _own_key: TYPES::SignatureKey,
    /// Queue for broadcasted messages
    broadcast_poll_queue: Arc<RwLock<Vec<RecvMsg<M>>>>,
    /// Queue for direct messages
    direct_poll_queue: Arc<RwLock<Vec<RecvMsg<M>>>>,
    /// Client is running
    running: AtomicBool,
    /// The web server connection is ready
    connected: AtomicBool,
    /// The connectioni to the web server
    client: surf_disco::Client<ClientError>,
    /// The duration to wait between poll attempts
    wait_between_polls: Duration,
}

impl<
        M: NetworkMsg,
        KEY: SignatureKey,
        ELECTIONCONFIG: ElectionConfig,
        TYPES: NodeType,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
    > Inner<M, KEY, ELECTIONCONFIG, TYPES, PROPOSAL, VOTE>
{
    /// Polls the web server at a given endpoint while the client is running
    async fn poll_web_server(&self, message_type: MessageType, _num_views_ahead: u64) {
        // Subscribe to changes in consensus info
        let consensus_update = self.consensus_info.subscribe().await;
        let mut consensus_info = self.consensus_info.copied().await;
        let mut vote_index: u64 = 0;
        let mut tx_index: u64 = 0;

        while self.running.load(Ordering::Relaxed) {
            let endpoint = match message_type {
                MessageType::Proposal => config::get_proposal_route(consensus_info.view_number),
                MessageType::VoteTimedOut => {
                    config::get_vote_route(consensus_info.view_number, vote_index)
                }
                MessageType::Transaction => config::get_transactions_route(tx_index),
            };

            let possible_message =
                if message_type == MessageType::VoteTimedOut && !consensus_info.is_next_leader {
                    Ok(None)
                } else {
                    self.get_message_from_web_server(endpoint).await
                };

            match possible_message {
                Ok(Some(deserialized_messages)) => {
                    match message_type {
                        MessageType::Proposal => {
                            info!("Received proposal for view {}", consensus_info.view_number);
                            // Only pushing the first proposal since we will soon only be allowing 1 proposal per view
                            self.broadcast_poll_queue
                                .write()
                                .await
                                .push(deserialized_messages[0].clone());
                            // Wait for the view to change before polling for proposals again
                            consensus_info = consensus_update.recv().await.unwrap();
                        }
                        MessageType::VoteTimedOut => {
                            info!(
                                "Received {} votes for view {}",
                                deserialized_messages.len(),
                                consensus_info.view_number
                            );
                            let mut direct_poll_queue = self.direct_poll_queue.write().await;
                            for vote in &deserialized_messages {
                                vote_index += 1;
                                direct_poll_queue.push(vote.clone());
                            }
                        }
                        MessageType::Transaction => {
                            info!("Received new transaction");
                            let mut lock = self.broadcast_poll_queue.write().await;
                            for tx in &deserialized_messages {
                                tx_index += 1;
                                lock.push(tx.clone());
                            }
                        }
                    }
                }
                Ok(None) => {
                    async_sleep(self.wait_between_polls).await;
                }
                Err(_e) => {
                    async_sleep(self.wait_between_polls).await;
                }
            }
            // Check if there is updated consensus info
            let new_consensus_info = consensus_update.try_recv();
            if let Ok(info) = new_consensus_info {
                consensus_info = info;
                vote_index = 0;
            }
        }
    }

    /// Sends a GET request to the webserver for some specified endpoint
    /// Returns a vec of deserialized, received messages or an error
    async fn get_message_from_web_server(
        &self,
        endpoint: String,
    ) -> Result<Option<Vec<RecvMsg<M>>>, NetworkError> {
        let result: Result<Option<Vec<Vec<u8>>>, ClientError> =
            self.client.get(&endpoint).send().await;
        match result {
            Err(_error) => Err(NetworkError::CentralizedWebServer {
                source: CentralizedWebServerNetworkError::ClientError,
            }),
            Ok(Some(messages)) => {
                let mut deserialized_messages = Vec::new();
                for message in &messages {
                    let deserialized_message = bincode::deserialize(message);
                    if let Err(e) = deserialized_message {
                        return Err(NetworkError::FailedToDeserialize { source: e });
                    }
                    deserialized_messages.push(deserialized_message.unwrap());
                }
                Ok(Some(deserialized_messages))
            }
            Ok(None) => Ok(None),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]
/// A message being sent to the web server
pub struct SendMsg<M: NetworkMsg> {
    /// The optional message, or body, to send
    message: Option<M>,
    /// The endpoint to send the message to
    endpoint: String,
}

/// A message being received from the web server
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]
pub struct RecvMsg<M: NetworkMsg> {
    /// The optional message being received
    message: Option<M>,
}

/// Trait for messages being sent to the web server
pub trait SendMsgTrait<M: NetworkMsg> {
    /// Returns the endpoint to send the message to
    fn get_endpoint(&self) -> String;
    /// Returns the actual message being sent
    fn get_message(&self) -> Option<M>;
}

/// Trait for messages being received from the web server
pub trait RecvMsgTrait<M: NetworkMsg> {
    /// Returns the actual message being received
    fn get_message(&self) -> Option<M>;
}

impl<M: NetworkMsg> SendMsgTrait<M> for SendMsg<M> {
    fn get_endpoint(&self) -> String {
        self.endpoint.clone()
    }

    fn get_message(&self) -> Option<M> {
        self.message.clone()
    }
}

impl<M: NetworkMsg> RecvMsgTrait<M> for RecvMsg<M> {
    fn get_message(&self) -> Option<M> {
        self.message.clone()
    }
}

impl<M: NetworkMsg> NetworkMsg for SendMsg<M> {}
impl<M: NetworkMsg> NetworkMsg for RecvMsg<M> {}

impl<
        M: NetworkMsg + 'static,
        K: SignatureKey + 'static,
        E: ElectionConfig + 'static,
        TYPES: NodeType + 'static,
        PROPOSAL: ProposalType<NodeType = TYPES> + 'static,
        VOTE: VoteType<TYPES> + 'static,
    > CentralizedWebServerNetwork<M, K, E, TYPES, PROPOSAL, VOTE>
{
    #[allow(clippy::panic)]
    /// Creates a new instance of the `CentralizedWebServerNetwork`
    /// # Panics
    /// if the web server url is malformed
    pub fn create(
        host: &str,
        port: u16,
        wait_between_polls: Duration,
        key: TYPES::SignatureKey,
    ) -> Self {
        let base_url_string = format!("http://{host}:{port}");
        error!("Connecting to web server at {base_url_string:?}");

        let base_url = base_url_string.parse();
        if base_url.is_err() {
            error!("Web server url {:?} is malformed", base_url_string);
        }

        // TODO ED Wait for healthcheck
        let client = surf_disco::Client::<ClientError>::new(base_url.unwrap());

        let inner = Arc::new(Inner {
            phantom: PhantomData,
            consensus_info: Arc::default(),
            broadcast_poll_queue: Arc::default(),
            direct_poll_queue: Arc::default(),
            running: AtomicBool::new(true),
            connected: AtomicBool::new(false),
            client,
            wait_between_polls,
            _own_key: key,
        });
        inner.connected.store(true, Ordering::Relaxed);

        async_spawn({
            let inner = Arc::clone(&inner);
            async move {
                while inner.running.load(Ordering::Relaxed) {
                    if let Err(e) = CentralizedWebServerNetwork::<M, K, E, TYPES, PROPOSAL, VOTE>::run_background_receive(Arc::clone(&inner)).await {
                        error!(?e, "Background polling task exited");
                    }
                    inner.connected.store(false, Ordering::Relaxed);
                }
            }
        });
        Self {
            inner,
            server_shutdown_signal: None,
        }
    }

    /// Launches background tasks for polling the web server
    async fn run_background_receive(
        inner: Arc<Inner<M, K, E, TYPES, PROPOSAL, VOTE>>,
    ) -> Result<(), ClientError> {
        let proposal_handle = async_spawn({
            let inner_clone = inner.clone();
            async move {
                inner_clone.poll_web_server(MessageType::Proposal, 0).await;
            }
        });
        let vote_handle = async_spawn({
            let inner_clone = inner.clone();

            async move {
                inner_clone
                    .poll_web_server(MessageType::VoteTimedOut, 0)
                    .await;
            }
        });
        let transaction_handle = async_spawn({
            let inner_clone = inner.clone();

            async move {
                inner_clone
                    .poll_web_server(MessageType::Transaction, 0)
                    .await;
            }
        });

        let task_handles = vec![proposal_handle, vote_handle, transaction_handle];

        let children_finished = futures::future::join_all(task_handles);
        let _result = children_finished.await;

        Ok(())
    }
}

/// Enum for matching messages to their type
#[derive(PartialEq)]
enum MessageType {
    /// Message is a transaction
    Transaction,
    /// Message is a vote or time out
    VoteTimedOut,
    /// Message is a proposal
    Proposal,
}

#[async_trait]
impl<
        TYPES: NodeType,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        MEMBERSHIP: Membership<TYPES>,
    > CommunicationChannel<TYPES, PROPOSAL, VOTE, MEMBERSHIP>
    for CentralizedWebCommChannel<TYPES, PROPOSAL, VOTE, MEMBERSHIP>
{
    /// Blocks until node is successfully initialized
    /// into the network
    async fn wait_for_ready(&self) {
        <CentralizedWebServerNetwork<_, _, _, _, _, _> as ConnectedNetwork<
            RecvMsg<Message<TYPES, PROPOSAL, VOTE>>,
            SendMsg<Message<TYPES, PROPOSAL, VOTE>>,
            TYPES::SignatureKey,
        >>::wait_for_ready(&self.0)
        .await;
    }

    /// checks if the network is ready
    /// nonblocking
    async fn is_ready(&self) -> bool {
        <CentralizedWebServerNetwork<_, _, _, _, _, _> as ConnectedNetwork<
            RecvMsg<Message<TYPES, PROPOSAL, VOTE>>,
            SendMsg<Message<TYPES, PROPOSAL, VOTE>>,
            TYPES::SignatureKey,
        >>::is_ready(&self.0)
        .await
    }

    /// Shut down this network. Afterwards this network should no longer be used.
    ///
    /// This should also cause other functions to immediately return with a [`NetworkError`]
    async fn shut_down(&self) -> () {
        <CentralizedWebServerNetwork<_, _, _, _, _, _> as ConnectedNetwork<
            RecvMsg<Message<TYPES, PROPOSAL, VOTE>>,
            SendMsg<Message<TYPES, PROPOSAL, VOTE>>,
            TYPES::SignatureKey,
        >>::shut_down(&self.0)
        .await;
    }

    /// broadcast message to those listening on the communication channel
    /// blocking
    async fn broadcast_message(
        &self,
        message: Message<TYPES, PROPOSAL, VOTE>,
        _election: &MEMBERSHIP,
    ) -> Result<(), NetworkError> {
        let network_msg = self.parse_post_message(message);
        match network_msg {
            Ok(network_msg) => self.0.broadcast_message(network_msg, BTreeSet::new()).await,
            Err(network_msg) => Err(NetworkError::CentralizedWebServer {
                source: network_msg,
            }),
        }
    }

    /// Sends a direct message to a specific node
    /// blocking
    async fn direct_message(
        &self,
        message: Message<TYPES, PROPOSAL, VOTE>,
        recipient: TYPES::SignatureKey,
    ) -> Result<(), NetworkError> {
        let network_msg = self.parse_post_message(message);
        match network_msg {
            Ok(network_msg) => self.0.direct_message(network_msg, recipient).await,
            Err(network_msg) => Err(NetworkError::CentralizedWebServer {
                source: network_msg,
            }),
        }
    }

    /// Moves out the entire queue of received messages of 'transmit_type`
    ///
    /// Will unwrap the underlying `NetworkMessage`
    /// blocking
    async fn recv_msgs(
        &self,
        transmit_type: TransmitType,
    ) -> Result<Vec<Message<TYPES, PROPOSAL, VOTE>>, NetworkError> {
        let result = <CentralizedWebServerNetwork<_, _, _, _, _, _> as ConnectedNetwork<
            RecvMsg<Message<TYPES, PROPOSAL, VOTE>>,
            SendMsg<Message<TYPES, PROPOSAL, VOTE>>,
            TYPES::SignatureKey,
        >>::recv_msgs(&self.0, transmit_type)
        .await;

        match result {
            Ok(messages) => Ok(messages.iter().map(|x| x.get_message().unwrap()).collect()),
            _ => Err(NetworkError::CentralizedWebServer {
                source: CentralizedWebServerNetworkError::ClientError,
            }),
        }
    }

    /// look up a node
    /// blocking
    async fn lookup_node(&self, _pk: TYPES::SignatureKey) -> Result<(), NetworkError> {
        Ok(())
    }

    async fn inject_consensus_info(&self, tuple: (u64, bool, bool)) -> Result<(), NetworkError> {
        <CentralizedWebServerNetwork<_, _, _, _, _, _> as ConnectedNetwork<
            RecvMsg<Message<TYPES, PROPOSAL, VOTE>>,
            SendMsg<Message<TYPES, PROPOSAL, VOTE>>,
            TYPES::SignatureKey,
        >>::inject_consensus_info(&self.0, tuple)
        .await
    }
}

#[async_trait]
impl<
        M: NetworkMsg + 'static,
        K: SignatureKey + 'static,
        E: ElectionConfig + 'static,
        TYPES: NodeType + 'static,
        PROPOSAL: ProposalType<NodeType = TYPES> + 'static,
        VOTE: VoteType<TYPES> + 'static,
    > ConnectedNetwork<RecvMsg<M>, SendMsg<M>, K>
    for CentralizedWebServerNetwork<M, K, E, TYPES, PROPOSAL, VOTE>
{
    /// Blocks until the network is successfully initialized
    async fn wait_for_ready(&self) {
        while !self.inner.connected.load(Ordering::Relaxed) {
            async_sleep(Duration::from_secs(1)).await;
        }
    }

    /// checks if the network is ready
    /// nonblocking
    async fn is_ready(&self) -> bool {
        self.inner.connected.load(Ordering::Relaxed)
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
        message: SendMsg<M>,
        _recipients: BTreeSet<K>,
    ) -> Result<(), NetworkError> {
        self.post_message_to_web_server(message).await
    }

    /// Sends a direct message to a specific node
    /// blocking
    async fn direct_message(&self, message: SendMsg<M>, _recipient: K) -> Result<(), NetworkError> {
        self.post_message_to_web_server(message).await
    }

    /// Moves out the entire queue of received messages of 'transmit_type`
    ///
    /// Will unwrap the underlying `NetworkMessage`
    /// blocking
    async fn recv_msgs(
        &self,
        transmit_type: TransmitType,
    ) -> Result<Vec<RecvMsg<M>>, NetworkError> {
        match transmit_type {
            TransmitType::Direct => {
                let mut queue = self.inner.direct_poll_queue.write().await;
                Ok(queue.drain(..).collect())
            }
            TransmitType::Broadcast => {
                let mut queue = self.inner.broadcast_poll_queue.write().await;
                Ok(queue.drain(..).collect())
            }
        }
    }

    /// look up a node
    /// blocking
    async fn lookup_node(&self, _pk: K) -> Result<(), NetworkError> {
        Ok(())
    }

    async fn inject_consensus_info(&self, tuple: (u64, bool, bool)) -> Result<(), NetworkError> {
        let (view_number, _is_current_leader, is_next_leader) = tuple;

        let new_consensus_info = ConsensusInfo {
            view_number,
            _is_current_leader,
            is_next_leader,
        };

        let mut result = Ok(());
        self.inner
            .consensus_info
            .modify(|old_consensus_info| {
                if new_consensus_info.view_number <= old_consensus_info.view_number {
                    result = Err(NetworkError::CentralizedWebServer {
                        source: CentralizedWebServerNetworkError::IncorrectConsensusData,
                    });
                }
                *old_consensus_info = new_consensus_info;
            })
            .await;
        result
    }
}
impl<
        TYPES: NodeType,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        MEMBERSHIP: Membership<TYPES>,
    > TestableNetworkingImplementation<TYPES, PROPOSAL, VOTE, MEMBERSHIP>
    for CentralizedWebCommChannel<TYPES, PROPOSAL, VOTE, MEMBERSHIP>
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
                "0.0.0.0",
                9000,
                Duration::from_millis(100),
                known_nodes[id as usize].clone(),
            );
            network.server_shutdown_signal = Some(sender);
            CentralizedWebCommChannel::new(network)
        })
    }

    fn in_flight_message_count(&self) -> Option<usize> {
        None
    }
}
