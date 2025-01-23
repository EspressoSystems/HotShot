//! This crate contains a general request-response protocol. It is used to send requests to
//! a set of recipients and wait for responses.

use std::time::Instant;
use std::{marker::PhantomData, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use dashmap::DashMap;
use data_source::DataSource;
use derive_builder::Builder;
use derive_more::derive::Deref;
use hotshot_types::traits::signature_key::SignatureKey;
use message::{Message, RequestMessage, ResponseMessage};
use network::{Bytes, Receiver, Sender};
use rand::seq::SliceRandom;
use recipient_source::RecipientSource;
use request::{Request, Response};
use tokio::spawn;
use tokio::time::{sleep, timeout};
use tracing::{error, warn};
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

/// A trait for serializing and deserializing a type to and from a byte array. [`Request`] types and
/// [`Response`] types will need to implement this trait
pub trait Serializable: Sized {
    /// Serialize the type to a byte array. If this is for a [`Request`] and your [`Request`] type
    /// is represented as an enum, please make sure that you serialize it with a unique type ID. Otherwise,
    /// you may end up with collisions as the request hash is used as a unique identifier
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

/// The underlying configuration for the request-response protocol
#[derive(Clone, Builder)]
pub struct RequestResponseConfig {
    /// The timeout for incoming requests. Do not respond to a request after this threshold
    /// has passed.
    incoming_request_ttl: Duration,
    /// The maximum amount of time we will spend trying to both derive a response for a request and
    /// send the response over the wire.
    response_timeout: Duration,
    /// The batch size for outgoing requests. This is the number of request messages that we will
    /// send out at a time for a single request before waiting for the [`request_batch_interval`].
    request_batch_size: usize,
    /// The time to wait (per request) between sending out batches of request messages
    request_batch_interval: Duration,
    /// The maximum (global) number of outgoing responses that can be in flight at any given time
    max_outgoing_responses: usize,
}

/// A protocol that allows for request-response communication. Is cheaply cloneable, so there is no
/// need to wrap it in an `Arc`
#[derive(Clone, Deref)]
pub struct RequestResponse<
    S: Sender<K>,
    R: Receiver,
    Req: Request,
    RS: RecipientSource<K>,
    DS: DataSource<Req>,
    K: SignatureKey + 'static,
>(Arc<RequestResponseInner<S, R, Req, RS, DS, K>>);

/// The inner implementation for the request-response protocol
pub struct RequestResponseInner<
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
    /// The handle to the task that receives messages
    _receive_task_handle: AbortOnDropHandle<()>,
    /// The map of currently active requests
    active_requests: Arc<DashMap<RequestHash, ActiveRequest<Req>>>,
    /// Phantom data to help with type inference
    phantom_data: PhantomData<(K, R, Req, DS)>,
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
        // The network sender that [`RequestResponseProtocol`] will use to send messages
        sender: S,
        // The network receiver that [`RequestResponseProtocol`] will use to receive messages
        receiver: R,
        // The recipient source that [`RequestResponseProtocol`] will use to get the recipients
        // that a specific message should expect responses from
        recipient_source: RS,
        // The [response] data source that [`RequestResponseProtocol`] will use to derive the
        // response data for a specific request
        data_source: DS,
    ) -> Self {
        // Create the active requests map
        let active_requests = Arc::new(DashMap::new());

        // Start the task that receives messages and handles them. This will automatically get cancelled
        // when the protocol is dropped
        let receive_task_handle = AbortOnDropHandle(tokio::spawn(Self::receiving_task(
            config.clone(),
            sender.clone(),
            receiver,
            data_source,
            Arc::clone(&active_requests),
        )));

        Self(Arc::new(RequestResponseInner {
            config,
            sender,
            recipient_source,
            _receive_task_handle: receive_task_handle,
            active_requests,
            phantom_data: PhantomData,
        }))
    }

    /// The task responsible for receiving messages from the receiver and handling them
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

        // While the receiver is open, we receive messages and handle them
        while let Ok(message) = receiver.receive_message().await {
            // Deserialize the message, warning if it fails
            let message = match Message::from_bytes(&message) {
                Ok(message) => message,
                Err(e) => {
                    warn!("Received invalid message: {e}");
                    continue;
                }
            };

            // Handle the message based on its type
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
                    handle_response(response_message, &active_requests);
                }
            }
        }
        error!("Request/response receive task exited: sending channel closed or dropped");
    }

    /// Request something from the protocol and wait for the response. This function
    /// will join with an existing request for the same data (determined by `Blake3` hash)
    ///
    /// # Errors
    /// - If the request times out
    /// - If the channel is closed (this is an internal error)
    pub async fn request(
        &self,
        request_message: RequestMessage<Req, K>,
        timeout_duration: Duration,
    ) -> Result<Req::Response> {
        // Calculate the hash of the request
        let request_hash = blake3::hash(&request_message.request.to_bytes()?);

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
                    request: request_message.request.clone(),
                    request_hash,
                    active_requests: Arc::clone(&self.active_requests),
                    ref_counter: Arc::new(()),
                }
            })
            .value()
            .clone();

        // Get the recipients that the message should expect responses from. Shuffle them so
        // that we don't always send to the same recipients in the same order
        let mut recipients = self
            .recipient_source
            .get_recipients_for(&request_message.request);
        recipients.shuffle(&mut rand::thread_rng());

        // Create a request message and serialize it
        let message = Bytes::from(
            Message::Request(request_message)
                .to_bytes()
                .with_context(|| "failed to serialize request message")?,
        );

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

            // Create the response message and serialize it
            let response = Bytes::from(
                Message::Response::<Req, K>(ResponseMessage {
                    request_hash: blake3::hash(&request_message.request.to_bytes()?),
                    response,
                })
                .to_bytes()
                .with_context(|| "failed to serialize response message")?,
            );

            // Send the response to the requester
            sender_clone
                .send_message(&response, request_message.public_key)
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

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use async_trait::async_trait;
    use hotshot_types::signature_key::{BLSPrivKey, BLSPubKey};
    use rand::Rng;
    use tokio::{sync::mpsc, task::JoinSet};

    use super::*;

    /// A test sender that has a list of all the participants in the network
    #[derive(Clone)]
    pub struct TestSender {
        network: Arc<HashMap<BLSPubKey, mpsc::Sender<Bytes>>>,
    }

    /// An implementation of the [`Sender`] trait for the [`TestSender`] type
    #[async_trait]
    impl Sender<BLSPubKey> for TestSender {
        async fn send_message(&self, message: &Bytes, recipient: BLSPubKey) -> Result<()> {
            self.network
                .get(&recipient)
                .ok_or(anyhow::anyhow!("recipient not found"))?
                .send(Arc::clone(message))
                .await
                .map_err(|_| anyhow::anyhow!("failed to send message"))?;

            Ok(())
        }
    }

    // Implement the [`RecipientSource`] trait for the [`TestSender`] type
    impl RecipientSource<BLSPubKey> for TestSender {
        fn get_recipients_for<R: Request>(&self, _request: &R) -> Vec<BLSPubKey> {
            // Get all the participants in the network
            self.network.keys().copied().collect()
        }
    }

    // Create a test request that is just some bytes
    #[derive(Clone, Debug)]
    struct TestRequest(Vec<u8>);

    // Implement the [`Serializable`] trait for the [`TestRequest`] type
    impl Serializable for TestRequest {
        fn to_bytes(&self) -> Result<Vec<u8>> {
            Ok(self.0.clone())
        }

        fn from_bytes(bytes: &[u8]) -> Result<Self> {
            Ok(TestRequest(bytes.to_vec()))
        }
    }

    // Implement the [`Request`] trait for the [`TestRequest`] type
    impl Request for TestRequest {
        type Response = Vec<u8>;
        fn validate(&self) -> Result<()> {
            Ok(())
        }
    }

    // Implement the [`Response`] trait for the [`TestRequest`] type
    impl Response<TestRequest> for Vec<u8> {
        fn validate(&self, _request: &TestRequest) -> Result<()> {
            Ok(())
        }
    }

    // Create a test data source that pretends to have the data or not
    #[derive(Clone)]
    struct TestDataSource {
        /// Whether we have the data or not
        has_data: bool,
        /// The time at which the data is available
        data_available_time: Instant,
    }

    #[async_trait]
    impl DataSource<Vec<u8>> for TestDataSource {
        async fn derive_response_for(&self, request: &Vec<u8>) -> Result<Vec<u8>> {
            // Return a response if we hit the hit rate
            if self.has_data && Instant::now() >= self.data_available_time {
                Ok(blake3::hash(request).as_bytes().to_vec())
            } else {
                Err(anyhow::anyhow!("did not have the data"))
            }
        }
    }

    fn default_protocol_config() -> RequestResponseConfig {
        RequestResponseConfigBuilder::create_empty()
            .incoming_request_ttl(Duration::from_secs(6))
            .response_timeout(Duration::from_secs(6))
            .request_batch_size(10)
            .request_batch_interval(Duration::from_millis(100))
            .max_outgoing_responses(10)
            .build()
            .expect("failed to build config")
    }

    /// Create fully connected test networks with `num_participants` participants
    fn create_participants(
        num: usize,
    ) -> Vec<(TestSender, mpsc::Receiver<Bytes>, (BLSPubKey, BLSPrivKey))> {
        // The entire network
        let mut network = HashMap::new();

        // All receivers in the network
        let mut receivers = Vec::new();

        // All keypairs in the network
        let mut keypairs = Vec::new();

        // For each participant,
        for i in 0..num {
            // Create a unique `BLSPubKey`
            let (public_key, private_key) =
                BLSPubKey::generated_from_seed_indexed([2; 32], i.try_into().unwrap());

            // Add the keypair to the list
            keypairs.push((public_key, private_key));

            // Create a channel for sending and receiving messages
            let (sender, receiver) = mpsc::channel::<Bytes>(100);

            // Add the participant to the network
            network.insert(public_key, sender);

            // Add the receiver to the list of receivers
            receivers.push(receiver);
        }

        // Create a test sender from the network
        let sender = TestSender {
            network: Arc::new(network),
        };

        // Return all senders and receivers
        receivers
            .into_iter()
            .zip(keypairs)
            .map(|(r, k)| (sender.clone(), r, k))
            .collect()
    }

    /// The configuration for an integration test
    #[derive(Clone)]
    struct IntegrationTestConfig {
        /// The request response protocol configuration
        request_response_config: RequestResponseConfig,
        /// The number of participants in the network
        num_participants: usize,
        /// The number of participants that have the data
        num_participants_with_data: usize,
        /// The timeout for the requests
        request_timeout: Duration,
        /// The delay before the nodes have the data available
        data_available_delay: Duration,
    }

    /// The result of an integration test
    struct IntegrationTestResult {
        /// The number of nodes that received a response
        num_succeeded: usize,
    }

    /// Run an integration test with the given parameters
    async fn run_integration_test(config: IntegrationTestConfig) -> IntegrationTestResult {
        // Create a fully connected network with `num_participants` participants
        let participants = create_participants(config.num_participants);

        // Create a join set to wait for all the tasks to finish
        let mut join_set = JoinSet::new();

        // We need to keep these here so they don't get dropped
        let handles = Arc::new(Mutex::new(Vec::new()));

        // For each one, create a new [`RequestResponse`] protocol
        for (i, (sender, receiver, (public_key, private_key))) in
            participants.into_iter().enumerate()
        {
            let config_clone = config.request_response_config.clone();
            let handles_clone = Arc::clone(&handles);
            join_set.spawn(async move {
                let protocol = RequestResponse::new(
                    config_clone,
                    sender.clone(),
                    receiver,
                    sender,
                    TestDataSource {
                        has_data: i < config.num_participants_with_data,
                        data_available_time: Instant::now() + config.data_available_delay,
                    },
                );

                // Add the handle to the handles list so it doesn't get dropped and
                // cancelled
                #[allow(clippy::used_underscore_binding)]
                handles_clone.lock().unwrap().push(Arc::clone(&protocol.0));

                // Create a random request
                let request = vec![rand::thread_rng().gen(); 100];

                // Get the hash of the request
                let request_hash = blake3::hash(&request).as_bytes().to_vec();

                // Create a new request message
                let request = RequestMessage::new_signed(&public_key, &private_key, &request)
                    .expect("failed to create request message");

                // Request the data from the protocol
                let response = protocol.request(request, config.request_timeout).await?;

                // Make sure the response is the hash of the request
                assert_eq!(response, request_hash);

                Ok::<(), anyhow::Error>(())
            });
        }

        // Wait for all the tasks to finish
        let mut num_succeeded = config.num_participants;
        while let Some(result) = join_set.join_next().await {
            if result.is_err() || result.unwrap().is_err() {
                num_succeeded -= 1;
            }
        }

        IntegrationTestResult { num_succeeded }
    }

    /// Test the integration of the protocol with 50% of the participants having the data
    #[tokio::test(flavor = "multi_thread")]
    async fn test_integration_50_0s() {
        // Build a config
        let config = IntegrationTestConfig {
            request_response_config: default_protocol_config(),
            num_participants: 100,
            num_participants_with_data: 50,
            request_timeout: Duration::from_secs(4),
            data_available_delay: Duration::from_secs(0),
        };

        // Run the test, making sure all the requests succeed
        let result = run_integration_test(config).await;
        assert_eq!(result.num_succeeded, 100);
    }

    /// Test the integration of the protocol when nobody has the data. Make sure we don't
    /// get any responses
    #[tokio::test(flavor = "multi_thread")]
    async fn test_integration_0() {
        // Build a config
        let config = IntegrationTestConfig {
            request_response_config: default_protocol_config(),
            num_participants: 100,
            num_participants_with_data: 0,
            request_timeout: Duration::from_secs(4),
            data_available_delay: Duration::from_secs(0),
        };

        // Run the test
        let result = run_integration_test(config).await;

        // Make sure all the requests succeeded
        assert_eq!(result.num_succeeded, 0);
    }

    /// Test the integration of the protocol when one node has the data after
    /// a delay of 1s
    #[tokio::test(flavor = "multi_thread")]
    async fn test_integration_1_1s() {
        // Build a config
        let config = IntegrationTestConfig {
            request_response_config: default_protocol_config(),
            num_participants: 100,
            num_participants_with_data: 1,
            request_timeout: Duration::from_secs(10),
            data_available_delay: Duration::from_secs(1),
        };

        // Run the test
        let result = run_integration_test(config).await;

        // Make sure all the requests succeeded
        assert_eq!(result.num_succeeded, 100);
    }
}
