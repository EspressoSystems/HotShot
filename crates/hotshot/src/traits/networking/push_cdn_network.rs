use super::NetworkError;
use ark_serialize::CanonicalSerialize;
use async_trait::async_trait;

use async_compatibility_layer::art::async_spawn;
use async_compatibility_layer::channel::UnboundedSendError;
use bincode::Options;

use client::{Client, Config as ClientConfig};
use common::schema::Message;
use common::topic::Topic;
use hotshot_signature_key::bn254::{BLSPrivKey, BLSPubKey};
use hotshot_task::{boxed_sync, BoxSyncFuture};
use hotshot_types::traits::network::{ConnectedNetwork, NetworkMsg};
use hotshot_types::traits::signature_key::SignatureKey;
use hotshot_types::{
    data::ViewNumber,
    message::Message as HotShotMessage,
    traits::{
        network::{
            CommunicationChannel, ConsensusIntentEvent, TestableChannelImplementation,
            TestableNetworkingImplementation, TransmitType,
        },
        node_implementation::NodeType,
    },
};
use hotshot_utils::bincode::bincode_opts;
use server::{Config as ServerConfig, Server};
use std::any::TypeId;
use std::marker::PhantomData;
use std::{fmt::Debug, sync::Arc};
use tracing::warn;

/// A communication channel with 2 networks, where we can fall back to the slower network if the
/// primary fails
#[derive(Clone, Debug)]
pub struct PushCdnCommChannel<TYPES: NodeType>(
    Arc<PushCdnNetwork<HotShotMessage<TYPES>, TYPES::SignatureKey>>,
    PhantomData<TYPES>,
);

impl<TYPES: NodeType> PushCdnCommChannel<TYPES> {
    /// Constructor
    #[must_use]
    pub fn new(network: Arc<PushCdnNetwork<HotShotMessage<TYPES>, TYPES::SignatureKey>>) -> Self {
        Self(network, PhantomData)
    }
}

/// Wrapper for the tuple of `WebServerNetwork` and `Libp2pNetwork`
/// We need this so we can impl `TestableNetworkingImplementation`
/// on the tuple
#[derive(Clone)]
pub struct PushCdnNetwork<M: NetworkMsg, K: SignatureKey + 'static> {
    /// The underlying client
    pub client: Client,
    /// List of topics to initially subscribe to
    pub topics: Vec<Topic>,
    /// Phantom data
    pub _pd: PhantomData<(M, K)>,
}

impl<M: NetworkMsg, K: SignatureKey> Debug for PushCdnNetwork<M, K> {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

impl<TYPES: NodeType> TestableNetworkingImplementation<TYPES>
    for PushCdnNetwork<HotShotMessage<TYPES>, TYPES::SignatureKey>
{
    fn generator(
        expected_node_count: usize,
        _num_bootstrap: usize,
        _network_id: usize,
        da_committee_size: usize,
        is_da: bool,
    ) -> Box<dyn Fn(u64) -> Self + 'static> {
        assert!(
            da_committee_size <= expected_node_count,
            "DA committee size must be less than or equal to total # nodes"
        );

        // pick random, unused port
        let port = portpicker::pick_unused_port().expect("Could not find an open port");

        async_spawn(async move {
            let server = Server::new(ServerConfig {
                bind_address: format!("127.0.0.1:{port}"),
                advertise_address: "127.0.0.1:8080".to_string(),
                redis_url: "redis://127.0.0.1:8080".to_string(),

                cert_path: None,
                key_path: None,
            })
            .unwrap();

            server.run().await
        });

        Box::new({
            move |node_id| {
                let privkey =
                    TYPES::SignatureKey::generated_from_seed_indexed([0u8; 32], node_id).1;
                let pubkey = TYPES::SignatureKey::from_private(&privkey);

                let privkey: BLSPrivKey = unsafe { std::mem::transmute_copy(&privkey) };
                let pubkey: BLSPubKey = unsafe { std::mem::transmute_copy(&pubkey) };

                // TODO: this is garbage
                let topic = if is_da { Topic::DA } else { Topic::Global };

                let client = Client::new(ClientConfig {
                    broker_address: format!("127.0.0.1:{port}"),
                    initial_topics: vec![topic.clone()],
                    signing_key: privkey.priv_key,
                    verify_key: pubkey.pub_key,
                })
                .unwrap();

                PushCdnNetwork {
                    client,
                    topics: vec![topic],
                    _pd: PhantomData,
                }
            }
        })
    }

    /// Get the number of messages in-flight.
    ///
    /// Some implementations will not be able to tell how many messages there are in-flight. These implementations should return `None`.
    fn in_flight_message_count(&self) -> Option<usize> {
        None
    }
}

impl<TYPES: NodeType> TestableNetworkingImplementation<TYPES> for PushCdnCommChannel<TYPES> {
    fn generator(
        expected_node_count: usize,
        num_bootstrap: usize,
        network_id: usize,
        da_committee_size: usize,
        is_da: bool,
    ) -> Box<dyn Fn(u64) -> Self + 'static> {
        let generator = <PushCdnNetwork<
        HotShotMessage<TYPES>,
        TYPES::SignatureKey,
    > as TestableNetworkingImplementation<_>>::generator(
        expected_node_count,
        num_bootstrap,
        network_id,
        da_committee_size,
        is_da
    );
        Box::new(move |node_id| Self(generator(node_id).into(), PhantomData))
    }

    /// Get the number of messages in-flight.
    ///
    /// Some implementations will not be able to tell how many messages there are in-flight. These implementations should return `None`.
    fn in_flight_message_count(&self) -> Option<usize> {
        None
    }
}

