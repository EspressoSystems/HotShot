use std::{marker::PhantomData, num::NonZero, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use dashmap::DashMap;
use data_source::DataSource;
use derive_more::derive::Deref;
use hotshot_types::traits::signature_key::SignatureKey;
use lru::LruCache;
use message::{Message, RequestMessage, ResponseMessage};
use network::{Receiver, Sender};
use parking_lot::RwLock;
use recipient_source::RecipientSource;
use request::Request;
use tokio::{sync::mpsc, time::timeout};
use tracing::warn;
use utils::abort_on_drop_handle::AbortOnDropHandle;

use crate::traits::networking::combined_network::calculate_hash_of;

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

/// A type alias for the hash of a request
pub type RequestHash = u64;

/// A trait for serializing and deserializing a type to and from a byte array
pub trait Serializable: Sized {
    /// Serialize the type to a byte array
    fn to_bytes(&self) -> Result<Vec<u8>>;

    /// Deserialize the type from a byte array
    fn from_bytes(bytes: &[u8]) -> Result<Self>;
}

/// The request-response configuration
#[derive(Clone, Deref)]
pub struct RequestResponseConfig(Arc<RequestResponseConfigInner>);

/// An `inner` struct for the [`RequestResponseConfig`]. We only use this
/// to make the [`RequestResponseConfig`] cloneable with minimal overhead
pub struct RequestResponseConfigInner {
    /// The timeout for incoming requests. Do not respond to a request after this threshold
    /// has passed.
    incoming_request_ttl: Duration,
    /// The timeout for sending responses. Includes the time it takes to derive the response
    /// and send it over the wire.
    response_timeout: Duration,
    /// The maximum number of outgoing responses that can be in flight at any time
    max_outgoing_responses: usize,
}

impl RequestResponseConfig {
    /// Create a new [`RequestResponseConfig`]
    pub fn new(
        response_timeout: Duration,
        incoming_request_ttl: Duration,
        max_outgoing_responses: usize,
    ) -> Self {
        Self(Arc::new(RequestResponseConfigInner {
            response_timeout,
            incoming_request_ttl,
            max_outgoing_responses,
        }))
    }
}

/// A protocol that allows for request-response communication
#[derive(Clone)]
pub struct RequestResponse<
    S: Sender<K>,
    R: Receiver<K>,
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
    /// The handle to the task that receives messages
    receive_task_handle: Arc<AbortOnDropHandle<()>>,
    /// Phantom data to help with type inference
    phantom_data: PhantomData<(K, R, Req, DS)>,
}

impl<
        S: Sender<K>,
        R: Receiver<K>,
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
        // The [response] data source to use for the protocol
        data_source: DS,
    ) -> Self {
        // Create the active requests map
        let active_requests = Arc::new(DashMap::new());

        // Start the task that receives messages and handles them
        let receive_task_handle = Arc::new(AbortOnDropHandle(tokio::spawn(Self::receiving_task(
            config.clone(),
            sender.clone(),
            receiver,
            data_source,
            active_requests.clone(),
        ))));

        Self {
            config,
            sender,
            recipient_source,
            receive_task_handle,
            phantom_data: PhantomData,
        }
    }

    /// The task responsible for receiving messages and handling them
    async fn receiving_task(
        config: RequestResponseConfig,
        sender: S,
        mut receiver: R,
        data_source: DS,
        active_requests: Arc<DashMap<RequestHash, mpsc::Sender<()>>>,
    ) {
        while let Ok(message) = receiver.receive_message::<Req>().await {
            match message {
                Message::Request(request_message) => {
                    Handlers::handle_request(request_message, &config, &sender, &data_source);
                }
                Message::Response(response_message) => {
                    // Handle the response message
                    Handlers::handle_response(response_message, &config);
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
        // let _message = Message::Request(request);

        todo!()
    }
}

struct Handlers;
impl Handlers {
    /// Handle a request sent to us
    fn handle_request<
        Req: Request,
        K: SignatureKey + 'static,
        DS: DataSource<Req>,
        S: Sender<K>,
    >(
        request_message: RequestMessage<Req, K>,
        config: &RequestResponseConfig,
        sender: &S,
        data_source: &DS,
    ) {
        // Validate the request message. This includes:
        // - Checking the signature and making sure it's valid
        // - Checking the timestamp and making sure it's not too old
        // - Calling the request's application-specific validation function
        if let Err(e) = request_message.validate(config.incoming_request_ttl) {
            warn!("Received invalid request: {e}");
            return;
        }

        // Spawn a task to:
        // - Derive the response data (check if we have it)
        // - Send the response to the requester
        let data_source_clone = data_source.clone();
        let sender_clone = sender.clone();
        let config_clone = config.clone();
        let response_task = tokio::spawn(async move {
            let result = timeout(config_clone.response_timeout, async move {
                // Try to fetch the response data from the data source
                let response = data_source_clone
                    .derive_response_for(&request_message.request)
                    .await
                    .with_context(|| "failed to derive response for request")?;

                // Send the response to the requester
                sender_clone
                    .send_message(
                        &Message::Response(ResponseMessage::<Req> {
                            request_hash: calculate_hash_of(&request_message.request),
                            response,
                        }),
                        request_message.public_key,
                    )
                    .await
                    .with_context(|| "failed to send response to requester")?;

                Ok::<(), anyhow::Error>(())
            })
            .await
            .map_err(|_| anyhow::anyhow!("timed out while sending response"))
            .and_then(|result| result);

            if let Err(e) = result {
                warn!("Failed to send response to requester: {e}");
            }
        });
    }

    /// Handle a response sent to us
    fn handle_response<Req: Request>(
        response: ResponseMessage<Req>,
        config: &RequestResponseConfig,
    ) {
        todo!()
    }
}
