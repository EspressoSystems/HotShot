use std::ops::Deref;

use anyhow::{Context, Result};
use async_trait::async_trait;
use hotshot_types::traits::{network::ConnectedNetwork, signature_key::SignatureKey};
use tokio::sync::mpsc;

use super::{message::Message, request::Request, Serializable};

/// The [`Sender`] trait is used to allow the [`RequestResponseProtocol`] to send messages to a specific recipient
#[async_trait]
pub trait Sender<K: SignatureKey + 'static>: Send + Sync + 'static {
    /// Send a message to the specified recipient
    async fn send_message<R: Request>(&self, message: &Message<R>, recipient: K) -> Result<()>;
}

/// The [`Receiver`] trait is used to allow the [`RequestResponseProtocol`] to receive messages from a network
/// or other source.
#[async_trait]
pub trait Receiver: Send + Sync + 'static {
    /// Receive a message
    async fn receive_message<R: Request>(&mut self) -> Result<Message<R>>;
}

/// A blanket implementation of the [`Sender`] trait for all types that dereference to [`ConnectedNetwork`]
#[async_trait]
impl<T, K> Sender<K> for T
where
    T: Deref<Target: ConnectedNetwork<K>> + Send + Sync + 'static,
    K: SignatureKey + 'static,
{
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
