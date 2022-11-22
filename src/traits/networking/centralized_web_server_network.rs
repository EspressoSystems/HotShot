use async_lock::RwLock;
use async_trait::async_trait;
use hotshot_types::data::ViewNumber;
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
use hotshot_utils::hack::nll_todo;
use serde::Deserialize;
use serde::Serialize;
use std::marker::PhantomData;
use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct CentralizedWebServerNetwork<TYPES: NodeTypes> {
    inner: Arc<Inner<TYPES>>,
}

#[derive(Debug)]
struct Inner<TYPES: NodeTypes> {

    // Temporary for the TYPES argument
    phantom: PhantomData<TYPES>,

    // Current view number so we can poll accordingly
    view_number: RwLock<ViewNumber>,

    // Queue for broadcasted messages (mainly transactions and proposals)
    broadcast_poll_queue: RwLock<Vec<u8>>,
    // Queue for direct messages (mainly votes)
    direct_poll_queue: RwLock<Vec<u8>>
}

// TODO add async task that continually polls for transactions, votes, and proposals.  Will
// need to inject the view number into this async task somehow.  This async task can put the
// message it receives into either a `broadcast_queue` or `direct_queue` so that the interace
// is the same as the other networking impls.  Will also need to implement some message
// wrapper similar to the other centralized server network that allows the web server
// to differentiate transactions from proposals.

#[async_trait]
impl<TYPES: NodeTypes> NetworkingImplementation<TYPES> for CentralizedWebServerNetwork<TYPES> {
    // TODO Start up async task, ensure we can reach the centralized server
    async fn ready(&self) -> bool {
        nll_todo()
    }

    // TODO send message to the centralized server
    // Will need some way for centralized server to distinguish between propsoals and transactions,
    // since it treats those differently
    async fn broadcast_message(&self, message: Message<TYPES>) -> Result<(), NetworkError> {
        nll_todo()
    }

    // TODO send message to centralized server (this should only be Vote/Timeout messages for now,
    // but in the future we'll need to handle other messages)
    async fn message_node(
        &self,
        message: Message<TYPES>,
        recipient: TYPES::SignatureKey,
    ) -> Result<(), NetworkError> {
        nll_todo()
    }

    // TODO Read from the queue that the async task dumps everything into
    // For now that task can dump transactions and proposals into the same queue
    async fn broadcast_queue(&self) -> Result<Vec<Message<TYPES>>, NetworkError> {
        nll_todo()
    }

    // TODO Get the next message from the broadcast queue
    async fn next_broadcast(&self) -> Result<Message<TYPES>, NetworkError> {
        nll_todo()
    }

    // TODO implemented the same as the broadcast queue
    async fn direct_queue(&self) -> Result<Vec<Message<TYPES>>, NetworkError> {
        nll_todo()
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
        nll_todo()
    }

    // TODO stop async background task
    async fn shut_down(&self) -> () {
        nll_todo()
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
        nll_todo()
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
        nll_todo()
    }

    // TODO Can be a no-op most likely
    fn in_flight_message_count(&self) -> Option<usize> {
        nll_todo()
    }
}
