use std::{collections::{VecDeque, HashMap}, task::Poll};

use futures::{Future};
use libp2p::{swarm::{NetworkBehaviour, NetworkBehaviourAction, NetworkBehaviourEventProcess}, request_response::{RequestResponse, RequestId, RequestResponseEvent, RequestResponseMessage, ResponseChannel}, PeerId, Multiaddr};
use tracing::{error, info};

use crate::network::NetworkEvent;

use super::{exponential_backoff::ExponentialBackoff, direct_message_codec::{DirectMessageCodec, DirectMessageRequest, DirectMessageResponse}};


pub struct DMRequest {
    pub peer_id: PeerId,
    pub data: Vec<u8>,
    pub backoff: ExponentialBackoff
}

// wrapper metadata around libp2p's request response
pub struct DMBehaviour {
    request_response: RequestResponse<DirectMessageCodec>,
    in_progress_rr: HashMap<RequestId, DMRequest>,
    failed_rr: VecDeque<DMRequest>,
    out_event_queue: Vec<DMEvent>
}

pub enum DMEvent {
    DirectRequest(Vec<u8>, PeerId, ResponseChannel<DirectMessageResponse>),
    DirectResponse(Vec<u8>, PeerId)
}

impl Into<NetworkEvent> for DMEvent {
    fn into(self) -> NetworkEvent {
        // if we get an event from the dm behaviour, push it
        // onto the event queue (which will get popped during poll)
        // and propagated back to the overall behaviour
        match self {
            DMEvent::DirectRequest(data, pid, chan) => {
                NetworkEvent::DirectRequest(data, pid, chan)
            },
            DMEvent::DirectResponse(data, pid) => {
                NetworkEvent::DirectResponse(data, pid)
            },
        }
    }
}

impl NetworkBehaviourEventProcess<RequestResponseEvent<DirectMessageRequest, DirectMessageResponse>>
    for DMBehaviour
{
    fn inject_event(&mut self, event: RequestResponseEvent<DirectMessageRequest, DirectMessageResponse>) {
        match event {
            RequestResponseEvent::InboundFailure {
                peer, request_id, error,
            } => {
                error!("inbound failure to send message to {:?} with error {:?}", peer, error);
                if let Some(mut req) = self.in_progress_rr.remove(&request_id) {
                    req.backoff.start_next(false);
                    self.failed_rr.push_back(req);
                }
            }
            | RequestResponseEvent::OutboundFailure {
                peer, request_id, error
            } => {
                error!("outbound failure to send message to {:?} with error {:?}", peer, error);
                if let Some(mut req) = self.in_progress_rr.remove(&request_id) {
                    req.backoff.start_next(false);
                    self.failed_rr.push_back(req);
                }
            }
            RequestResponseEvent::Message { message, peer, .. } => match message {
                RequestResponseMessage::Request {
                    request: DirectMessageRequest(msg),
                    channel,
                    ..
                } => {
                    // receiver, not initiator.
                    // don't track. If we are disconnected, sender will reinitiate
                    self.out_event_queue
                        .push(DMEvent::DirectRequest(msg, peer, channel));
                }
                RequestResponseMessage::Response {
                    request_id,
                    response: DirectMessageResponse(msg),
                } => {
                    // success, finished.
                    if let Some(req) = self.in_progress_rr.remove(&request_id) {
                        self.out_event_queue
                            .push(DMEvent::DirectResponse(msg, req.peer_id));
                    } else {

                        error!("recv-ed a direct response, but is no longer tracking message!");
                    }
                }
            },
            e @ RequestResponseEvent::ResponseSent { .. } => {
                info!(?e, " sending response");
            }
        }
    }
}

impl NetworkBehaviour for DMBehaviour {
    type ConnectionHandler = <RequestResponse<DirectMessageCodec> as NetworkBehaviour>::ConnectionHandler;

