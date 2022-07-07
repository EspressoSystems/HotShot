use std::{
    collections::{HashSet, VecDeque},
    task::Poll,
};

use libp2p::{
    core::{connection::ConnectionId, transport::ListenerId},
    gossipsub::{Gossipsub, GossipsubEvent, IdentTopic, TopicHash},
    swarm::{
        ConnectionHandler, IntoConnectionHandler, NetworkBehaviour, NetworkBehaviourAction,
        NetworkBehaviourEventProcess, PollParameters,
    },
    PeerId,
};
use tracing::{error, warn};

use super::exponential_backoff::ExponentialBackoff;

/// wrapper metadata around libp2p's gossip protocol
pub struct GossipBehaviour {
    /// Timeout trackidng when to retry gossip
    backoff: ExponentialBackoff,
    /// The in progress gossip queries
    in_progress_gossip: VecDeque<(IdentTopic, Vec<u8>)>,
    /// The gossip behaviour
    gossipsub: Gossipsub,
    /// Output events to parent behavioru
    out_event_queue: Vec<GossipEvent>,
    /// Set of topics we are subscribed to
    subscribed_topics: HashSet<String>,
}

/// Output event
pub enum GossipEvent {
    /// We received a gossip
    GossipMsg(Vec<u8>, TopicHash),
}

impl NetworkBehaviourEventProcess<GossipsubEvent> for GossipBehaviour {
    fn inject_event(&mut self, event: GossipsubEvent) {
        match event {
            GossipsubEvent::Message { message, .. } => {
                // if we get an event from the gossipsub behaviour, push it
                // onto the event queue (which will get popped during poll)
                // and propagated back to the overall behaviour
                self.out_event_queue
                    .push(GossipEvent::GossipMsg(message.data, message.topic));
            }
            GossipsubEvent::Subscribed { topic, .. } => {
                error!("subscribed to topic {}", topic);
            }
            GossipsubEvent::Unsubscribed { topic, .. } => {
                error!("unsubscribed to topic {}", topic);
            }
            GossipsubEvent::GossipsubNotSupported { peer_id } => {
                error!("gossipsub not supported on {}!", peer_id);
            }
        }
    }
}

impl NetworkBehaviour for GossipBehaviour {
    type ConnectionHandler = <Gossipsub as NetworkBehaviour>::ConnectionHandler;

