use crate::network::{NetworkEvent, ResponseEvent};
use async_compatibility_layer::channel::UnboundedSender;
use libp2p::request_response::{Message, ResponseChannel};
use serde::{Deserialize, Serialize};

/// Request for VID data, contains the commitment for the data we want, and the hotshot
/// Public key.  
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request(#[serde(with = "serde_bytes")] pub Vec<u8>);

/// Response for some VID data that we already collected
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response(
    /// Data was found and sent back to us as bytes
    #[serde(with = "serde_bytes")]
    pub Vec<u8>,
);

/// Represents a request sent from another node for some data
/// Ecapusulates the request and the channel for the response.
#[derive(Debug)]
pub struct ResponseRequested(Request, ResponseChannel<Response>);

/// Handles messages from the `request_response` behaviour by sending them to the application
pub async fn handle_vid(
    event: libp2p::request_response::Event<Request, Response>,
    sender: UnboundedSender<NetworkEvent>,
) {
    match event {
        libp2p::request_response::Event::Message { peer: _, message } => match message {
            Message::Request {
                request_id: _,
                request,
                channel,
            } => {
                let _ = sender
                    .send(NetworkEvent::ResponseRequested(ResponseRequested(
                        request, channel,
                    )))
                    .await;
            }
            Message::Response {
                request_id,
                response,
            } => {
                let _ = sender
                    .send(NetworkEvent::ResponseReceived(ResponseEvent(
                        Some(response),
                        request_id,
                    )))
                    .await;
            }
        },
        libp2p::request_response::Event::OutboundFailure {
            peer: _,
            request_id,
            error,
        } => {
            tracing::warn!("Error Sending VID Request {:?}", error);
            let _ = sender
                .send(NetworkEvent::ResponseReceived(ResponseEvent(
                    None, request_id,
                )))
                .await;
        }
        libp2p::request_response::Event::InboundFailure { .. }
        | libp2p::request_response::Event::ResponseSent { .. } => {}
    }
}
