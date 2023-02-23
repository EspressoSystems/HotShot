//! A network implementation that connects to a centralized web server.
//!
//! To run the web server, see the `./centralized_web_server/` folder in this repo.
//!

// TODO ED Remove once ready to merge
#![allow(dead_code, unused, deprecated)]

use async_std::channel::Recv;
#[cfg(feature = "async-std-executor")]
use async_std::net::TcpStream;
use nll::nll_todo::nll_todo;
#[cfg(feature = "tokio-executor")]
use tokio::net::TcpStream;
#[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
std::compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}

use async_compatibility_layer::async_primitives::subscribable_rwlock::ReadView;
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
use std::iter::Rev;
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
        Message<TYPES, PROPOSAL, VOTE>,
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

    fn parse_post_message(
        &self,
        message: Message<TYPES, PROPOSAL, VOTE>,
    ) -> SendMsg<TYPES, PROPOSAL, VOTE> {
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

        let network_msg: SendMsg<TYPES, PROPOSAL, VOTE> = SendMsg {
            message: Some(message),
            endpoint,
        };
        network_msg

        // TODO ED Current web server doesn't have a concept of recipients
    }
}

#[derive(Clone, Debug)]
pub struct CentralizedWebServerNetwork<
    M: NetworkMsg,
    // Why don't we need this? TODO ED
    ///+ WebServerNetworkMessageTrait<TYPES, PROPOSAL, VOTE>
    KEY: SignatureKey,
    ELECTIONCONFIG: ElectionConfig,
    TYPES: NodeType,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
> {
    /// The inner state
    // TODO ED What's the point of inner?
    inner: Arc<Inner<M, KEY, ELECTIONCONFIG, TYPES, PROPOSAL, VOTE>>,
    /// An optional shutdown signal. This is only used when this connection is created through the `TestableNetworkingImplementation` API.
    server_shutdown_signal: Option<Arc<OneShotSender<()>>>,
}

