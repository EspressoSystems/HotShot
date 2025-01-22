use std::{marker::PhantomData, sync::Arc, time::Duration};

use anyhow::Result;
use data_source::DataSource;
use hotshot_types::traits::signature_key::SignatureKey;
use message::Message;
use network::{Receiver, Sender};
use recipient_source::RecipientSource;
use request::Request;
use tracing::warn;
use utils::abort_on_drop_handle::AbortOnDropHandle;

/// The data source trait. Is what we use to derive the response data for a request
pub mod data_source;
/// The message type. Is the base type for all messages in the request-response protocol
pub mod message;
/// The network traits. Is what we use to send and receive messages over the network as
/// the protocol
pub mod network;
/// The recipient source trait. Is what we use to get the recipients that a specific message should
/// expect responses from
pub mod recipient_source;
/// The request trait. Is what we use to define a request and a corresponding response type
pub mod request;

/// A trait for serializing and deserializing a type to and from a byte array
pub trait Serializable: Sized {
    /// Serialize the type to a byte array
    fn to_bytes(&self) -> Result<Vec<u8>>;

    /// Deserialize the type from a byte array
    fn from_bytes(bytes: &[u8]) -> Result<Self>;
}

/// The request-response configuration
#[derive(Clone)]
pub struct RequestResponseConfig {
    /// The timeout for responses. Includes the time it takes to derive the response data
    /// and send it over the wire.
    response_timeout: Duration,
}

/// A protocol that allows for request-response communication
#[derive(Clone)]
pub struct RequestResponse<
    S: Sender<K>,
    R: Receiver,
    Req: Request,
    RS: RecipientSource<K>,
    DS: DataSource<Req>,
    K: SignatureKey + 'static,
> {
    /// The configuration of the protocol
    config: RequestResponseConfig,
    /// The sender to use for the protocol
    sender: S,
    /// The recipient source to use for the protocol
    recipient_source: RS,
    /// The response data source to use for the protocol
    response_data_source: DS,
    /// The handle to the task that receives messages
    receive_task_handle: Arc<AbortOnDropHandle<()>>,
    /// Phantom data to help with type inference
    phantom_data: PhantomData<(K, R, Req)>,
}

impl<
        S: Sender<K>,
        R: Receiver,
        Req: Request,
        RS: RecipientSource<K>,
        DS: DataSource<Req>,
        K: SignatureKey + 'static,
    > RequestResponse<S, R, Req, RS, DS, K>
{
    /// Create a new [`RequestResponseProtocol`]
    pub fn new(
        // The configuration for the protocol
        config: RequestResponseConfig,
        // The network sender to use for the protocol
        sender: S,
        // The network receiver to use for the protocol
        receiver: R,
        // The recipient source to use for the protocol
        recipient_source: RS,
        // The response data source to use for the protocol
        response_data_source: DS,
    ) -> Self {
        // Start the task that receives messages and handles them
        let receive_task_handle = Arc::new(AbortOnDropHandle(tokio::spawn(Self::receive_task(
            receiver,
        ))));

        Self {
            config,
            sender,
            recipient_source,
            response_data_source,
            receive_task_handle,
            phantom_data: PhantomData,
        }
    }

    /// The task responsible for receiving messages and handling them
    async fn receive_task(mut receiver: R) {
        while let Ok(message) = receiver.receive_message::<Req>().await {
            match message {
                Message::Request(_request) => {
                    // Handle the request message
                    todo!()
                }
                Message::Response(_response) => {
                    // Handle the response message
                    todo!()
                }
            }
        }
        warn!("Request/response receive task exited: sending channel closed or dropped")
    }

    /// Request something from the protocol and wait for the response
    pub async fn request(&self, request: Req, _timeout: Duration) -> Result<Req::Response> {
        // Get the recipients that the message should expect responses from
        let _recipients = self.recipient_source.get_recipients_for(&request);

        // Create a request message
        let _message = Message::Request(request);

        todo!()
    }
}
