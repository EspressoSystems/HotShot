use async_compatibility_layer::{
    art::{async_block_on, async_sleep, async_spawn},
    channel::{oneshot, OneShotReceiver, OneShotSender},
};
use async_lock::{RwLock, RwLockUpgradableReadGuard};
use async_trait::async_trait;
use bincode::Options;
use hotshot_centralized_web_server;
use hotshot_types::data::ViewNumber;
use hotshot_types::traits::state::ConsensusTime;
use hotshot_types::{
    message::Message,
    traits::{
        metrics::{Metrics, NoMetrics},
        network::{
            FailedToDeserializeSnafu, FailedToSerializeSnafu, NetworkChange, NetworkError,
            NetworkingImplementation, TestableNetworkingImplementation,
        },
        node_implementation::NodeTypes,
        signature_key::{ed25519::Ed25519Pub, SignatureKey, TestableSignatureKey},
    },
};
use hotshot_utils::bincode::bincode_opts;
use nll::nll_todo::nll_todo;

use serde::Deserialize;
use serde::Serialize;
use snafu::ResultExt;
use std::marker::PhantomData;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use surf_disco::error;
use surf_disco::error::ClientError;
use tide_disco::error::ServerError;
use tracing::error;

#[derive(Clone, Debug)]
pub struct CentralizedWebServerNetwork<TYPES: NodeTypes> {
    /// The inner state
    inner: Arc<Inner<TYPES>>,
    /// An optional shutdown signal. This is only used when this connection is created through the `TestableNetworkingImplementation` API.
    server_shutdown_signal: Option<Arc<OneShotSender<()>>>,
}

