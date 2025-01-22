use std::{marker::PhantomData, time::Duration};

use async_trait::async_trait;
use hotshot_types::traits::{network::ConnectedNetwork, signature_key::SignatureKey};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::sync::mpsc;

pub trait Validatable {
    /// Validate the message as:
    /// - Satisfying the request (maybe something like a signature check)
    /// - Satisfying the response (is the correct data)
    fn is_valid(&self) -> bool;
}

/// A trait alias to help with readability
pub trait Request: Serialize + DeserializeOwned + Send + Sync + Validatable {}
pub trait Response: Serialize + DeserializeOwned + Send + Sync + Validatable {}

/// The message type for the request-response protocol
#[derive(Serialize, Deserialize)]
pub enum Message<Q, A> {
    /// A request message
    Request(Q),
    /// A response message
    Response(A),
}

/// The [`Sender`] trait is used to allow the [`RequestResponseProtocol`] to send messages to a specific recipient
/// and to get all relevant recipients for a specific message.
#[async_trait]
pub trait Sender<K: SignatureKey + 'static> {
    /// Send a message to a specified recipient
    async fn send_message<Q: Request, A: Response>(
        &self,
        message: &Message<Q, A>,
        recipient: K,
    ) -> anyhow::Result<()>;
}

/// The [`Receiver`] trait is used to allow the [`RequestResponseProtocol`] to receive messages from a network
/// or other source.
#[async_trait]
pub trait Receiver {
    /// Receive a message
    async fn receive_message<Q: Request, A: Response>(&mut self) -> anyhow::Result<Message<Q, A>>;
}

/// A trait that allows the [`RequestResponseProtocol`] to get the recipients that a specific message should
/// expect responses from
pub trait RecipientSource<K: SignatureKey + 'static> {
    /// Get all the recipients that the specific message should expect responses from
    fn get_recipients_for<Q: Request, A: Response>(&self, message: &Message<Q, A>) -> Vec<K>;
}

/// A blanket implementation of the [`Sender`] trait for all types that implement [`ConnectedNetwork`]
#[async_trait]
impl<N: ConnectedNetwork<K>, K: SignatureKey + 'static> Sender<K> for N {
    async fn send_message<Q: Request, A: Response>(
        &self,
        message: &Message<Q, A>,
        recipient: K,
    ) -> anyhow::Result<()> {
        // Serialize the message
        let serialized_message = bincode::serialize(message)?;

        // Just send the message to the recipient
        self.direct_message(serialized_message, recipient)
            .await
            .map_err(anyhow::Error::from)
    }
}

/// An implementation of the [`Receiver`] trait for the [`mpsc::Receiver`] type. Allows us to send messages
/// to a channel and have the protocol receive them.
#[async_trait]
impl Receiver for mpsc::Receiver<Vec<u8>> {
    async fn receive_message<Q: Request, A: Response>(&mut self) -> anyhow::Result<Message<Q, A>> {
        // Receive a message from the channel
        let message = self.recv().await.ok_or(anyhow::anyhow!("channel closed"))?;

        // Deserialize the message
        bincode::deserialize(&message).map_err(anyhow::Error::from)
    }
}

/// The request-response configuration
#[derive(Clone)]
pub struct RequestResponseConfig {}

/// A protocol that allows for request-response communication
#[derive(Clone)]
pub struct RequestResponse<
    S: Sender<K>,
    R: Receiver,
    RS: RecipientSource<K>,
    K: SignatureKey + 'static,
> {
    /// The configuration of the protocol
    config: RequestResponseConfig,
    /// The sender to use for the protocol
    sender: S,
    /// The receiver to use for the protocol
    receiver: R,
    /// The recipient source to use for the protocol
    recipient_source: RS,
    /// Phantom data to help with type inference
    phantom_data: PhantomData<K>,
}

impl<S: Sender<K>, R: Receiver, RS: RecipientSource<K>, K: SignatureKey + 'static>
    RequestResponse<S, R, RS, K>
{
    /// Create a new [`RequestResponseProtocol`]
    ///
    /// # Arguments
    /// * `config` - The configuration for the protocol
    /// * `sender` - The sender to use for the protocol
    /// * `receiver` - The receiver to use for the protocol
    /// * `recipient_source` - The recipient source to use for the protocol
    pub fn new(
        config: RequestResponseConfig,
        sender: S,
        receiver: R,
        recipient_source: RS,
    ) -> Self {
        Self {
            config,
            sender,
            receiver,
            recipient_source,
            phantom_data: PhantomData,
        }
    }

    /// Request something from the protocol and wait for the response
    pub async fn request<Q: Request, A: Response>(
        &self,
        request: Q,
        timeout: Duration,
    ) -> anyhow::Result<A> {
        // Create a request message
        let message: Message<Q, A> = Message::Request(request);

        // Get the recipients that the message should expect responses from
        let recipients = self.recipient_source.get_recipients_for(&message);

        todo!()
    }
}
