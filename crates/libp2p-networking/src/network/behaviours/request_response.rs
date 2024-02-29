use std::collections::HashMap;

use async_compatibility_layer::channel::UnboundedSender;
use futures::channel::oneshot::Sender;
use libp2p::request_response::{Message, OutboundRequestId, ResponseChannel};
use serde::{Deserialize, Serialize};

use crate::network::NetworkEvent;

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

#[derive(Default, Debug)]
/// Handler for request response messages
pub(crate) struct RequestResponseState {
    /// Map requests to the their response channels
    request_map: HashMap<OutboundRequestId, Sender<Option<Response>>>,
}

impl RequestResponseState {
    /// Handles messages from the `request_response` behaviour by sending them to the application
    pub async fn handle_request_response(
        &mut self,
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
                    let Some(chan) = self.request_map.remove(&request_id) else {
                        return;
                    };
                    if chan.send(Some(response)).is_err() {
                        tracing::warn!("Failed to send resonse to client, channel closed.");
                    }
                }
            },
            libp2p::request_response::Event::OutboundFailure {
                peer: _,
                request_id,
                error,
            } => {
                tracing::warn!("Error Sending VID Request {:?}", error);
                let Some(chan) = self.request_map.remove(&request_id) else {
                    return;
                };
                if chan.send(None).is_err() {
                    tracing::warn!("Failed to send resonse to client, channel closed.");
                }
            }
            libp2p::request_response::Event::InboundFailure { .. }
            | libp2p::request_response::Event::ResponseSent { .. } => {}
        }
    }
    /// Add a requests return channel to the map of pending requests
    pub fn add_request(&mut self, id: OutboundRequestId, chan: Sender<Option<Response>>) {
        self.request_map.insert(id, chan);
    }
}