impl<TYPES: NodeTypes> CentralizedWebServerNetwork<TYPES> {
    fn new() -> Self {
        //KALEY: maybe new and create should be the same- in the centralized_server_network.rs file,
        //it's called create() but I think new() makes more sense here. Will change next
        nll_todo()
    }
    fn create() -> Self {
        let port = 8000 as u16;
        let base_url = format!("0.0.0.0:{port}");
        let base_url = format!("http://{base_url}").parse().unwrap();
        let client = surf_disco::Client::<ServerError>::new(base_url);

        let inner = Arc::new(Inner {
            phantom: Default::default(),
            //KALEY todo: init view number? get from server?
            view_number: RwLock::from(TYPES::Time::new(0)),
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
    // Temporary for the TYPES argument
    phantom: PhantomData<TYPES>,

    // Current view number so we can poll accordingly
    view_number: RwLock<TYPES::Time>,

    // Queue for broadcasted messages (mainly transactions and proposals)
    broadcast_poll_queue: RwLock<Vec<Message<TYPES>>>,
    // Queue for direct messages (mainly votes)
    direct_poll_queue: RwLock<Vec<Message<TYPES>>>,
    //KALEY: these may not be necessary
    running: AtomicBool,
    connected: AtomicBool,
    client: surf_disco::Client<ServerError>,
}

// TODO add async task that continually polls for transactions, votes, and proposals.  Will
// need to inject the view number into this async task somehow.  This async task can put the
// message it receives into either a `broadcast_queue` or `direct_queue` so that the interace
// is the same as the other networking impls.  Will also need to implement some message
// wrapper similar to the other centralized server network that allows the web server
// to differentiate transactions from proposals.

async fn run_background_receive<TYPES: NodeTypes>(
    connection: Arc<Inner<TYPES>>,
) -> Result<(), ServerError> {

    // TODO ED: separate this polling so it is not linear, 
    // poll future views

    println!("Run background receive task has started!");
    // Just poll current view for now:
    loop {
        let view_number = connection.view_number.read().await;

        // TODO ED: Actually track transaction index 
        let possible_transactions: Result<Option<Vec<Vec<u8>>>, ClientError> = connection
        .client
        .get(&format!("/api/transactions/0"))
        .send()
        .await;

        match possible_transactions {
            Err(error) => error!("Transaction error is: {:?}", error),
            Ok(Some(transactions)) => {
                println!("Found a tx!");
                let mut lock = connection.broadcast_poll_queue.write().await;
                transactions.iter().for_each(|tx| {
                    let deserialized_tx =
                        bincode::deserialize::<Message<TYPES>>(tx).unwrap();
                    // println!("prop is {:?}", deserialized_proposal.kind);
                    // WHY causing issues?
                    lock.push(deserialized_tx.clone());
                });

            }, 
            Ok(None) => {println!("No transactions")},
        }

        let possible_proposal: Result<Option<Vec<Vec<u8>>>, ClientError> = connection
            .client
            .get(&format!("/api/proposal/{}", view_number.to_string()))
            .send()
            .await;

        // TODO ED stop polling once we have a proposal
        match possible_proposal {
            // TODO ED: differentiate between different errors, some errors mean there is nothing in the queue,
            // others could mean an actual error; perhaps nothing should return None instead of an error
            Err(error) => panic!("Proposal error is: {:?}", error),
            Ok(Some(proposals)) => {
                // println!("{:?}", proposals);
                // Add proposal to broadcast queue
                let mut lock = connection.broadcast_poll_queue.write().await;
                proposals.iter().for_each(|proposal| {
                    let deserialized_proposal =
                        bincode::deserialize::<Message<TYPES>>(proposal).unwrap();
                    // println!("prop is {:?}", deserialized_proposal.kind);
                    // WHY causing issues?
                    lock.push(deserialized_proposal.clone());
                });
            }
            Ok(None) => println!("Proposal is None"),
        }

        let possible_vote: Result<Option<Vec<Vec<u8>>>, ClientError> = connection
            .client
            .get(&format!("/api/votes/{}", view_number.to_string()))
            .send()
            .await;

        // TODO ED stop polling once we have a proposal
        match possible_vote {
            // TODO ED: differentiate between different errors, some errors mean there is nothing in the queue,
            // others could mean an actual error; perhaps nothing should return None instead of an error
            Err(error) => panic!("Vote error is: {:?}", error),
            Ok(Some(votes)) => {
                // println!("{:?}", proposals);
                // Add proposal to broadcast queue
                let mut lock = connection.direct_poll_queue.write().await;
                votes.iter().for_each(|vote| {
                    let deserialized_vote = bincode::deserialize::<Message<TYPES>>(vote).unwrap();
                    println!("voteis {:?}", deserialized_vote.kind);
                    // WHY causing issues?
                    lock.push(deserialized_vote.clone());
                });
                // Shouldn't need this:
                drop(lock);
            }

            Ok(None) => println!("vote is None"),
        }



        // TODO ED: Adjust this parameter once things are working
        async_sleep(Duration::from_millis(300)).await;
    }
    // TODO ED: propogate errors from above once we change the way empty responses are received
    Ok(())
}

#[async_trait]
impl<TYPES: NodeTypes> NetworkingImplementation<TYPES> for CentralizedWebServerNetwork<TYPES> {
    // TODO Start up async task, ensure we can reach the centralized server
    async fn ready(&self) -> bool {
        while !self.inner.connected.load(Ordering::Relaxed) {
            println!("Sleeping waiting to be ready");
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
                println!("Sent proposal is: {:?}", sent_proposal);
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
                println!("Data message being broadcast {:?}", sent_tx);
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
                // println!("Consensus Message Is: {:?}", message.clone().kind);
                let sent_vote: Result<(), ServerError> = self
                    .inner
                    .client
                    .post(&format!("/api/votes/{}", m.view_number().to_string()))
                    .body_binary(&message)
                    .unwrap()
                    .send()
                    .await;
                println!("sent vote is: {:?}", sent_vote);
            }
            hotshot_types::message::MessageKind::Data(_) => {
                println!("Data message being sent directly")
            }
        }
        Ok(())
    }

    // TODO Read from the queue that the async task dumps everything into
    // For now that task can dump transactions and proposals into the same queue
    async fn broadcast_queue(&self) -> Result<Vec<Message<TYPES>>, NetworkError> {
        let mut queue = self.inner.broadcast_poll_queue.write().await;
        println!("Q len is {}", queue.len());
        let messages = queue.drain(..).collect();
        println!("message are {:?}", messages);
        Ok(messages)
        // nll_todo()
    }

    // TODO Get the next message from the broadcast queue
    async fn next_broadcast(&self) -> Result<Message<TYPES>, NetworkError> {
        nll_todo()
    }

    // TODO implemented the same as the broadcast queue
    async fn direct_queue(&self) -> Result<Vec<Message<TYPES>>, NetworkError> {
        // println!("BEFORE DRAIN DIRECT QUEUE");
        let mut queue = self.inner.direct_poll_queue.write().await;
        // println!("MIDDLE DRAIN DIRECT QUEUE");

        let messages = queue.drain(..).collect();
        // println!("AFTER DRAIN DIRECT QUEUE");

        Ok(messages)
    }

    // TODO implemented the same as the broadcast queue
    async fn next_direct(&self) -> Result<Message<TYPES>, NetworkError> {
        nll_todo()
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
        nll_todo()
    }

    async fn inject_view_number(&self, view_number: TYPES::Time) {
        println!(
            "Inject view number called with view: {:?}",
            view_number.clone()
        );
        let old_view = self.inner.view_number.upgradable_read().await;
        if *old_view < view_number {
            let mut new_view = RwLockUpgradableReadGuard::upgrade(old_view).await;
            *new_view = view_number;
            println!("New inject view number is: {:?}", new_view);
        }
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
