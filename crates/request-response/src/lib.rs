//! This crate contains a general request-response protocol. It is used to send requests to
//! a set of recipients and wait for responses.

use std::time::Instant;
use std::{marker::PhantomData, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use dashmap::DashMap;
use data_source::DataSource;
use derive_more::derive::Deref;
use hotshot_types::traits::signature_key::SignatureKey;
use message::{Message, RequestMessage, ResponseMessage};
use network::{Receiver, Sender};
use rand::seq::SliceRandom;
use recipient_source::RecipientSource;
use request::{Request, Response};
use tokio::spawn;
use tokio::time::{sleep, timeout};
use tracing::warn;
use util::abort_on_drop_handle::AbortOnDropHandle;
use util::bounded_vec_deque::BoundedVecDeque;

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
/// Utility types and functions
mod util;

/// A type alias for the hash of a request
pub type RequestHash = blake3::Hash;

/// A trait for serializing and deserializing a type to and from a byte array
pub trait Serializable: Sized {
    /// Serialize the type to a byte array
    ///
    /// # Errors
    /// - If the type cannot be serialized to a byte array
    fn to_bytes(&self) -> Result<Vec<u8>>;

    /// Deserialize the type from a byte array
    ///
    /// # Errors
    /// - If the byte array is not a valid representation of the type
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
    /// The maximum amount of time we will spend trying to both derive a response for a request and
    /// send the response over the wire.
    response_timeout: Duration,
    /// The batch size for outgoing requests. This is the number of request messages that we will
    /// send out at a time for a single request before waiting for the [`request_batch_interval`].
    request_batch_size: usize,
    /// The time to wait between sending out batches of request messages for a single request.
    request_batch_interval: Duration,
    /// The maximum (global) number of outgoing responses that can be in flight at any given time
    max_outgoing_responses: usize,
}

impl RequestResponseConfig {
    /// Create a new [`RequestResponseConfig`]
    #[must_use]
    pub fn new(
        response_timeout: Duration,
        incoming_request_ttl: Duration,
        request_batch_size: usize,
        request_batch_interval: Duration,
        max_outgoing_responses: usize,
    ) -> Self {
        Self(Arc::new(RequestResponseConfigInner {
            incoming_request_ttl,
            response_timeout,
            request_batch_size,
            request_batch_interval,
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
    _receive_task_handle: Arc<AbortOnDropHandle<()>>,
    /// The map of currently active requests
    active_requests: Arc<DashMap<RequestHash, ActiveRequest<Req>>>,
    /// Phantom data to help with type inference
    phantom_data: PhantomData<(K, R, Req, DS)>,
}

/// An active request. This is what we use to track a request and its corresponding response
/// in the protocol.
#[derive(Clone)]
pub struct ActiveRequest<R: Request> {
    /// The sender to use for the protocol
    sender: async_broadcast::Sender<R::Response>,
    /// The receiver to use for the protocol
    receiver: async_broadcast::Receiver<R::Response>,
    /// The request that we are waiting for a response to
    request: R,
    /// The hash of the request
    request_hash: RequestHash,
    /// A copy of the map of currently active requests
    active_requests: Arc<DashMap<RequestHash, ActiveRequest<R>>>,
    /// The tracker for references to this active request. We use this to
    /// remove ourselves from the map if we are dropped
    ref_counter: Arc<()>,
}

impl<R: Request> Drop for ActiveRequest<R> {
    fn drop(&mut self) {
        // If the reference counter == 2, we are the "last" reference. This is because
        // we have one reference in the map, and one reference in the `ActiveRequest` struct
        // itself.
        if Arc::strong_count(&self.ref_counter) == 2 {
            // Remove ourselves from the map
            self.active_requests.remove(&self.request_hash);
        }
    }
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
            Arc::clone(&active_requests),
        ))));

        Self {
            config,
            sender,
            recipient_source,
            _receive_task_handle: receive_task_handle,
            active_requests,
            phantom_data: PhantomData,
        }
    }

    /// The task responsible for receiving messages and handling them
    async fn receiving_task(
        config: RequestResponseConfig,
        sender: S,
        mut receiver: R,
        data_source: DS,
        active_requests: Arc<DashMap<RequestHash, ActiveRequest<Req>>>,
    ) {
        // Create a bounded queue for the currently active responses. We use this to make sure
        // we have less than [`config.max_outgoing_responses`] responses in flight at any time.
        let mut active_responses = BoundedVecDeque::new(config.max_outgoing_responses);

        while let Ok(message) = receiver.receive_message::<Req>().await {
            match message {
                Message::Request(request_message) => {
                    handle_request(
                        request_message,
                        &config,
                        &sender,
                        &data_source,
                        &mut active_responses,
                    );
                }
                Message::Response(response_message) => {
                    // Handle the response message
                    handle_response(response_message, &active_requests);
                }
            }
        }
        warn!("Request/response receive task exited: sending channel closed or dropped");
    }

    /// Request something from the protocol and wait for the response. This function
    /// will join with an existing request for the same data (determined by `Blake3` hash)
    ///
    /// # Errors
    /// - If the request times out
    /// - If the channel is closed (this is an internal error)
    pub async fn request(
        &self,
        request: RequestMessage<Req, K>,
        timeout_duration: Duration,
    ) -> Result<Req::Response> {
        // Calculate the hash of the request
        let request_hash = blake3::hash(&request.request.to_bytes()?);

        // Get the corresponding entry in the map. If it doesn't exist, we create a new
        // active request and insert it into the map. If it does, we join with the existing
        // active request
        let mut active_request = self
            .active_requests
            .entry(request_hash)
            .or_insert_with(|| {
                // Create a new broadcast channel for the response
                let (sender, receiver) = async_broadcast::broadcast(1);

                // Create a new active request
                ActiveRequest {
                    sender,
                    receiver,
                    request: request.request.clone(),
                    request_hash,
                    active_requests: Arc::clone(&self.active_requests),
                    ref_counter: Arc::new(()),
                }
            })
            .value()
            .clone();

        // Get the recipients that the message should expect responses from. Shuffle them so
        // that we don't always send to the same recipients in the same order
        let mut recipients = self.recipient_source.get_recipients_for(&request.request);
        recipients.shuffle(&mut rand::thread_rng());

        // Create a request message
        let message = Arc::new(Message::Request(request));

        // Get the current time so we can check when the timeout has elapsed
        let start_time = Instant::now();

        // Spawn a task that sends out requests to the network
        let config_clone = self.config.clone();
        let sender_clone = self.sender.clone();
        let _handle = AbortOnDropHandle(spawn(async move {
            // Create a bounded queue for the outgoing requests. We use this to make sure
            // we have less than [`config.request_batch_size`] requests in flight at any time
            let mut outgoing_requests = BoundedVecDeque::new(config_clone.request_batch_size);

            // While the timeout hasn't elapsed, we send out requests to the network
            while start_time.elapsed() < timeout_duration {
                // Send out requests to the network in their own separate tasks
                for recipient_batch in recipients.chunks(config_clone.request_batch_size) {
                    for recipient in recipient_batch {
                        // Clone the message, recipient, and sender
                        let message_clone = Arc::clone(&message);
                        let recipient_clone = recipient.clone();
                        let sender_clone = sender_clone.clone();

                        // Spawn the task that sends the request to the participant
                        let individual_sending_task = spawn(async move {
                            let _ = sender_clone
                                .send_message(&message_clone, recipient_clone)
                                .await;
                        });

                        // Add the sending task to the queue
                        outgoing_requests.push(AbortOnDropHandle(individual_sending_task));
                    }

                    // After we send the batch out, wait the [`config.request_batch_interval`]
                    // before sending the next one
                    sleep(config_clone.request_batch_interval).await;
                }
            }
        }));

        // Wait for a response on the channel (or timeout)
        timeout(timeout_duration, active_request.receiver.recv())
            .await
            .map_err(|_| anyhow::anyhow!("request timed out"))
            .and_then(|result| result.map_err(|e| anyhow::anyhow!("channel was closed: {e}")))
    }
}

