use std::{marker::PhantomData, time::Duration};

use anyhow::{Context, Error, Result};
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

pub trait Serializable: Sized {
    /// Serialize the type to a byte array
    fn to_bytes(&self) -> Result<Vec<u8>>;

    /// Deserialize the type from a byte array
    fn from_bytes(bytes: &[u8]) -> Result<Self>;
}

/// A trait for a request. Associates itself with a response type.
pub trait Request: Send + Sync + Serializable + Validatable {
    /// The response type associated with this request
    type Response: Response;
}

/// A trait that a response needs to implement
pub trait Response: Send + Sync + Serializable + Validatable {}

/// The message type for the request-response protocol
pub enum Message<R: Request> {
    /// A request message
    Request(R),
    /// A response message
    Response(R::Response),
}

/// A blanket implementation of the [`Serializable`] trait for any [`Message`]
impl<R: Request> Serializable for Message<R> {
    /// Convert any [`Message`] to bytes
    fn to_bytes(&self) -> Result<Vec<u8>> {
        // Convert the message to bytes based on the type. By default it is just type-prefixed
        let bytes = match self {
            Message::Request(request) => [&[0], request.to_bytes()?.as_slice()].concat(),
            Message::Response(response) => [&[1], response.to_bytes()?.as_slice()].concat(),
        };

        Ok(bytes)
    }

    /// Convert bytes to a [`Message`]
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        // Get the message type
        let message_type = bytes
            .get(0)
            .ok_or(anyhow::anyhow!("message type not found"))?;

        // Deserialize the message based on the type
        match message_type {
            0 => {
                let request_bytes = bytes
                    .get(1..)
                    .ok_or(anyhow::anyhow!("request bytes not found"))?;
                Ok(Message::Request(R::from_bytes(request_bytes)?))
            }
            1 => {
                let response_bytes = bytes
                    .get(1..)
                    .ok_or(anyhow::anyhow!("response bytes not found"))?;
                Ok(Message::Response(R::Response::from_bytes(response_bytes)?))
            }
            _ => Err(anyhow::anyhow!("invalid message type")),
        }
    }
}

/// The [`Sender`] trait is used to allow the [`RequestResponseProtocol`] to send messages to a specific recipient
/// and to get all relevant recipients for a specific message.
#[async_trait]
pub trait Sender<K: SignatureKey + 'static> {
    /// Send a message to a specified recipient
    async fn send_message<R: Request>(&self, message: &Message<R>, recipient: K) -> Result<()>;
}

/// The [`Receiver`] trait is used to allow the [`RequestResponseProtocol`] to receive messages from a network
/// or other source.
#[async_trait]
pub trait Receiver {
    /// Receive a message
    async fn receive_message<R: Request>(&mut self) -> Result<Message<R>>;
}

/// A trait that allows the [`RequestResponseProtocol`] to get the recipients that a specific message should
/// expect responses from
pub trait RecipientSource<K: SignatureKey + 'static> {
    /// Get all the recipients that the specific message should expect responses from
    fn get_recipients_for<R: Request>(&self, message: &Message<R>) -> Vec<K>;
}

/// A blanket implementation of the [`Sender`] trait for all types that implement [`ConnectedNetwork`]
#[async_trait]
impl<N: ConnectedNetwork<K>, K: SignatureKey + 'static> Sender<K> for N {
    async fn send_message<R: Request>(&self, message: &Message<R>, recipient: K) -> Result<()> {
        // Serialize the message
        let serialized_message = message
            .to_bytes()
            .with_context(|| "failed to serialize message")?;

        // Just send the message to the recipient
        self.direct_message(serialized_message, recipient)
            .await
            .with_context(|| "failed to send message")
    }
}

/// An implementation of the [`Receiver`] trait for the [`mpsc::Receiver`] type. Allows us to send messages
/// to a channel and have the protocol receive them.
#[async_trait]
impl Receiver for mpsc::Receiver<Vec<u8>> {
    async fn receive_message<R: Request>(&mut self) -> Result<Message<R>> {
        // Receive a message from the channel
        let message = self.recv().await.ok_or(anyhow::anyhow!("channel closed"))?;

        // Convert the message to a [`Message`]
        Message::from_bytes(&message)
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
    pub async fn request<Req: Request>(
        &self,
        request: Req,
        timeout: Duration,
    ) -> Result<Req::Response> {
        // Create a request message
        let message: Message<Req> = Message::Request(request);

        // Get the recipients that the message should expect responses from
        let recipients = self.recipient_source.get_recipients_for(&message);

        todo!()
    }
}