// TODO ED Two impls of centralized web server network struct?  Fix
impl<
        M: NetworkMsg,
        KEY: SignatureKey,
        ELECTIONCONFIG: ElectionConfig,
        TYPES: NodeType,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
    > CentralizedWebServerNetwork<M, KEY, ELECTIONCONFIG, TYPES, PROPOSAL, VOTE>
{
    async fn post_message_to_web_server(
        &self,
        message: SendMsg<TYPES, PROPOSAL, VOTE>,
    ) -> Result<(), NetworkError> {
        let result: Result<(), ClientError> = self
            .inner
            .client
            .post(&message.get_endpoint())
            // TODO ED Sending whole message until we can work out the Generics for M
            .body_binary(&message.get_message())
            .unwrap()
            .send()
            .await;
        // TODO ED Actually return result
        println!("Result is {:?}", result);
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ConsensusInfo {
    view_number: u64,
    is_current_leader: bool,
    is_next_leader: bool,
}

#[derive(Debug)]
struct Inner<
    M: NetworkMsg,
    KEY: SignatureKey,
    ELECTIONCONFIG: ElectionConfig,
    TYPES: NodeType,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
> {
    // TODO ED Get rid of phantom if can
    phantom: PhantomData<(KEY, ELECTIONCONFIG, PROPOSAL, VOTE)>,
    // Current view number so we can poll accordingly
    // TODO ED Should we keep these as three objects or one?
    // view_number: Arc<SubscribableRwLock<<TYPES as NodeType>::Time>>,
    // is_current_leader: Arc<SubscribableRwLock<bool>>,
    // is_next_leader: Arc<SubscribableRwLock<bool>>,
    consensus_info: Arc<SubscribableRwLock<ConsensusInfo>>,

    // TODO Do we ever use this?
    own_key: TYPES::SignatureKey,
    // // Queue for broadcasted messages (mainly transactions and proposals)
    broadcast_poll_queue: Arc<RwLock<Vec<RecvMsg<M>>>>,
    // // Queue for direct messages (mainly votes)
    // Should this be channels? TODO ED
    direct_poll_queue: Arc<RwLock<Vec<RecvMsg<M>>>>,
    // TODO ED the same as connected?
    running: AtomicBool,
    // The network is connected to the web server and ready to go
    connected: AtomicBool,
    client: surf_disco::Client<ClientError>,
    wait_between_polls: Duration,
}

impl<
        M: NetworkMsg, //+ WebServerNetworkMessageTrait<TYPES, PROPOSAL, VOTE>,
        KEY: SignatureKey,
        ELECTIONCONFIG: ElectionConfig,
        TYPES: NodeType,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
    > Inner<M, KEY, ELECTIONCONFIG, TYPES, PROPOSAL, VOTE>
{
    async fn poll_web_server(&self, message_type: MessageType, num_views_ahead: u64) {
        // Subscribe to changes in consensus info
        let consensus_update = self.consensus_info.subscribe().await;
        let mut consensus_info = self.consensus_info.copied().await;
        let mut vote_index: u64 = 0;
        let mut tx_index: u64 = 0;

        loop {
            let endpoint = match message_type {
                MessageType::Proposal => {
                    config::get_proposal_route(consensus_info.view_number.into())
                }
                MessageType::VoteTimedOut => {
                    config::get_vote_route(consensus_info.view_number.into(), vote_index.into())
                }
                MessageType::Transaction => config::get_transactions_route(tx_index.into()),
            };
            // println!("Endpoint is {}", endpoint);
            let possible_message = if message_type == MessageType::VoteTimedOut {
                // are we the leader?
                // TODO ED Can refactor this to be more readable

                if consensus_info.is_next_leader {
                    self.get_message_from_web_server(endpoint).await
                } else {
                    Ok(None)
                }
            } else {
                self.get_message_from_web_server(endpoint).await
            };

            match possible_message {
                // TODO ED Only need the first proposal
                Ok(Some(deserialized_messages)) => {
                    match message_type {
                        MessageType::Proposal => {
                            error!("Got proposal");
                            // println!("PROP IS: {:?}", deserialized_messages[0].clone());
                            // println!("SIZE OF PROP IS: {:?}", size_of_val(&*deserialized_messages));

                            // Only pushing the first proposal here since we will soon only be allowing 1 proposal per view
                            self.broadcast_poll_queue
                                .write()
                                .await
                                .push(deserialized_messages[0].clone());
                            consensus_info = consensus_update.recv().await.unwrap();
                        }
                        MessageType::VoteTimedOut => {
                            error!("Got vote or timed out message");
                            // ED TODO - Stop getting votes once we've recieved a QC's worth

                            let mut direct_poll_queue = self.direct_poll_queue.write().await;
                            deserialized_messages.iter().for_each(|vote| {
                                vote_index += 1;
                                direct_poll_queue.push(vote.clone());
                            });
                        }
                        MessageType::Transaction => {
                            error!("Got txs");
                            // println!("SIZE OF TX IS: {:?}", size_of_val(&*deserialized_messages));
                            let mut lock = self.broadcast_poll_queue.write().await;
                            deserialized_messages.iter().for_each(|tx| {
                                tx_index += 1;
                                lock.push(tx.clone());
                            });
                        } // println!("Deserialized message is: {:?}", deserialized_messages[0]);
                          // self.broadcast_poll_queue
                          //     .write()
                          //     .await
                          //     .push(deserialized_messages[0].clone());
                          // consensus_info = consensus_update.recv().await.unwrap();
                          // consensus_info = self.consensus_info.copied().await;
                    }
                }
                // TODO ED Currently should never be hit
                Ok(None) => {
                    async_sleep(self.wait_between_polls).await;
                }

                // TODO ED Keeping these separate in case we want to do something different later
                // Also implement better server error instead of NotImplemented
                Err(e) => {
                    // sleep a bit before repolling
                    // println!("ERROR IS {:?}", e);
                    // TODO ED Requires us sending the endpoint along with?
                    async_sleep(self.wait_between_polls).await;
                }
            }
            let new_consensus_info = consensus_update.try_recv();
            if new_consensus_info.is_ok() {
                consensus_info = new_consensus_info.unwrap();
                vote_index = 0;
            }
            // Don't do anything until we're in a new view
        }
    }

    async fn get_message_from_web_server(
        &self,
        endpoint: String,
    ) -> Result<Option<Vec<RecvMsg<M>>>, ClientError> {
        let result: Result<Option<Vec<Vec<u8>>>, ClientError> =
            self.client.get(&endpoint).send().await;
        // TODO ED Clean this up
        match result {
            Err(error) => Err(error),
            Ok(Some(messages)) => {
                let mut deserialized_messages = Vec::new();
                messages.iter().for_each(|message| {
                    let deserialized_message = bincode::deserialize(message).unwrap();
                    deserialized_messages.push(deserialized_message);
                });
                Ok(Some(deserialized_messages))
            }
            Ok(None) => Ok(None),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]

pub struct SendMsg<TYPES: NodeType, PROPOSAL: ProposalType<NodeType = TYPES>, VOTE: VoteType<TYPES>>
{
    message: Option<Message<TYPES, PROPOSAL, VOTE>>,
    endpoint: String,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]
pub struct RecvMsg<M: NetworkMsg> {
    message: Option<M>,
}

// Ideally you'd want it to be generic over any network msg, but for now this is fine
pub trait SendMsgTrait<
    TYPES: NodeType,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
>
{
    fn get_endpoint(&self) -> String;
    fn get_message(&self) -> Option<Message<TYPES, PROPOSAL, VOTE>>;
}

pub trait RecvMsgTrait<M: NetworkMsg> {
    fn get_message(&self) -> Option<M>;
}

impl<TYPES: NodeType, PROPOSAL: ProposalType<NodeType = TYPES>, VOTE: VoteType<TYPES>>
    SendMsgTrait<TYPES, PROPOSAL, VOTE> for SendMsg<TYPES, PROPOSAL, VOTE>
{
    // TODO ED String doesn't impl copy?
    fn get_endpoint(&self) -> String {
        self.endpoint.clone()
    }

    fn get_message(&self) -> Option<Message<TYPES, PROPOSAL, VOTE>> {
        self.message.clone()
    }
}

impl<M: NetworkMsg> RecvMsgTrait<M> for RecvMsg<M> {
    fn get_message(&self) -> Option<M> {
        self.message.clone()
    }
}

impl<TYPES: NodeType, PROPOSAL: ProposalType<NodeType = TYPES>, VOTE: VoteType<TYPES>> NetworkMsg
    for SendMsg<TYPES, PROPOSAL, VOTE>
{
}
impl<M: NetworkMsg> NetworkMsg for RecvMsg<M> {}

impl<
        M: NetworkMsg + 'static, //+ WebServerNetworkMessageTrait<TYPES, PROPOSAL, VOTE>
        K: SignatureKey + 'static,
        E: ElectionConfig + 'static,
        TYPES: NodeType + 'static,
        PROPOSAL: ProposalType<NodeType = TYPES> + 'static,
        VOTE: VoteType<TYPES> + 'static,
    > CentralizedWebServerNetwork<M, K, E, TYPES, PROPOSAL, VOTE>
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

        async_spawn({
            let inner = Arc::clone(&inner);
            async move {
                while inner.running.load(Ordering::Relaxed) {
                    if let Err(e) = CentralizedWebServerNetwork::<M, K, E, TYPES, PROPOSAL, VOTE>::run_background_receive(Arc::clone(&inner)).await {
                        error!(?e, "background thread exited");
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

    // TODO ED Move to inner impl?
    async fn run_background_receive(
        inner: Arc<Inner<M, K, E, TYPES, PROPOSAL, VOTE>>,
    ) -> Result<(), ClientError> {
        // TODO ED Change running variable if this function closes
        // TODO ED Do we need this function wrapper?  We could start all of them directly
        let proposal_handle = async_spawn({
            let inner_clone = inner.clone();
            async move { inner_clone.poll_web_server(MessageType::Proposal, 0).await }
        });
        let vote_handle = async_spawn({
            let inner_clone = inner.clone();

            async move {
                inner_clone
                    .poll_web_server(MessageType::VoteTimedOut, 0)
                    .await
            }
        });
        let transaction_handle = async_spawn({
            let inner_clone = inner.clone();

            async move {
                inner_clone
                    .poll_web_server(MessageType::Transaction, 0)
                    .await
            }
        });

        let mut task_handles = Vec::new();
        task_handles.push(proposal_handle);
        task_handles.push(vote_handle);
        task_handles.push(transaction_handle);
        // task_handles.push(proposal_handle_plus_one);

        let children_finished = futures::future::join_all(task_handles);
        let result = children_finished.await;

        Ok(())
    }
}
#[derive(PartialEq)]
enum MessageType {
    Transaction,
    VoteTimedOut,
    Proposal,
}

#[async_trait]
impl<
        // M: NetworkMsg,
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
        <CentralizedWebServerNetwork<_, _, _, _, _, _> as ConnectedNetwork<
            RecvMsg<Message<TYPES, PROPOSAL, VOTE>>,
            SendMsg<TYPES, PROPOSAL, VOTE>,
            TYPES::SignatureKey,
        >>::wait_for_ready(&self.0)
        .await;
    }

    /// checks if the network is ready
    /// nonblocking
    async fn is_ready(&self) -> bool {
        <CentralizedWebServerNetwork<_, _, _, _, _, _> as ConnectedNetwork<
            RecvMsg<Message<TYPES, PROPOSAL, VOTE>>,
            SendMsg<TYPES, PROPOSAL, VOTE>,
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
            SendMsg<TYPES, PROPOSAL, VOTE>,
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
        // TODO ED Change parse post message to get endpoint or something similar?
        let network_msg = self.parse_post_message(message);
        self.0.broadcast_message(network_msg, BTreeSet::new()).await
    }

    /// Sends a direct message to a specific node
    /// blocking
    async fn direct_message(
        &self,
        message: Message<TYPES, PROPOSAL, VOTE>,
        recipient: TYPES::SignatureKey,
    ) -> Result<(), NetworkError> {
        let network_msg = self.parse_post_message(message);
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
        let result = <CentralizedWebServerNetwork<_, _, _, _, _, _> as ConnectedNetwork<
            RecvMsg<Message<TYPES, PROPOSAL, VOTE>>,
            SendMsg<TYPES, PROPOSAL, VOTE>,
            TYPES::SignatureKey,
        >>::recv_msgs(&self.0, transmit_type)
        .await;

        match result {
            Ok(messages) => {
                // println!("Received proposal message !!!!! {:?}", messages);

                Ok(messages.iter().map(|x| x.get_message().unwrap()).collect())
            }
            _ => Err(NetworkError::UnimplementedFeature),
        }
        // Ok(Vec::new())
    }

    /// look up a node
    /// blocking
    async fn lookup_node(&self, pk: TYPES::SignatureKey) -> Result<(), NetworkError> {
        Ok(())
    }

    async fn inject_consensus_info(&self, tuple: (u64, bool, bool)) -> Result<(), NetworkError> {
        <CentralizedWebServerNetwork<_, _, _, _, _, _> as ConnectedNetwork<
            RecvMsg<Message<TYPES, PROPOSAL, VOTE>>,
            SendMsg<TYPES, PROPOSAL, VOTE>,
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
    > ConnectedNetwork<RecvMsg<M>, SendMsg<TYPES, PROPOSAL, VOTE>, K>
    for CentralizedWebServerNetwork<M, K, E, TYPES, PROPOSAL, VOTE>
// Make this a trait?
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
        message: SendMsg<TYPES, PROPOSAL, VOTE>,
        recipients: BTreeSet<K>,
    ) -> Result<(), NetworkError> {
        let result = self.post_message_to_web_server(message).await;

        // TODO ED Match result

        Ok(())
    }

    /// Sends a direct message to a specific node
    /// blocking
    async fn direct_message(
        &self,
        message: SendMsg<TYPES, PROPOSAL, VOTE>,
        recipient: K,
    ) -> Result<(), NetworkError> {
        let result = self.post_message_to_web_server(message).await;

        // TODO ED Match result

        Ok(())
    }

    /// Moves out the entire queue of received messages of 'transmit_type`
    ///
    /// Will unwrap the underlying `NetworkMessage`
    /// blocking
    async fn recv_msgs(
        &self,
        transmit_type: TransmitType,
    ) -> Result<Vec<RecvMsg<M>>, NetworkError> {
        // TODO ED Implement
        match transmit_type {
            TransmitType::Direct => {
                let mut queue = self.inner.direct_poll_queue.write().await;
                Ok(queue.drain(..).collect())
            }
            TransmitType::Broadcast => {
                let mut queue = self.inner.broadcast_poll_queue.write().await;
                Ok(queue.drain(..).collect())
                // Ok(messages)
            }
        }
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
        self.inner
            .consensus_info
            .modify(|old_consensus_info| {
                // TODO ED This should never happen, but checking anyway
                if new_consensus_info.view_number <= old_consensus_info.view_number {
                    panic!();
                }
                *old_consensus_info = new_consensus_info;
            })
            .await;

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