/// Handle a request sent to us
fn handle_request<Req: Request, K: SignatureKey + 'static, DS: DataSource<Req>, S: Sender<K>>(
    request_message: RequestMessage<Req, K>,
    config: &RequestResponseConfig,
    sender: &S,
    data_source: &DS,
    active_responses: &mut BoundedVecDeque<AbortOnDropHandle<()>>,
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
                        request_hash: blake3::hash(&request_message.request.to_bytes()?),
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

    // Add the response task to the active responses queue. This will automatically cancel an older task
    // if there are more than [`config.max_outgoing_responses`] responses in flight.
    active_responses.push(AbortOnDropHandle(response_task));
}

/// Handle a response sent to us
fn handle_response<Req: Request>(
    response: ResponseMessage<Req>,
    active_requests: &Arc<DashMap<RequestHash, ActiveRequest<Req>>>,
) {
    // Get the entry in the map, ignoring it if it doesn't exist
    let Some(active_request) = active_requests
        .get(&response.request_hash)
        .map(|r| r.value().clone())
    else {
        return;
    };

    // Make sure the response is valid for the given request
    if let Err(e) = response.response.validate(&active_request.request) {
        warn!("Received invalid response: {e}");
        return;
    }

    // Send the response to the requester (the user of [`RequestResponse::request`])
    let _ = active_request.sender.try_broadcast(response.response);
}
