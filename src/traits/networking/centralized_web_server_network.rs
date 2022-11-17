use async_trait::async_trait;
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
use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

#[derive(Clone, Debug)]
struct CentralizedWebServerNetwork {}

#[async_trait]
impl<TYPES: NodeTypes> NetworkingImplementation<TYPES> for CentralizedWebServerNetwork {
    async fn ready(&self) -> bool {
        nll_todo()
    }
    async fn broadcast_message(&self, message: Message<TYPES>) -> Result<(), NetworkError> {
        nll_todo()
    }
    async fn message_node(
        &self,
        message: Message<TYPES>,
        recipient: TYPES::SignatureKey,
    ) -> Result<(), NetworkError> {
        nll_todo()
    }
    async fn broadcast_queue(&self) -> Result<Vec<Message<TYPES>>, NetworkError> {
        nll_todo()
    }

    async fn next_broadcast(&self) -> Result<Message<TYPES>, NetworkError> {
        nll_todo()
    }

    async fn direct_queue(&self) -> Result<Vec<Message<TYPES>>, NetworkError> {
        nll_todo()
    }

    async fn next_direct(&self) -> Result<Message<TYPES>, NetworkError> {
        nll_todo()
    }

    async fn known_nodes(&self) -> Vec<TYPES::SignatureKey> {
        nll_todo()
    }
    async fn network_changes(
        &self,
    ) -> Result<Vec<NetworkChange<TYPES::SignatureKey>>, NetworkError> {
        nll_todo()
    }

    async fn shut_down(&self) -> () {
        nll_todo()
    }

    async fn put_record(
        &self,
        key: impl Serialize + Send + Sync + 'static,
        value: impl Serialize + Send + Sync + 'static,
    ) -> Result<(), NetworkError> {
        nll_todo()
    }

    async fn get_record<V: for<'a> Deserialize<'a>>(
        &self,
        key: impl Serialize + Send + Sync + 'static,
    ) -> Result<V, NetworkError> {
        nll_todo()
    }
    async fn notify_of_subsequent_leader(
        &self,
        pk: TYPES::SignatureKey,
        cancelled: Arc<AtomicBool>,
    ) {
        nll_todo()
    }
}