    type OutEvent = DMEvent;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        self.request_response.new_handler()
    }

    fn inject_event(
        &mut self,
        peer_id: libp2p::PeerId,
        connection: libp2p::core::connection::ConnectionId,
        event: <<Self::ConnectionHandler as libp2p::swarm::IntoConnectionHandler>::Handler as libp2p::swarm::ConnectionHandler>::OutEvent,
    ) {
        NetworkBehaviour::inject_event(&mut self.request_response, peer_id, connection, event)
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
        params: &mut impl libp2p::swarm::PollParameters,
    ) -> std::task::Poll<libp2p::swarm::NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
        while let Some(req) = self.failed_rr.pop_front() {
            if req.backoff.is_expired() {
                self.send_rr(req);
            } else {
                self.failed_rr.push_back(req);
            }
        }
        loop {
            match NetworkBehaviour::poll(&mut self.request_response, cx, params) {
                Poll::Ready(ready) => {
                    match ready {
                        // NOTE: this generates request
                        NetworkBehaviourAction::GenerateEvent(e) => {
                            NetworkBehaviourEventProcess::inject_event(self, e)
                        },
                        NetworkBehaviourAction::Dial { opts, handler } => {
                            return Poll::Ready(NetworkBehaviourAction::Dial {
                                opts,
                                handler,
                            });
                        },
                        NetworkBehaviourAction::NotifyHandler { peer_id, handler, event } => {

                            return Poll::Ready(
                                NetworkBehaviourAction::NotifyHandler {
                                    peer_id,
                                    handler,
                                    event
                                },
                                );
                        },
                        NetworkBehaviourAction::ReportObservedAddr { address, score } => {
                            return Poll::Ready(
                                NetworkBehaviourAction::ReportObservedAddr {
                                    address,
                                    score,
                                },
                                );
                        },
                        NetworkBehaviourAction::CloseConnection { peer_id, connection } => {
                            return Poll::Ready(
                                NetworkBehaviourAction::CloseConnection {
                                    peer_id,
                                    connection,
                                });
                        },
                    }
                },
                Poll::Pending => {
                    break
                },
            }
        }
        // tco
        let f: Poll<
            NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>,
            > = Self::poll(self, cx, params);
        f
    }

    fn addresses_of_peer(&mut self, pid: &PeerId) -> Vec<libp2p::Multiaddr> {
        self.request_response.addresses_of_peer(pid)
    }

    fn inject_connection_established(
        &mut self,
        peer_id: &PeerId,
        connection_id: &libp2p::core::connection::ConnectionId,
        endpoint: &libp2p::core::ConnectedPoint,
        failed_addresses: Option<&Vec<libp2p::Multiaddr>>,
        other_established: usize,
    ) {
        self.request_response.inject_connection_established(peer_id, connection_id, endpoint, failed_addresses, other_established);
    }

    fn inject_connection_closed(
        &mut self,
        pid: &PeerId,
        cid: &libp2p::core::connection::ConnectionId,
        cp: &libp2p::core::ConnectedPoint,
        handler: <Self::ConnectionHandler as libp2p::swarm::IntoConnectionHandler>::Handler,
        remaining_established: usize,
    ) {
        self.request_response.inject_connection_closed(pid, cid, cp, handler, remaining_established);
    }

    fn inject_address_change(
        &mut self,
        pid: &PeerId,
        cid: &libp2p::core::connection::ConnectionId,
        old: &libp2p::core::ConnectedPoint,
        new: &libp2p::core::ConnectedPoint,
    ) {
        self.request_response.inject_address_change(pid, cid, old, new);
    }

    fn inject_dial_failure(
        &mut self,
        peer_id: Option<PeerId>,
        handler: Self::ConnectionHandler,
        error: &libp2p::swarm::DialError,
    ) {
        self.request_response.inject_dial_failure(peer_id, handler, error);
    }

    fn inject_listen_failure(
        &mut self,
        local_addr: &libp2p::Multiaddr,
        send_back_addr: &libp2p::Multiaddr,
        handler: Self::ConnectionHandler,
    ) {
        self.request_response.inject_listen_failure(local_addr, send_back_addr, handler);
    }

    fn inject_new_listener(&mut self, id: libp2p::core::connection::ListenerId) {
        self.request_response.inject_new_listener(id);
    }

    fn inject_new_listen_addr(&mut self, id: libp2p::core::connection::ListenerId, addr: &libp2p::Multiaddr) {
        self.request_response.inject_new_listen_addr(id, addr);
    }

    fn inject_expired_listen_addr(&mut self, id: libp2p::core::connection::ListenerId, addr: &libp2p::Multiaddr) {
        self.request_response.inject_expired_listen_addr(id, addr);
    }

    fn inject_listener_error(&mut self, id: libp2p::core::connection::ListenerId, err: &(dyn std::error::Error + 'static)) {
        self.request_response.inject_listener_error(id, err);
    }

    fn inject_listener_closed(&mut self, id: libp2p::core::connection::ListenerId, reason: Result<(), &std::io::Error>) {
        self.request_response.inject_listener_closed(id, reason);
    }

    fn inject_new_external_addr(&mut self, addr: &libp2p::Multiaddr) {
        self.request_response.inject_new_external_addr(addr);
    }

    fn inject_expired_external_addr(&mut self, addr: &libp2p::Multiaddr) {
        self.request_response.inject_expired_external_addr(addr);
    }
}

impl DMBehaviour {
    pub fn new(request_response: RequestResponse<DirectMessageCodec>) -> Self{
        Self {
            request_response,
            in_progress_rr: HashMap::default(),
            failed_rr: VecDeque::default(),
            out_event_queue: Vec::default()
        }
    }

   pub fn add_address(&mut self, peer_id: &PeerId, address: Multiaddr) {
        self.request_response.add_address(peer_id, address)
   }

    /// send request
    pub fn send_rr(&mut self, req: DMRequest) {
        let new_request = self
            .request_response
            .send_request(&req.peer_id, DirectMessageRequest(req.data.clone()));
        self.in_progress_rr.insert(new_request, req);

    }

    /// Add a direct request for a given peer
    pub fn add_direct_request(&mut self, req: DMRequest) {
        let request_id = self
            .request_response
            .send_request(&req.peer_id, DirectMessageRequest(req.data.clone()));
        info!("direct message request with id {:?}", request_id);

        self.in_progress_rr.insert(request_id, req);
    }

    /// Add a direct response for a channel
    pub fn add_direct_response(
        &mut self,
        chan: ResponseChannel<DirectMessageResponse>,
        msg: Vec<u8>,
    ) {
        let res = self
            .request_response
            .send_response(chan, DirectMessageResponse(msg));
        if let Err(e) = res {
            error!("Error replying to direct message. {:?}", e);
        }
    }

}
