//! A network implementation that connects to a web server.
//!
//! To run the web server, see the `./web_server/` folder in this repo.
//!

#[cfg(feature = "async-std-executor")]
#[cfg(feature = "tokio-executor")]
#[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
std::compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}

use async_compatibility_layer::async_primitives::subscribable_rwlock::ReadView;
use async_compatibility_layer::async_primitives::subscribable_rwlock::SubscribableRwLock;
use async_compatibility_layer::channel::unbounded;
use async_compatibility_layer::channel::UnboundedReceiver;
use async_compatibility_layer::channel::UnboundedSender;

use async_compatibility_layer::{
    art::{async_sleep, async_spawn},
    channel::{oneshot, OneShotSender},
};
use async_lock::RwLock;
use async_trait::async_trait;
use hotshot_task::{boxed_sync, BoxSyncFuture};
use hotshot_task_impls::da;
use hotshot_types::message::{Message, MessagePurpose};
use hotshot_types::traits::network::ConsensusIntentEvent;
use hotshot_types::traits::node_implementation::NodeImplementation;
use hotshot_types::{
    data::ProposalType,
    traits::{
        election::{ElectionConfig, Membership},
        network::{
            CommunicationChannel, ConnectedNetwork, NetworkError, NetworkMsg,
            TestableChannelImplementation, TestableNetworkingImplementation, TransmitType,
            WebServerNetworkError,
        },
        node_implementation::NodeType,
        signature_key::{SignatureKey, TestableSignatureKey},
    },
    vote::VoteType,
};
use hotshot_web_server::{self, config};
use rand::random;
use serde::{Deserialize, Serialize};

use hotshot_types::traits::network::ViewMessage;
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
use tracing::{error, info, warn};
/// Represents the communication channel abstraction for the web server
#[derive(Clone, Debug)]
pub struct WebCommChannel<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
    MEMBERSHIP: Membership<TYPES>,
>(
    Arc<WebServerNetwork<Message<TYPES, I>, TYPES::SignatureKey, TYPES::ElectionConfigType, TYPES>>,
    PhantomData<(MEMBERSHIP, I, PROPOSAL, VOTE)>,
);

impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES>,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        MEMBERSHIP: Membership<TYPES>,
    > WebCommChannel<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP>
{
    /// Create new communication channel
    #[must_use]
    pub fn new(
        network: Arc<
            WebServerNetwork<
                Message<TYPES, I>,
                TYPES::SignatureKey,
                TYPES::ElectionConfigType,
                TYPES,
            >,
        >,
    ) -> Self {
        Self(network, PhantomData::default())
    }
}

/// The web server network state
#[derive(Clone, Debug)]
pub struct WebServerNetwork<
    M: NetworkMsg,
    KEY: SignatureKey,
    ELECTIONCONFIG: ElectionConfig,
    TYPES: NodeType,
> {
    /// The inner, core state of the web server network
    inner: Arc<Inner<M, KEY, ELECTIONCONFIG, TYPES>>,
    /// An optional shutdown signal. This is only used when this connection is created through the `TestableNetworkingImplementation` API.
    server_shutdown_signal: Option<Arc<OneShotSender<()>>>,
}

impl<M: NetworkMsg, KEY: SignatureKey, ELECTIONCONFIG: ElectionConfig, TYPES: NodeType>
    WebServerNetwork<M, KEY, ELECTIONCONFIG, TYPES>
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
        // error!("POST message error for endpoint {} is {:?}", &message.get_endpoint(), result.clone());
        result.map_err(|_e| NetworkError::WebServer {
            source: WebServerNetworkError::ClientError,
        })
    }
}

/// Consensus info that is injected from `HotShot`
#[derive(Debug, Default, Clone, Copy)]
pub struct ConsensusInfo {
    /// The latest view number
    view_number: u64,
    /// Whether this node is the leader of `view_number`
    is_current_leader: bool,
    /// Whether this node is the leader of the next view
    is_next_leader: bool,
}