#[async_trait]
impl<TYPES: NodeType> CommunicationChannel<TYPES> for PushCdnCommChannel<TYPES> {
    type NETWORK = PushCdnNetwork<HotShotMessage<TYPES>, TYPES::SignatureKey>;

    fn pause(&self) {
        unimplemented!("Pausing not implemented for push CDN network");
    }

    fn resume(&self) {
        unimplemented!("Resuming not implemented for push CDN network");
    }

    async fn wait_for_ready(&self) {
        self.0.wait_for_ready().await;
    }

    async fn is_ready(&self) -> bool {
        self.0.is_ready().await
    }

    fn shut_down<'a, 'b>(&'a self) -> BoxSyncFuture<'b, ()>
    where
        'a: 'b,
        Self: 'b,
    {
        let closure = async move {
            self.0.shut_down().await;
        };
        boxed_sync(closure)
    }

    async fn broadcast_message(&self, message: HotShotMessage<TYPES>) -> Result<(), NetworkError> {
        self.0.broadcast_message(message).await
    }

    async fn direct_message(
        &self,
        message: HotShotMessage<TYPES>,
        recipient: TYPES::SignatureKey,
    ) -> Result<(), NetworkError> {
        self.0.direct_message(message, recipient).await
    }

    fn recv_msgs<'a, 'b>(
        &'a self,
        transmit_type: TransmitType,
    ) -> BoxSyncFuture<'b, Result<Vec<HotShotMessage<TYPES>>, NetworkError>>
    where
        'a: 'b,
        Self: 'b,
    {
        let closure = async move { self.0.recv_msgs(transmit_type).await };
        boxed_sync(closure)
    }

    async fn queue_node_lookup(
        &self,
        _view_number: ViewNumber,
        _pk: TYPES::SignatureKey,
    ) -> Result<(), UnboundedSendError<Option<(ViewNumber, TYPES::SignatureKey)>>> {
        Ok(())
    }

    async fn inject_consensus_info(&self, event: ConsensusIntentEvent<TYPES::SignatureKey>) {
        self.0.inject_consensus_info(event).await;
    }
}

impl<TYPES: NodeType> TestableChannelImplementation<TYPES> for PushCdnCommChannel<TYPES> {
    fn generate_network() -> Box<dyn Fn(Arc<Self::NETWORK>) -> Self + 'static> {
        {
            Box::new(move |network| PushCdnCommChannel::new(network))
        }
    }
}

#[async_trait]
impl<M: NetworkMsg, K: SignatureKey + 'static> ConnectedNetwork<M, K> for PushCdnNetwork<M, K> {
    async fn wait_for_ready(&self) {
        self.client.wait_connect().await;
    }

    async fn is_ready(&self) -> bool {
        true
    }

    fn shut_down<'a, 'b>(&'a self) -> BoxSyncFuture<'b, ()>
    where
        'a: 'b,
        Self: 'b,
    {
        let closure = async move {};
        boxed_sync(closure)
    }

    async fn broadcast_message(&self, message: M) -> Result<(), NetworkError> {
        // bincode message
        let serialized_message = match bincode_opts().serialize(&message) {
            Ok(serialized) => serialized,
            Err(e) => {
                warn!("Failed to serialize message: {}", e);
                return Err(NetworkError::SerializationError { source: e });
            }
        };

        self.client
            .send_message(Arc::new(Message::Broadcast(
                self.topics.clone(),
                serialized_message,
            )))
            .await;

        Ok(())
    }

    async fn direct_message(&self, message: M, recipient: K) -> Result<(), NetworkError> {
        // bincode message
        let serialized_message = match bincode_opts().serialize(&message) {
            Ok(serialized) => serialized,
            Err(e) => {
                warn!("Failed to serialize message: {}", e);
                return Err(NetworkError::SerializationError { source: e });
            }
        };

        if TypeId::of::<K>() == TypeId::of::<BLSPubKey>() {
            let recipient: BLSPubKey = unsafe { std::mem::transmute_copy(&recipient) };
            // serialize public key
            let mut recipient_buf = Vec::new();
            recipient
                .pub_key
                .serialize_uncompressed(&mut recipient_buf)
                .unwrap();

            self.client
                .send_message(Arc::new(Message::Direct(recipient_buf, serialized_message)))
                .await;

            Ok(())
        } else {
            return Err(NetworkError::CouldNotDeliver);
        }
    }

    fn recv_msgs<'a, 'b>(
        &'a self,
        _transmit_type: TransmitType,
    ) -> BoxSyncFuture<'b, Result<Vec<M>, NetworkError>>
    where
        'a: 'b,
        Self: 'b,
    {
        let closure = async move {
            let message = self.client.recv_message().await;
            if let Message::Data(data) = message {
                let result: Result<M, _> = bincode_opts()
                    .deserialize(&data)
                    .map_err(|e| NetworkError::SerializationError { source: e });
                return Ok(vec![result?]);
            }
            Ok(vec![])
        };

        boxed_sync(closure)
    }

    async fn queue_node_lookup(
        &self,
        _view_number: ViewNumber,
        _pk: K,
    ) -> Result<(), UnboundedSendError<Option<(ViewNumber, K)>>> {
        Ok(())
    }

    async fn inject_consensus_info(&self, _event: ConsensusIntentEvent<K>) {}
}
