use std::{marker::PhantomData, rc::Rc, sync::Arc, time::Duration};

use anyhow::Result;
use dashmap::DashMap;
use data_source::DataSource;
use derive_more::derive::Deref;
use hotshot_types::traits::signature_key::SignatureKey;
use message::{Message, RequestMessage, ResponseMessage};
use network::{Receiver, Sender};
use recipient_source::RecipientSource;
use request::Request;
use tokio::sync::mpsc;
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
}

impl RequestResponseConfig {
    /// Create a new [`RequestResponseConfig`]
    pub fn new(response_timeout: Duration, incoming_request_ttl: Duration) -> Self {
        Self(Arc::new(RequestResponseConfigInner {
            response_timeout,
            incoming_request_ttl,
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
        mut receiver: R,
        data_source: DS,
        active_requests: Arc<DashMap<RequestHash, mpsc::Sender<()>>>,
    ) {
        while let Ok(message) = receiver.receive_message::<Req>().await {
            match message {
                Message::Request(request_message) => {
                    Handlers::handle_request(request_message, &config);
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
    fn handle_request<Req: Request, K: SignatureKey>(
        request: RequestMessage<Req, K>,
        config: &RequestResponseConfig,
    ) {
        // Validate the request message. This includes checking the signature and making sure it's
        // not too old
        if let Err(e) = request.validate(config.incoming_request_ttl) {
            warn!("Received invalid request: {e}");
            return;
        }

        // Make sure the request content itself is valid to the application
        if let Err(e) = request.content.validate() {
            warn!("Received invalid request content: {e}");
            return;
        }
    }

    /// Handle a response sent to us
    fn handle_response<Req: Request>(
        response: ResponseMessage<Req>,
        config: &RequestResponseConfig,
    ) {
        todo!()
    }
}