    type OutEvent = GossipEvent;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        self.gossipsub.new_handler()
    }

    fn inject_event(
        &mut self,
        peer_id: PeerId,
        connection: ConnectionId,
        event: <<Self::ConnectionHandler as IntoConnectionHandler>::Handler as ConnectionHandler>::OutEvent,
    ) {
        // pass event GENERATED by handler from swarm to gossipsub
        self.gossipsub.inject_event(peer_id, connection, event);
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
        // retry sending shit
        if self.backoff.is_expired() {
            let published = self.drain_publish_gossips();
            self.backoff.start_next(published);
        }
        if let Poll::Ready(ready) = NetworkBehaviour::poll(&mut self.gossipsub, cx, params) {
            match ready {
                NetworkBehaviourAction::GenerateEvent(e) => {
                    // add event to event queue which will be subsequently popped off.
                    NetworkBehaviourEventProcess::inject_event(self, e);
                }
                NetworkBehaviourAction::Dial { opts, handler } => {
                    return Poll::Ready(NetworkBehaviourAction::Dial { opts, handler });
                }
                NetworkBehaviourAction::NotifyHandler {
                    peer_id,
                    handler,
                    event,
                } => {
                    return Poll::Ready(NetworkBehaviourAction::NotifyHandler {
                        peer_id,
                        handler,
                        event,
                    });
                }
                NetworkBehaviourAction::ReportObservedAddr { address, score } => {
                    return Poll::Ready(NetworkBehaviourAction::ReportObservedAddr {
                        address,
                        score,
                    });
                }
                NetworkBehaviourAction::CloseConnection {
                    peer_id,
                    connection,
                } => {
                    return Poll::Ready(NetworkBehaviourAction::CloseConnection {
                        peer_id,
                        connection,
                    });
                }
            }
        }
        if !self.out_event_queue.is_empty() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                self.out_event_queue.remove(0),
            ));
        }
        Poll::Pending
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<libp2p::Multiaddr> {
        self.gossipsub.addresses_of_peer(peer_id)
    }

    fn inject_connection_established(
        &mut self,
        peer_id: &PeerId,
        connection_id: &libp2p::core::connection::ConnectionId,
        endpoint: &libp2p::core::ConnectedPoint,
        failed_addresses: Option<&Vec<libp2p::Multiaddr>>,
        other_established: usize,
    ) {
        self.gossipsub.inject_connection_established(
            peer_id,
            connection_id,
            endpoint,
            failed_addresses,
            other_established,
        );
    }

    fn inject_connection_closed(
        &mut self,
        pid: &PeerId,
        cid: &libp2p::core::connection::ConnectionId,
        cp: &libp2p::core::ConnectedPoint,
        handler: <Self::ConnectionHandler as libp2p::swarm::IntoConnectionHandler>::Handler,
        remaining_established: usize,
    ) {
        self.gossipsub
            .inject_connection_closed(pid, cid, cp, handler, remaining_established);
    }

    fn inject_address_change(
        &mut self,
        pid: &PeerId,
        cid: &libp2p::core::connection::ConnectionId,
        old: &libp2p::core::ConnectedPoint,
        new: &libp2p::core::ConnectedPoint,
    ) {
        self.gossipsub.inject_address_change(pid, cid, old, new);
    }

    fn inject_dial_failure(
        &mut self,
        peer_id: Option<PeerId>,
        handler: Self::ConnectionHandler,
        error: &libp2p::swarm::DialError,
    ) {
        self.gossipsub.inject_dial_failure(peer_id, handler, error);
    }

    fn inject_listen_failure(
        &mut self,
        local_addr: &libp2p::Multiaddr,
        send_back_addr: &libp2p::Multiaddr,
        handler: Self::ConnectionHandler,
    ) {
        self.gossipsub
            .inject_listen_failure(local_addr, send_back_addr, handler);
    }

    fn inject_new_listener(&mut self, id: ListenerId) {
        self.gossipsub.inject_new_listener(id);
    }

    fn inject_new_listen_addr(&mut self, id: ListenerId, addr: &libp2p::Multiaddr) {
        self.gossipsub.inject_new_listen_addr(id, addr);
    }

    fn inject_expired_listen_addr(&mut self, id: ListenerId, addr: &libp2p::Multiaddr) {
        self.gossipsub.inject_expired_listen_addr(id, addr);
    }

    fn inject_listener_error(&mut self, id: ListenerId, err: &(dyn std::error::Error + 'static)) {
        self.gossipsub.inject_listener_error(id, err);
    }

    fn inject_listener_closed(&mut self, id: ListenerId, reason: Result<(), &std::io::Error>) {
        self.gossipsub.inject_listener_closed(id, reason);
    }

    fn inject_new_external_addr(&mut self, addr: &libp2p::Multiaddr) {
        self.gossipsub.inject_new_external_addr(addr);
    }

    fn inject_expired_external_addr(&mut self, addr: &libp2p::Multiaddr) {
        self.gossipsub.inject_expired_external_addr(addr);
    }
}

impl GossipBehaviour {
    /// Create new gossip behavioru based on gossipsub
    pub fn new(gossipsub: Gossipsub) -> Self {
        Self {
            backoff: ExponentialBackoff::default(),
            in_progress_gossip: VecDeque::default(),
            gossipsub,
            out_event_queue: Vec::default(),
            subscribed_topics: HashSet::default(),
        }
    }

    /// Publish a given gossip
    pub fn publish_gossip(&mut self, topic: IdentTopic, contents: Vec<u8>) {
        // TODO might be better just to push this into the queue and not try to
        // send here
        let res = self.gossipsub.publish(topic.clone(), contents.clone());
        if res.is_err() {
            error!("error publishing gossip message {:?}", res);
            self.in_progress_gossip.push_back((topic, contents));
        }
    }

    /// Subscribe to a given topic
    pub fn subscribe_gossip(&mut self, t: &str) {
        if self.subscribed_topics.contains(t) {
            warn!(
                "tried to subscribe to already subscribed topic {:?}. Noop.",
                t
            );
        } else if self.gossipsub.subscribe(&IdentTopic::new(t)).is_err() {
            error!("error subscribing to topic {}", t);
        } else {
            error!("subscribed req to {:?}", t);
            self.subscribed_topics.insert(t.to_string());
        }
    }

    /// Unsubscribe from a given topic
    pub fn unsubscribe_gossip(&mut self, t: &str) {
        if self.subscribed_topics.contains(t) {
            if self.gossipsub.unsubscribe(&IdentTopic::new(t)).is_err() {
                error!("error unsubscribing to topic {}", t);
            } else {
                self.subscribed_topics.remove(t);
            }
        } else {
            warn!("tried to unsubscribe to untracked topic {:?}. Noop.", t);
        }
    }

    /// Attempt to drain the internal gossip list, publishing each gossip
    pub fn drain_publish_gossips(&mut self) -> bool {
        let mut r_val = true;

        while let Some((topic, contents)) = self.in_progress_gossip.pop_front() {
            let res = self.gossipsub.publish(topic.clone(), contents.clone());
            if res.is_err() {
                self.in_progress_gossip.push_back((topic, contents));
                r_val = false;
                break;
            }
        }
        r_val
    }
}