/// Represents the core of web server networking
#[derive(Debug)]
struct Inner<M: NetworkMsg, KEY: SignatureKey, ELECTIONCONFIG: ElectionConfig, TYPES: NodeType> {
    /// Phantom data for generic types
    phantom: PhantomData<(KEY, ELECTIONCONFIG)>,
    /// Nodes in the election committee (either quorum election or committee election)
    _committee_nodes: Vec<TYPES::SignatureKey>,
    /// Whether or not we are in the committee nodes
    on_committee: bool,
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
    /// Whether we are connecting to a DA server
    is_da: bool,

    // /// Channel for consensus to send the web server information about what it should poll
    // consensus_intent_sender: UnboundedSender<ConsensusIntentEvent>,

    // /// Channel to receive polling intents from consensus
    // consensus_intent_receiver: UnboundedReceiver<ConsensusIntentEvent>,

    // proposal_receiver: UnboundedReceiver<ConsensusIntentEvent>,
    proposal_sender: UnboundedSender<ConsensusIntentEvent>,

    // vote_receiver: UnboundedReceiver<ConsensusIntentEvent>,
    vote_sender: UnboundedSender<ConsensusIntentEvent>,
}

impl<M: NetworkMsg, KEY: SignatureKey, ELECTIONCONFIG: ElectionConfig, TYPES: NodeType>
    Inner<M, KEY, ELECTIONCONFIG, TYPES>
{
    /// Polls the web server at a given endpoint while the client is running
    async fn poll_web_server(
        &self,
        message_purpose: MessagePurpose,
        num_views_ahead: u64,
    ) -> Result<(), NetworkError> {
        // Subscribe to changes in consensus info
        let consensus_update = self.consensus_info.subscribe().await;
        let mut consensus_info = self.consensus_info.copied().await;
        let mut vote_index: u64 = 0;
        let mut tx_index: u64 = 0;

        while self.running.load(Ordering::Relaxed) {
            let view_number = consensus_info.view_number + num_views_ahead;
            let endpoint = match message_purpose {
                MessagePurpose::Proposal => config::get_proposal_route(view_number),
                MessagePurpose::Vote => {
                    config::get_vote_route((view_number).wrapping_sub(1), vote_index)
                }
                MessagePurpose::Data => config::get_transactions_route(tx_index),
                MessagePurpose::Internal => unimplemented!(),
                MessagePurpose::ViewSyncProposal => {
                    config::get_view_sync_proposal_route(view_number)
                }
                MessagePurpose::ViewSyncVote => {
                    config::get_view_sync_vote_route(view_number, vote_index)
                }
                MessagePurpose::DAC => config::get_da_certificate_route(view_number),
            };

            // TODO ED To account for DA, this logic polls all votes if a node
            // is the current leader or next leader, which is inefficient.  But this
            // will be updated during the run_view refactor. If this causes performance
            // issues we can revert back to using MessagePurposeDestination
            let possible_message = if message_purpose == (MessagePurpose::Vote)
                && (!consensus_info.is_current_leader && !consensus_info.is_next_leader)
            {
                Ok(None)
            } else {
                self.get_message_from_web_server(endpoint).await
            };

            match possible_message {
                Ok(Some(deserialized_messages)) => {
                    match message_purpose {
                        MessagePurpose::Proposal => {
                            error!(
                                "Received proposal from web server for view {} {}",
                                view_number, self.is_da
                            );
                            // Only pushing the first proposal since we will soon only be allowing 1 proposal per view
                            self.broadcast_poll_queue
                                .write()
                                .await
                                .push(deserialized_messages[0].clone());
                            // Wait for the view to change before polling for proposals again
                            consensus_info = consensus_update.recv().await.unwrap();
                        }
                        MessagePurpose::Vote => {
                            error!(
                                "Received {} votes from web server for view {} is da {}",
                                deserialized_messages.len(),
                                view_number,
                                self.is_da
                            );
                            let mut direct_poll_queue = self.direct_poll_queue.write().await;
                            for vote in &deserialized_messages {
                                vote_index += 1;
                                direct_poll_queue.push(vote.clone());
                            }
                        }
                        MessagePurpose::Data => {
                            info!("Received new from web server transaction");
                            let mut lock = self.broadcast_poll_queue.write().await;
                            for tx in &deserialized_messages {
                                tx_index += 1;
                                lock.push(tx.clone());
                            }
                        }
                        MessagePurpose::DAC => {
                            error!("Received DAC from web server");
                            let mut lock = self.broadcast_poll_queue.write().await;
                            lock.push(deserialized_messages[0].clone());
                            // TODO ED Wait until next view?
                            consensus_info = consensus_update.recv().await.unwrap();
                        }
                        MessagePurpose::Internal => {
                            unimplemented!()
                        }
                        MessagePurpose::ViewSyncProposal => todo!(),
                        MessagePurpose::ViewSyncVote => todo!(),
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
        Err(NetworkError::ShutDown)
    }

    async fn poll_web_server_new(
        &self,
        receiver: UnboundedReceiver<ConsensusIntentEvent>,
        message_purpose: MessagePurpose,
    ) -> Result<(), NetworkError> {
        let mut view_number = 0;
        let mut vote_index = 0;
        let mut tx_index = 0;

        while self.running.load(Ordering::Relaxed) {
            let endpoint = match message_purpose {
                MessagePurpose::Proposal => config::get_proposal_route(view_number),
                MessagePurpose::Vote => {
                    config::get_vote_route((view_number).wrapping_sub(1), vote_index)
                }
                MessagePurpose::Data => config::get_transactions_route(tx_index),
                MessagePurpose::Internal => unimplemented!(),
                MessagePurpose::ViewSyncProposal => {
                    config::get_view_sync_proposal_route(view_number)
                }
                MessagePurpose::ViewSyncVote => {
                    config::get_view_sync_vote_route(view_number, vote_index)
                }
                MessagePurpose::DAC => config::get_da_certificate_route(view_number),
            };

            let possible_message = self.get_message_from_web_server(endpoint).await;
            // error!("Polling for view {}", view_number);

            match possible_message {
                Ok(Some(deserialized_messages)) => {
                    match message_purpose {
                        MessagePurpose::Proposal => {
                            warn!(
                                "Received proposal from web server for view {} {}",
                                view_number, self.is_da
                            );
                            // Only pushing the first proposal since we will soon only be allowing 1 proposal per view
                            self.broadcast_poll_queue
                                .write()
                                .await
                                .push(deserialized_messages[0].clone());
                            // Wait for the view to change before polling for proposals again
                            let event = receiver.recv().await;
                            match event {
                                Ok(event) => view_number = event.view_number(),
                                Err(_r) => {
                                    error!("Proposal receiver error!  It was likely shutdown")
                                }
                            }
                        }

                        _ => todo!(),
                    }
                }
                Ok(None) => {
                    async_sleep(self.wait_between_polls).await;
                }
                Err(_e) => {
                    async_sleep(self.wait_between_polls).await;
                }
            }
            let maybe_event = receiver.try_recv();
            view_number = match maybe_event {
                Ok(event) => {
                    error!("Received event!");
                    vote_index = 0;
                    event.view_number()
                }
                Err(_e) => view_number,
            };
        }
        Err(NetworkError::ShutDown)
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
            Err(_error) => Err(NetworkError::WebServer {
                source: WebServerNetworkError::ClientError,
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
        M: NetworkMsg + 'static + ViewMessage<TYPES>,
        K: SignatureKey + 'static,
        E: ElectionConfig + 'static,
        TYPES: NodeType + 'static,
    > WebServerNetwork<M, K, E, TYPES>
{
    /// Creates a new instance of the `WebServerNetwork`
    /// # Panics
    /// if the web server url is malformed
    pub fn create(
        host: &str,
        port: u16,
        wait_between_polls: Duration,
        key: TYPES::SignatureKey,
        _committee_nodes: Vec<TYPES::SignatureKey>,
        is_da_server: bool,
    ) -> Self {
        let base_url_string = format!("http://{host}:{port}");
        error!("Connecting to web server at {base_url_string:?} is da: {is_da_server}");

        let base_url = base_url_string.parse();
        if base_url.is_err() {
            error!("Web server url {:?} is malformed", base_url_string);
        }

        // TODO ED Wait for healthcheck
        let client = surf_disco::Client::<ClientError>::new(base_url.unwrap());

        // let healthcheck = format!("{base_url_string}/api");
        // // Healthcheck
        // if client.get(&healthcheck).send().await.is_err() {
        //     panic!("healthcheck is not right");
        // }

        let on_committee = _committee_nodes.contains(&key);
        // let (consensus_intent_sender, consensus_intent_receiver) = unbounded();
        let (vote_sender, vote_receiver) = unbounded::<ConsensusIntentEvent>();
        let (proposal_sender, proposal_receiver) = unbounded::<ConsensusIntentEvent>();

        let inner = Arc::new(Inner {
            phantom: PhantomData,
            _committee_nodes,
            on_committee,
            consensus_info: Arc::default(),
            broadcast_poll_queue: Arc::default(),
            direct_poll_queue: Arc::default(),
            running: AtomicBool::new(true),
            connected: AtomicBool::new(false),
            client,
            wait_between_polls,
            _own_key: key,
            is_da: is_da_server,
            proposal_sender: proposal_sender.clone(),
            vote_sender: vote_sender.clone(),
        });

        inner.connected.store(true, Ordering::Relaxed);


        match is_da_server {
            // We are polling for DA-related events
            true => {
                let da_proposal_handle = async_spawn({
                    let inner_clone = inner.clone();
                    async move {
                        if let Err(e) = inner_clone
                            .poll_web_server_new(proposal_receiver, MessagePurpose::Proposal)
                            .await
                        {
                            error!(
                                "Background receive proposal polling encountered an error: {:?}",
                                e
                            );
                        }
                    }
                });

                // da_proposal_handle.await;
            }

            // We are polling for regular consensus events
            false => {
                let quorum_proposal_handle = async_spawn({
                    let inner_clone = inner.clone();
                    async move {
                        if let Err(e) = inner_clone
                            .poll_web_server_new(proposal_receiver, MessagePurpose::Proposal)
                            .await
                        {
                            error!(
                                "Background receive proposal polling encountered an error: {:?}",
                                e
                            );
                        }
                    }
                });
            }
        }

        // async_spawn({
        //     let inner = Arc::clone(&inner);
        //     async move {
        //         while inner.running.load(Ordering::Relaxed) {
        //             if let Err(e) = WebServerNetwork::<M, K, E, TYPES>::run_background_receive(
        //                 Arc::clone(&inner),
        //                 is_da_server,
        //             )
        //             .await
        //             {
        //                 error!(?e, "Background polling task exited");
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

    /// Launches background tasks for polling the web server
    async fn run_background_receive(
        inner: Arc<Inner<M, K, E, TYPES>>,
        is_da: bool,
    ) -> Result<(), ClientError> {
        match is_da {
            // TODO ED Deprecetate this funciton
            // We are polling for DA-related events
            true => {

                //     let da_proposal_handle = async_spawn({
                //         let inner_clone = inner.clone();
                //         async move {
                //             if let Err(e) = inner_clone
                //                 .poll_web_server_new()
                //                 .await
                //             {
                //                 error!(
                //                     "Background receive proposal polling encountered an error: {:?}",
                //                     e
                //                 );
                //             }
                //         }
                //     });
            }

            // We are polling for regular consensus events
            false => {}
        }

        // match is_da {
        //     false => {
        //         let quorum_proposal_handle = async_spawn({
        //             let inner_clone = inner.clone();
        //             async move {
        //                 if let Err(e) = inner_clone
        //                     .poll_web_server(MessagePurpose::Proposal, 0)
        //                     .await
        //                 {
        //                     error!(
        //                         "Background receive proposal polling encountered an error: {:?}",
        //                         e
        //                     );
        //                 }
        //             }
        //         });
        //         let quorum_vote_handle = async_spawn({
        //             let inner_clone = inner.clone();

        //             async move {
        //                 if let Err(e) = inner_clone.poll_web_server(MessagePurpose::Vote, 0).await {
        //                     error!(
        //                         "Background receive vote polling encountered an error: {:?}",
        //                         e
        //                     );
        //                 }
        //             }
        //         });
        //         let quorum_vote_handle = async_spawn({
        //             let inner_clone = inner.clone();

        //             async move {
        //                 if let Err(e) = inner_clone.poll_web_server(MessagePurpose::Vote, 1).await {
        //                     error!(
        //                         "Background receive vote polling encountered an error: {:?}",
        //                         e
        //                     );
        //                 }
        //             }
        //         });
        //         let view_sync_proposal_handle = async_spawn({
        //             let inner_clone = inner.clone();
        //             async move {
        //                 // Exit task if we are not on committee

        //                 if let Err(e) = inner_clone
        //                     .poll_web_server(MessagePurpose::ViewSyncProposal, 1)
        //                     .await
        //                 {
        //                     error!(
        //                         "Background receive proposal polling encountered an error: {:?}",
        //                         e
        //                     );
        //                 }
        //             }
        //         });
        //         let view_sync_vote_handle = async_spawn({
        //             let inner_clone = inner.clone();

        //             async move {
        //                 if let Err(e) = inner_clone
        //                     .poll_web_server(MessagePurpose::ViewSyncVote, 2)
        //                     .await
        //                 {
        //                     error!(
        //                         "Background receive vote polling encountered an error: {:?}",
        //                         e
        //                     );
        //                 }
        //             }
        //         });
        //         let da_cert_handle = async_spawn({
        //             let inner_clone = inner.clone();

        //             async move {
        //                 if let Err(e) = inner_clone.poll_web_server(MessagePurpose::DAC, 0).await {
        //                     error!(
        //                         "Background receive vote polling encountered an error: {:?}",
        //                         e
        //                     );
        //                 }
        //             }
        //         });

        //         let da_handle_2 = async_spawn({
        //             let inner_clone = inner.clone();

        //             async move {
        //                 if let Err(e) = inner_clone.poll_web_server(MessagePurpose::DAC, 1).await {
        //                     error!(
        //                         "Background receive vote polling encountered an error: {:?}",
        //                         e
        //                     );
        //                 }
        //             }
        //         });

        //         let task_handles = vec![
        //             quorum_proposal_handle,
        //             quorum_vote_handle,
        //             view_sync_proposal_handle,
        //             view_sync_vote_handle, // committee_transaction_handle,
        //             da_cert_handle,
        //             da_handle_2,
        //         ];

        //         let _children_finished = futures::future::join_all(task_handles).await;
        //     }
        //     true => {
        //         let committee_transaction_handle = async_spawn({
        //             let inner_clone = inner.clone();

        //             async move {
        //                 if let Err(e) = inner_clone.poll_web_server(MessagePurpose::Data, 0).await {
        //                     error!(
        //                         "Background receive transaction polling encountered an error: {:?}",
        //                         e
        //                     );
        //                 }
        //             }
        //         });
        //         let committee_proposal_handle = async_spawn({
        //             let inner_clone = inner.clone();
        //             async move {
        //                 // Exit task if we are not on committee
        //                 if !inner_clone.on_committee {
        //                     return;
        //                 }
        //                 if let Err(e) = inner_clone
        //                     .poll_web_server(MessagePurpose::Proposal, 1)
        //                     .await
        //                 {
        //                     error!(
        //                         "Background receive proposal polling encountered an error: {:?}",
        //                         e
        //                     );
        //                 }
        //             }
        //         });
        //         let committee_vote_handle = async_spawn({
        //             let inner_clone = inner.clone();

        //             async move {
        //                 if let Err(e) = inner_clone.poll_web_server(MessagePurpose::Vote, 1).await {
        //                     error!(
        //                         "Background receive vote polling encountered an error: {:?}",
        //                         e
        //                     );
        //                 }
        //             }
        //         });
        //         let committee_vote_handle2 = async_spawn({
        //             let inner_clone = inner.clone();

        //             async move {
        //                 if let Err(e) = inner_clone.poll_web_server(MessagePurpose::Vote, 0).await {
        //                     error!(
        //                         "Background receive vote polling encountered an error: {:?}",
        //                         e
        //                     );
        //                 }
        //             }
        //         });
        //         let task_handles = vec![
        //             committee_proposal_handle,
        //             committee_vote_handle,
        //             committee_transaction_handle,
        //             committee_vote_handle2
        //         ];

        //         let _children_finished = futures::future::join_all(task_handles).await;
        //     }
        // }
        Ok(())
    }

    /// Parses a message to find the appropriate endpoint
    /// Returns a `SendMsg` containing the endpoint
    fn parse_post_message(message: M) -> Result<SendMsg<M>, WebServerNetworkError> {
        let view_number: TYPES::Time = message.get_view_number();

        let endpoint = match &message.purpose() {
            MessagePurpose::Proposal => config::post_proposal_route(*view_number),
            MessagePurpose::Vote => config::post_vote_route(*view_number),
            MessagePurpose::Data => config::post_transactions_route(),
            MessagePurpose::Internal => return Err(WebServerNetworkError::EndpointError),
            MessagePurpose::ViewSyncProposal => config::post_view_sync_proposal_route(*view_number),
            MessagePurpose::ViewSyncVote => config::post_view_sync_vote_route(*view_number),
            MessagePurpose::DAC => config::post_da_certificate_route(*view_number),
        };

        let network_msg: SendMsg<M> = SendMsg {
            message: Some(message),
            endpoint,
        };
        Ok(network_msg)
    }
}

#[async_trait]
impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES>,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        MEMBERSHIP: Membership<TYPES>,
    > CommunicationChannel<TYPES, Message<TYPES, I>, PROPOSAL, VOTE, MEMBERSHIP>
    for WebCommChannel<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP>
{
    type NETWORK =
        WebServerNetwork<Message<TYPES, I>, TYPES::SignatureKey, TYPES::ElectionConfigType, TYPES>;
    /// Blocks until node is successfully initialized
    /// into the network
    async fn wait_for_ready(&self) {
        <WebServerNetwork<_, _, _, _> as ConnectedNetwork<
            Message<TYPES, I>,
            TYPES::SignatureKey,
        >>::wait_for_ready(&self.0)
        .await;
    }

    /// checks if the network is ready
    /// nonblocking
    async fn is_ready(&self) -> bool {
        <WebServerNetwork<_, _, _, _,> as ConnectedNetwork<
            Message<TYPES, I>,
            TYPES::SignatureKey,
        >>::is_ready(&self.0)
        .await
    }

    /// Shut down this network. Afterwards this network should no longer be used.
    ///
    /// This should also cause other functions to immediately return with a [`NetworkError`]
    fn shut_down<'a, 'b>(&'a self) -> BoxSyncFuture<'b, ()>
    where
        'a: 'b,
        Self: 'b,
    {
        let closure = async move {
            <WebServerNetwork<_, _, _, _> as ConnectedNetwork<
                Message<TYPES, I>,
                TYPES::SignatureKey,
            >>::shut_down(&self.0)
            .await;
        };
        boxed_sync(closure)
    }

    /// broadcast message to those listening on the communication channel
    /// blocking
    async fn broadcast_message(
        &self,
        message: Message<TYPES, I>,
        _election: &MEMBERSHIP,
    ) -> Result<(), NetworkError> {
        self.0.broadcast_message(message, BTreeSet::new()).await
    }

    /// Sends a direct message to a specific node
    /// blocking
    async fn direct_message(
        &self,
        message: Message<TYPES, I>,
        recipient: TYPES::SignatureKey,
    ) -> Result<(), NetworkError> {
        self.0.direct_message(message, recipient).await
    }

    /// Moves out the entire queue of received messages of 'transmit_type`
    ///
    /// Will unwrap the underlying `NetworkMessage`
    /// blocking
    fn recv_msgs<'a, 'b>(
        &'a self,
        transmit_type: TransmitType,
    ) -> BoxSyncFuture<'b, Result<Vec<Message<TYPES, I>>, NetworkError>>
    where
        'a: 'b,
        Self: 'b,
    {
        let closure = async move {
            <WebServerNetwork<_, _, _, _> as ConnectedNetwork<
                Message<TYPES, I>,
                TYPES::SignatureKey,
            >>::recv_msgs(&self.0, transmit_type)
            .await
        };
        boxed_sync(closure)
    }

    /// look up a node
    /// blocking
    async fn lookup_node(&self, _pk: TYPES::SignatureKey) -> Result<(), NetworkError> {
        Ok(())
    }

    async fn inject_consensus_info(&self, event: ConsensusIntentEvent) -> Result<(), NetworkError> {
        <WebServerNetwork<_, _, _, _,> as ConnectedNetwork<
            Message<TYPES, I>,
            TYPES::SignatureKey,
        >>::inject_consensus_info(&self.0, event)
        .await
    }
}

#[async_trait]
impl<
        M: NetworkMsg + 'static + ViewMessage<TYPES>,
        K: SignatureKey + 'static,
        E: ElectionConfig + 'static,
        TYPES: NodeType + 'static,
    > ConnectedNetwork<M, K> for WebServerNetwork<M, K, E, TYPES>
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
    fn shut_down<'a, 'b>(&'a self) -> BoxSyncFuture<'b, ()>
    where
        'a: 'b,
        Self: 'b,
    {
        let closure = async move {
            self.inner.running.store(false, Ordering::Relaxed);
        };
        boxed_sync(closure)
    }

    /// broadcast message to some subset of nodes
    /// blocking
    async fn broadcast_message(
        &self,
        message: M,
        _recipients: BTreeSet<K>,
    ) -> Result<(), NetworkError> {
        let network_msg = Self::parse_post_message(message);
        match network_msg {
            Ok(network_msg) => {
                // error!("network msg is {:?}", network_msg.clone());

                self.post_message_to_web_server(network_msg).await
            }
            Err(network_msg) => Err(NetworkError::WebServer {
                source: network_msg,
            }),
        }
    }

    /// Sends a direct message to a specific node
    /// blocking
    async fn direct_message(&self, message: M, _recipient: K) -> Result<(), NetworkError> {
        let network_msg = Self::parse_post_message(message);
        match network_msg {
            Ok(network_msg) => self.post_message_to_web_server(network_msg).await,
            Err(network_msg) => Err(NetworkError::WebServer {
                source: network_msg,
            }),
        }
    }

    /// Moves out the entire queue of received messages of 'transmit_type`
    ///
    /// Will unwrap the underlying `NetworkMessage`
    /// blocking
    fn recv_msgs<'a, 'b>(
        &'a self,
        transmit_type: TransmitType,
    ) -> BoxSyncFuture<'b, Result<Vec<M>, NetworkError>>
    where
        'a: 'b,
        Self: 'b,
    {
        let closure = async move {
            match transmit_type {
                TransmitType::Direct => {
                    let mut queue = self.inner.direct_poll_queue.write().await;
                    Ok(queue
                        .drain(..)
                        .collect::<Vec<_>>()
                        .iter()
                        .map(|x| x.get_message().unwrap())
                        .collect())
                }
                TransmitType::Broadcast => {
                    let mut queue = self.inner.broadcast_poll_queue.write().await;
                    Ok(queue
                        .drain(..)
                        .collect::<Vec<_>>()
                        .iter()
                        .map(|x| x.get_message().unwrap())
                        .collect())
                }
            }
        };
        boxed_sync(closure)
    }

    /// look up a node
    /// blocking
    async fn lookup_node(&self, _pk: K) -> Result<(), NetworkError> {
        Ok(())
    }

    async fn inject_consensus_info(&self, event: ConsensusIntentEvent) -> Result<(), NetworkError> {
        // let result = self.inner.consensus_intent_sender.send(event).await;
        error!("Injecting event: {:?}", event.clone());
      
        let result = match &event {
            ConsensusIntentEvent::PollForVote(_) => self.inner.vote_sender.send(event).await,
            ConsensusIntentEvent::PollForProposal(_) => self.inner.proposal_sender.send(event).await,
            ConsensusIntentEvent::PollForDAC(_) => todo!(),
            ConsensusIntentEvent::PollForViewSyncVotes(_) => todo!(),
            ConsensusIntentEvent::PollForViewSyncCertificate(_) => todo!(),
        };

        // // Should use a better error type here
        result.map_err(|_e| NetworkError::WebServer {
            source: WebServerNetworkError::ClientError,
        })
    }
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>>
    TestableNetworkingImplementation<TYPES, Message<TYPES, I>>
    for WebServerNetwork<Message<TYPES, I>, TYPES::SignatureKey, TYPES::ElectionConfigType, TYPES>
where
    TYPES::SignatureKey: TestableSignatureKey,
{
    fn generator(
        expected_node_count: usize,
        _num_bootstrap: usize,
        _network_id: usize,
        da_committee_size: usize,
        is_da: bool,
    ) -> Box<dyn Fn(u64) -> Self + 'static> {
        let (server_shutdown_sender, server_shutdown) = oneshot();
        let sender = Arc::new(server_shutdown_sender);
        // TODO ED Restrict this to be an open port using portpicker
        let port = random::<u16>();
        error!("Launching web server on port {port}");
        // Start web server
        async_spawn(hotshot_web_server::run_web_server::<TYPES::SignatureKey>(
            Some(server_shutdown),
            port,
        ));

        let known_nodes = (0..expected_node_count as u64)
            .map(|id| {
                TYPES::SignatureKey::from_private(&TYPES::SignatureKey::generate_test_key(id))
            })
            .collect::<Vec<_>>();

        let mut committee_nodes = known_nodes.clone();
        if is_da {
            committee_nodes.truncate(da_committee_size);
        }

        // Start each node's web server client
        Box::new(move |id| {
            let sender = Arc::clone(&sender);
            let mut network = WebServerNetwork::create(
                "0.0.0.0",
                port,
                Duration::from_millis(100),
                known_nodes[id as usize].clone(),
                committee_nodes.clone(),
                is_da,
            );
            network.server_shutdown_signal = Some(sender);
            network
        })
    }

    fn in_flight_message_count(&self) -> Option<usize> {
        None
    }
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES>,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        MEMBERSHIP: Membership<TYPES>,
    > TestableNetworkingImplementation<TYPES, Message<TYPES, I>>
    for WebCommChannel<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP>
where
    TYPES::SignatureKey: TestableSignatureKey,
{
    fn generator(
        expected_node_count: usize,
        num_bootstrap: usize,
        network_id: usize,
        da_committee_size: usize,
        is_da: bool,
    ) -> Box<dyn Fn(u64) -> Self + 'static> {
        let generator = <WebServerNetwork<
            Message<TYPES, I>,
            TYPES::SignatureKey,
            TYPES::ElectionConfigType,
            TYPES,
        > as TestableNetworkingImplementation<_, _>>::generator(
            expected_node_count,
            num_bootstrap,
            network_id,
            da_committee_size,
            is_da,
        );
        Box::new(move |node_id| Self(generator(node_id).into(), PhantomData))
    }

    fn in_flight_message_count(&self) -> Option<usize> {
        None
    }
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES>,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        MEMBERSHIP: Membership<TYPES>,
    >
    TestableChannelImplementation<
        TYPES,
        Message<TYPES, I>,
        PROPOSAL,
        VOTE,
        MEMBERSHIP,
        WebServerNetwork<Message<TYPES, I>, TYPES::SignatureKey, TYPES::ElectionConfigType, TYPES>,
    > for WebCommChannel<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP>
where
    TYPES::SignatureKey: TestableSignatureKey,
{
    fn generate_network() -> Box<
        dyn Fn(
                Arc<
                    WebServerNetwork<
                        Message<TYPES, I>,
                        TYPES::SignatureKey,
                        TYPES::ElectionConfigType,
                        TYPES,
                    >,
                >,
            ) -> Self
            + 'static,
    > {
        Box::new(move |network| WebCommChannel::new(network))
    }
}
