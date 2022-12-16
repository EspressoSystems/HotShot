use async_compatibility_layer::{
    art::{async_sleep, async_spawn},
    async_primitives::subscribable_rwlock::{ReadView, SubscribableRwLock},
    channel::{oneshot, OneShotSender},
};
use async_lock::RwLock;
use async_trait::async_trait;

use hotshot_centralized_web_server::{self, config};
use hotshot_types::traits::state::ConsensusTime;
use hotshot_types::{
    message::Message,
    traits::{
        network::{
            NetworkChange, NetworkError, NetworkingImplementation, TestableNetworkingImplementation,
        },
        node_implementation::NodeTypes,
        signature_key::TestableSignatureKey,
    },
};

use nll::nll_todo::nll_todo;

use serde::Deserialize;
use serde::Serialize;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use surf_disco::error::ClientError;
use tide_disco::error::ServerError;
use tracing::{error, info};

#[derive(Clone, Debug)]
pub struct CentralizedWebServerNetwork<TYPES: NodeTypes> {
    /// The inner state
    inner: Arc<Inner<TYPES>>,
    /// An optional shutdown signal. This is only used when this connection is created through the `TestableNetworkingImplementation` API.
    server_shutdown_signal: Option<Arc<OneShotSender<()>>>,
}

impl<TYPES: NodeTypes> CentralizedWebServerNetwork<TYPES> {
    fn create() -> Self {
        let port = config::WEB_SERVER_PORT;
        // TODO add URL is param
        let base_url = format!("0.0.0.0:{port}");
        let base_url = format!("http://{base_url}").parse().unwrap();
        let client = surf_disco::Client::<ServerError>::new(base_url);

        let inner = Arc::new(Inner {
            //KALEY todo: init view number? get from server?
            view_number: Arc::new(SubscribableRwLock::new(TYPES::Time::new(0))),
            broadcast_poll_queue: Default::default(),
            direct_poll_queue: Default::default(),
            running: AtomicBool::new(true),
            connected: AtomicBool::new(false),
            client,
        });
        inner.connected.store(true, Ordering::Relaxed);

        async_spawn({
            let inner = Arc::clone(&inner);
            async move {
                while inner.running.load(Ordering::Relaxed) {
                    if let Err(e) = run_background_receive(Arc::clone(&inner)).await {
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
}

#[derive(Debug)]
struct Inner<TYPES: NodeTypes> {
    // Current view number so we can poll accordingly
    view_number: Arc<SubscribableRwLock<TYPES::Time>>,

    // Queue for broadcasted messages (mainly transactions and proposals)
    broadcast_poll_queue: Arc<RwLock<Vec<Message<TYPES>>>>,
    // Queue for direct messages (mainly votes)
    direct_poll_queue: Arc<RwLock<Vec<Message<TYPES>>>>,
    //KALEY: these may not be necessary
    running: AtomicBool,
    connected: AtomicBool,
    client: surf_disco::Client<ServerError>,
}

async fn poll_web_server<TYPES: NodeTypes>(
    endpoint: String,
    connection: Arc<Inner<TYPES>>,
) -> Result<Option<Vec<Message<TYPES>>>, ClientError> {
    let result: Result<Option<Vec<Vec<u8>>>, ClientError> =
        connection.client.get(&endpoint).send().await;

    match result {
        // TODO ED: change this to be more Rust-like?
        Err(error) => Err(error),
        Ok(Some(messages)) => {
            let mut deserialized_messages = Vec::new();
            messages.iter().for_each(|message| {
                let deserialized_message = bincode::deserialize::<Message<TYPES>>(message).unwrap();
                deserialized_messages.push(deserialized_message);
            });
            Ok(Some(deserialized_messages))
        }
        Ok(None) => Ok(None),
    }
}

async fn poll_generic<TYPES: NodeTypes>(
    connection: Arc<Inner<TYPES>>,
    kind: MessageType,
    wait_between_polls: Duration,
    num_views_ahead: u64,
) -> Result<(), ServerError> {
    let mut tx_index: u128 = 0;

    let receiver = connection.view_number.subscribe().await;
    let mut view_number = connection.view_number.copied().await;
    view_number += num_views_ahead;

    let mut vote_index: u128 = 0;

    loop {
        let mut endpoint = String::new();
        // TODO ED probably want to export these endpoints from the web server so we aren't hardcoding them here
        match kind {
            MessageType::Proposal => {
                endpoint = config::get_proposal_route((*view_number).into());
            }
            MessageType::VoteTimedOut => {
                endpoint = config::get_vote_route((*view_number).into(), vote_index);
            }
            MessageType::Transaction => {
                endpoint = config::get_transactions_route(tx_index);
            }
            _ => nll_todo(),
        }
        // TODO ED: Only poll for votes if we are the leader
        let possible_message = poll_web_server(endpoint.clone(), connection.clone()).await;
        match possible_message {
            Err(error) => {
                error!("{:?}", error);
                async_sleep(wait_between_polls).await;
            }
            Ok(Some(deserialized_messages)) => match kind {
                MessageType::Proposal => {
                    // Only pushing the first proposal here since we will soon only be allowing 1 proposal per view
                    connection
                        .broadcast_poll_queue
                        .write()
                        .await
                        .push(deserialized_messages[0].clone());
                    view_number = receiver.recv().await.unwrap();
                }
                MessageType::VoteTimedOut => {
                    let mut direct_poll_queue = connection.direct_poll_queue.write().await;
                    deserialized_messages.iter().for_each(|vote| {
                        vote_index += 1;
                        direct_poll_queue.push(vote.clone());
                    });
                }
                MessageType::Transaction => {
                    let mut lock = connection.broadcast_poll_queue.write().await;
                    deserialized_messages.iter().for_each(|tx| {
                        tx_index += 1;
                        lock.push(tx.clone());
                    });
                }
                _ => nll_todo(),
            },
            Ok(None) => {
                // TODO ED: Worth it to use recv_timeout?
                async_sleep(wait_between_polls).await;
            }
        }

        // It seems better to only update the view number if it changes,
        // rather than reading the view each loop
        let new_view_number = receiver.try_recv();
        if new_view_number.is_ok() {
            view_number = new_view_number.unwrap();
            view_number += num_views_ahead;
            let vote_index = 0;
        }
    }

    Ok(())
}

enum MessageType {
    Transaction,
    VoteTimedOut,
    Proposal,
}

async fn run_background_receive<TYPES: NodeTypes>(
    connection: Arc<Inner<TYPES>>,
) -> Result<(), ServerError> {
    error!("Run background receive task has started!");
    let wait_between_polls = Duration::from_millis(500);

    let proposal_handle = async_spawn({
        let connection_ref = connection.clone();
        async move { poll_generic(connection_ref, MessageType::Proposal, wait_between_polls, 0).await }
    });
    let vote_handle = async_spawn({
        let connection_ref = connection.clone();
        async move {
            poll_generic(
                connection_ref,
                MessageType::VoteTimedOut,
                wait_between_polls,
                0,
            )
            .await
        }
    });
    let transaction_handle = async_spawn({
        let connection_ref = connection.clone();
        async move {
            poll_generic(
                connection_ref,
                MessageType::Transaction,
                wait_between_polls,
                0,
            )
            .await
        }
    });

    let proposal_handle_plus_one = async_spawn({
        let connection_ref = connection.clone();
        async move {
            poll_generic(
                connection_ref,
                MessageType::Proposal,
                wait_between_polls * 2,
                1,
            )
            .await
        }
    });

    // await them all:
    let mut task_handles = Vec::new();
    task_handles.push(proposal_handle);
    task_handles.push(vote_handle);
    task_handles.push(transaction_handle);
    task_handles.push(proposal_handle_plus_one);

    let children_finished = futures::future::join_all(task_handles);
    let result = children_finished.await;

    Ok(())
}

#[async_trait]
impl<TYPES: NodeTypes> NetworkingImplementation<TYPES> for CentralizedWebServerNetwork<TYPES> {
    // TODO Start up async task, ensure we can reach the centralized server
    async fn ready(&self) -> bool {
        while !self.inner.connected.load(Ordering::Relaxed) {
            info!("Sleeping waiting to be ready");
            async_sleep(Duration::from_secs(1)).await;
        }
        true
    }

    // TODO send message to the centralized server
    // Will need some way for centralized server to distinguish between propsoals and transactions,
    // since it treats those differently
    async fn broadcast_message(&self, message: Message<TYPES>) -> Result<(), NetworkError> {
        match message.clone().kind {
            hotshot_types::message::MessageKind::Consensus(m) => {
                let sent_proposal: Result<(), ServerError> = self
                    .inner
                    .client
                    .post(&format!("/api/proposal/{}", m.view_number().to_string()))
                    .body_binary(&message)
                    .unwrap()
                    .send()
                    .await;
            }
            // Most likely a transaction being broadcast
            hotshot_types::message::MessageKind::Data(_) => {
                let sent_tx: Result<(), ServerError> = self
                    .inner
                    .client
                    .post(&format!("/api/transactions"))
                    .body_binary(&message)
                    .unwrap()
                    .send()
                    .await;
            }
        }
        // TODO: put in our own broadcast queue

        Ok(())
    }

    // TODO send message to centralized server (this should only be Vote/Timeout messages for now,
    // but in the future we'll need to handle other messages)
    async fn message_node(
        &self,
        message: Message<TYPES>,
        recipient: TYPES::SignatureKey,
    ) -> Result<(), NetworkError> {
        match message.clone().kind {
            // Vote or timeout message
            hotshot_types::message::MessageKind::Consensus(m) => {
                let sent_vote: Result<(), ServerError> = self
                    .inner
                    .client
                    .post(&format!("/api/votes/{}", m.view_number().to_string()))
                    .body_binary(&message)
                    .unwrap()
                    .send()
                    .await;
            }
            hotshot_types::message::MessageKind::Data(_) => {}
        }
        Ok(())
    }

    // TODO Read from the queue that the async task dumps everything into
    // For now that task can dump transactions and proposals into the same queue
    async fn broadcast_queue(&self) -> Result<Vec<Message<TYPES>>, NetworkError> {
        let mut queue = self.inner.broadcast_poll_queue.write().await;
        let messages = queue.drain(..).collect();
        Ok(messages)
    }

    // TODO Get the next message from the broadcast queue
    async fn next_broadcast(&self) -> Result<Message<TYPES>, NetworkError> {
        let mut queue = self.inner.broadcast_poll_queue.write().await;
        let message = queue.remove(0);
        Ok(message)
    }

    // TODO implemented the same as the broadcast queue
    async fn direct_queue(&self) -> Result<Vec<Message<TYPES>>, NetworkError> {
        let mut queue = self.inner.direct_poll_queue.write().await;
        let messages = queue.drain(..).collect();
        Ok(messages)
    }

    // TODO implemented the same as the broadcast queue
    async fn next_direct(&self) -> Result<Message<TYPES>, NetworkError> {
        let mut queue = self.inner.direct_poll_queue.write().await;
        let message = queue.remove(0);
        Ok(message)
    }

    // TODO Need to see if this is used anywhere, otherwise can be a no-op
    async fn known_nodes(&self) -> Vec<TYPES::SignatureKey> {
        nll_todo()
    }

    // TODO can likely be a no-op, I don't think we ever use this
    async fn network_changes(
        &self,
    ) -> Result<Vec<NetworkChange<TYPES::SignatureKey>>, NetworkError> {
        Ok(Vec::new())
    }

    // TODO stop async background task
    async fn shut_down(&self) -> () {
        self.inner.running.store(false, Ordering::Relaxed);
    }

    // TODO can return an Error like the other centralized server impl
    async fn put_record(
        &self,
        key: impl Serialize + Send + Sync + 'static,
        value: impl Serialize + Send + Sync + 'static,
    ) -> Result<(), NetworkError> {
        nll_todo()
    }

    // TODO can return an Error like the other centralized server impl
    async fn get_record<V: for<'a> Deserialize<'a>>(
        &self,
        key: impl Serialize + Send + Sync + 'static,
    ) -> Result<V, NetworkError> {
        nll_todo()
    }

    // TODO No-op, only needed for libp2p
    async fn notify_of_subsequent_leader(
        &self,
        pk: TYPES::SignatureKey,
        cancelled: Arc<AtomicBool>,
    ) {
        // nll_todo()
    }

    async fn inject_view_number(&self, view_number: TYPES::Time) {
        info!("Injecting {:?}", view_number.clone());
        self.inner
            .view_number
            .modify(|current_view_number| {
                *current_view_number = view_number;
            })
            .await;
    }
}

impl<TYPES: NodeTypes> TestableNetworkingImplementation<TYPES>
    for CentralizedWebServerNetwork<TYPES>
where
    TYPES::SignatureKey: TestableSignatureKey,
{
    // TODO Can do something similar to other centralized server impl
    fn generator(
        expected_node_count: usize,
        num_bootstrap: usize,
    ) -> Box<dyn Fn(u64) -> Self + 'static> {
        let (server_shutdown_sender, server_shutdown) = oneshot();
        let sender = Arc::new(server_shutdown_sender);
        // Start web server
        async_spawn(hotshot_centralized_web_server::run_web_server(Some(
            server_shutdown,
        )));

        // Start each node's web server client
        Box::new(move |id| {
            let sender = Arc::clone(&sender);
            let mut network = CentralizedWebServerNetwork::create();
            network.server_shutdown_signal = Some(sender);
            network
        })
    }

    // TODO Can be a no-op most likely
    fn in_flight_message_count(&self) -> Option<usize> {
        nll_todo()
    }
}
