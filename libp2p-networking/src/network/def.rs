use crate::{
    direct_message::{DirectMessageCodec, DirectMessageRequest, DirectMessageResponse},
    network::{NetworkError, NetworkEvent},
};
use libp2p::{
    gossipsub::{Gossipsub, GossipsubEvent, IdentTopic as Topic},
    identify::{Identify, IdentifyEvent},
    kad::{store::MemoryStore, GetClosestPeersOk, Kademlia, KademliaEvent, QueryResult},
    request_response::{
        RequestId, RequestResponse, RequestResponseEvent, RequestResponseMessage, ResponseChannel,
    },
    swarm::{
        NetworkBehaviour, NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters,
    },
    Multiaddr, NetworkBehaviour, PeerId,
};
use rand::{prelude::IteratorRandom, thread_rng};
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    task::{Context, Poll},
};
use tracing::{debug, error, info};

/// Overarching network behaviour performing:
/// - network topology discovoery
/// - direct messaging
/// - p2p broadcast
/// - connection management
#[derive(NetworkBehaviour, custom_debug::Debug)]
#[behaviour(out_event = "NetworkEvent", poll_method = "poll", event_process = true)]
pub struct NetworkDef {
    /// purpose: broadcasting messages to many peers
    /// NOTE gossipsub works ONLY for sharing messsages right now
    /// in the future it may be able to do peer discovery and routing
    /// <`https://github.com/libp2p/rust-libp2p/issues/2398>
    #[debug(skip)]
    gossipsub: Gossipsub,

    /// purpose: peer routing
    #[debug(skip)]
    kadem: Kademlia<MemoryStore>,

    /// purpose: peer discovery
    #[debug(skip)]
    identify: Identify,

    /// purpose: directly messaging peer
    #[debug(skip)]
    request_response: RequestResponse<DirectMessageCodec>,

    /// if the node has been bootstrapped into the kademlia network
    #[behaviour(ignore)]
    bootstrap_state: BootstrapState,
    // TODO separate out into ConnectionData struct
    /// set of connected peers
    #[behaviour(ignore)]
    connected_peers: HashSet<PeerId>,
    // TODO replace this with a set of queryids
    /// set of currently connecting peers
    #[behaviour(ignore)]
    connecting_peers: HashSet<PeerId>,
    /// set of peers that were at one point connected
    #[behaviour(ignore)]
    known_peers: HashSet<PeerId>,
    /// set of events to send to UI
    #[behaviour(ignore)]
    #[debug(skip)]
    client_event_queue: Vec<NetworkEvent>,
    /// whether or not to prune nodes
    #[behaviour(ignore)]
    pruning_enabled: bool,
    /// track in progress request-response
    #[behaviour(ignore)]
    in_progress_rr: HashMap<RequestId, (Vec<u8>, PeerId)>,
    /// track gossip messages that failed to send and we should send later on
    #[behaviour(ignore)]
    in_progress_gossip: Vec<(Topic, Vec<u8>)>,
    // track unknown addrs
    #[behaviour(ignore)]
    unknown_addrs: HashSet<Multiaddr>,
    /// peers we ignore (mainly here for conductor usecase)
    #[behaviour(ignore)]
    ignored_peers: HashSet<PeerId>,
}

impl NetworkDef {
    /// Create a new instance of a `NetworkDef`
    pub fn new(
        gossipsub: Gossipsub,
        kadem: Kademlia<MemoryStore>,
        identify: Identify,
        request_response: RequestResponse<DirectMessageCodec>,
        pruning_enabled: bool,
        ignored_peers: HashSet<PeerId>,
    ) -> NetworkDef {
        Self {
            gossipsub,
            kadem,
            identify,
            request_response,
            connected_peers: HashSet::new(),
            connecting_peers: HashSet::new(),
            known_peers: HashSet::new(),
            client_event_queue: Vec::new(),
            bootstrap_state: BootstrapState::NotStarted,
            pruning_enabled,
            in_progress_rr: HashMap::new(),
            in_progress_gossip: Vec::new(),
            unknown_addrs: HashSet::new(),
            // currently only functionality is to "not prune" these nodes
            ignored_peers,
        }
    }

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<NetworkEvent, <Self as NetworkBehaviour>::ConnectionHandler>>
    {
        // TODO: Should we change this into a channel?

        // push events that must be relayed back to client onto queue
        // to be consumed by client event handler
        if !self.client_event_queue.is_empty() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                self.client_event_queue.remove(0),
            ));
        }

        Poll::Pending
    }
}

/// Bootstrap functions
impl NetworkDef {
    /// Bootstrap the network. Make sure at least 1 peer is known, by registering it with `.add_address`
    ///
    /// # Errors
    ///
    /// Will return a `NoKnownPeers` error when no known peers are defined
    pub fn bootstrap(&mut self) -> Result<(), NetworkError> {
        match self.kadem.bootstrap() {
            Ok(_) => Ok(()),
            Err(_) => Err(NetworkError::NoKnownPeers),
        }
    }

    /// Returns true if the bootstrap state is finished
    pub fn is_bootstrapped(&self) -> bool {
        self.bootstrap_state == BootstrapState::Finished
    }

    /// Returns true if the bootstrap state is not started
    pub fn should_bootstrap(&self) -> bool {
        self.bootstrap_state == BootstrapState::NotStarted
    }
}

/// Address functions
impl NetworkDef {
    /// Add an address
    pub fn add_address(&mut self, peer_id: &PeerId, address: Multiaddr) {
        self.unknown_addrs.remove(&address);
        self.kadem.add_address(peer_id, address);
    }

    /// Add an unknown address
    pub fn add_unknown_address(&mut self, addr: Multiaddr) {
        self.unknown_addrs.insert(addr);
    }

    /// Iter all unknown addresses
    pub fn iter_unknown_addressess(&self) -> impl Iterator<Item = Multiaddr> + '_ {
        self.unknown_addrs.iter().cloned()
    }
}

/// Peer functions
impl NetworkDef {
    /// Add a connected peer
    pub fn add_connected_peer(&mut self, peer_id: PeerId) {
        self.connected_peers.insert(peer_id);
        self.connecting_peers.remove(&peer_id);
    }

    /// Remove a connected peer
    pub fn remove_connected_peer(&mut self, peer_id: PeerId) {
        self.connected_peers.remove(&peer_id);
    }

    /// Get a list of the connected peers
    pub fn connected_peers(&self) -> HashSet<PeerId> {
        self.connected_peers.clone()
    }

    /// Add a known peer
    pub fn add_known_peer(&mut self, peer_id: PeerId) {
        self.known_peers.insert(peer_id);
    }

    /// Get a list of the known peers
    pub fn known_peers(&self) -> HashSet<PeerId> {
        self.known_peers.clone()
    }

    /// Add a connecting peer
    pub fn add_connecting_peer(&mut self, a_peer: PeerId) {
        self.connecting_peers.insert(a_peer);
    }

    /// Remove a peer, both connecting and connected
    pub fn remove_peer(&mut self, peer_id: PeerId) {
        self.connected_peers.remove(&peer_id);
        self.connecting_peers.remove(&peer_id);
    }

    /// Notify the event queue that there are new known peers
    pub fn notify_update_known_peers(&mut self) {
        let new_peers = self.known_peers.clone();
        self.client_event_queue
            .push(NetworkEvent::UpdateKnownPeers(new_peers));
    }

    /// Get a list of peers, both connecting and connected
    pub fn get_peers(&self) -> HashSet<PeerId> {
        self.connecting_peers
            .union(&self.connected_peers)
            .copied()
            .collect()
    }

    /// Return a list of peers to prune, based on given `max_num_peers`
    pub fn get_peers_to_prune(&self, max_num_peers: usize) -> Vec<PeerId> {
        if !self.is_bootstrapped()
            || !self.pruning_enabled
            || self.connected_peers.len() <= max_num_peers
        {
            return Vec::new();
        }
        let peers_to_rm = self.connected_peers.iter().copied().choose_multiple(
            &mut thread_rng(),
            self.connected_peers.len() - max_num_peers,
        );
        let rr_peers = self
            .in_progress_rr
            .iter()
            .map(|(_, (_, pid))| pid)
            .copied()
            .collect::<HashSet<_>>();
        let ignored_peers = self.ignored_peers.clone();
        let safe_peers = rr_peers.union(&ignored_peers).collect::<HashSet<_>>();
        peers_to_rm
            .into_iter()
            .filter(|p| !safe_peers.contains(p))
            .collect()
    }

    /// Toggle pruning
    pub fn toggle_pruning(&mut self, is_enabled: bool) {
        self.pruning_enabled = is_enabled;
    }

    /// Start a query for the closest peers
    pub fn query_closest_peers(&mut self, random_peer: PeerId) {
        self.kadem.get_closest_peers(random_peer);
    }

    /// Add a list of peers to the ignored peers list
    pub fn extend_ignored_peers(&mut self, peers: Vec<PeerId>) {
        self.ignored_peers.extend(peers.into_iter());
    }
}

/// Gossip functions
impl NetworkDef {
    /// Publish a given gossip
    pub fn publish_gossip(&mut self, topic: Topic, contents: Vec<u8>) {
        // TODO might be better just to push this into the queue and not try to
        // send here
        let res = self.gossipsub.publish(topic.clone(), contents.clone());
        if res.is_err() {
            self.in_progress_gossip.push((topic, contents));
        }
    }

    /// Subscribe to a given topic
    pub fn subscribe_gossip(&mut self, t: &str) {
        if self.gossipsub.subscribe(&Topic::new(t)).is_err() {
            error!("error subscribing to topic {}", t);
        }
    }

    /// Unsubscribe from a given topic
    pub fn unsubscribe_gossip(&mut self, t: &str) {
        if self.gossipsub.unsubscribe(&Topic::new(t)).is_err() {
            error!("error unsubscribing to topic {}", t);
        }
    }

    /// Attempt to drain the internal gossip list, publishing each gossip
    pub fn drain_publish_gossips(&mut self) {
        let mut num_sent = 0;
        for (topic, contents) in self.in_progress_gossip.as_slice() {
            let res = self.gossipsub.publish(topic.clone(), contents.clone());
            if res.is_err() {
                break;
            }
            num_sent += 1;
        }
        self.in_progress_gossip = self.in_progress_gossip[num_sent..].into();
    }
}

/// Request/response functions
impl NetworkDef {
    /// Add a direct request for a given peer
    pub fn add_direct_request(&mut self, pid: PeerId, msg: Vec<u8>) {
        let request_id = self
            .request_response
            .send_request(&pid, DirectMessageRequest(msg.clone()));
        self.in_progress_rr.insert(request_id, (msg, pid));
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

impl NetworkBehaviourEventProcess<GossipsubEvent> for NetworkDef {
    fn inject_event(&mut self, event: GossipsubEvent) {
        if let GossipsubEvent::Message { message, .. } = event {
            self.client_event_queue
                .push(NetworkEvent::GossipMsg(message.data));
        }
    }
}

impl NetworkBehaviourEventProcess<KademliaEvent> for NetworkDef {
    fn inject_event(&mut self, event: KademliaEvent) {
        info!(?event, "kadem event");
        match event {
            KademliaEvent::OutboundQueryCompleted {
                result: QueryResult::Bootstrap(Ok(_)),
                ..
            } => {
                // we're bootstrapped
                // don't bootstrap again
                self.bootstrap_state = BootstrapState::Started;
            }
            KademliaEvent::OutboundQueryCompleted {
                result: QueryResult::Bootstrap(Err(_)),
                ..
            } => {
                // bootstrap failed. try again
                self.bootstrap_state = BootstrapState::NotStarted;
            }
            KademliaEvent::OutboundQueryCompleted {
                result: QueryResult::GetClosestPeers(Ok(GetClosestPeersOk { peers, .. })),
                ..
            } => {
                for peer in peers {
                    self.known_peers.insert(peer);
                }
                self.client_event_queue
                    .push(NetworkEvent::UpdateKnownPeers(self.known_peers.clone()));
            }
            KademliaEvent::RoutingUpdated { peer, .. } => {
                self.known_peers.insert(peer);
                self.client_event_queue
                    .push(NetworkEvent::UpdateKnownPeers(self.known_peers.clone()));
            }
            _ => {
                debug!("Not handled");
            }
        }
    }
}

impl NetworkBehaviourEventProcess<IdentifyEvent> for NetworkDef {
    fn inject_event(&mut self, event: IdentifyEvent) {
        if let IdentifyEvent::Received { peer_id, info, .. } = event {
            for addr in info.listen_addrs {
                self.kadem.add_address(&peer_id, addr.clone());
            }
            self.known_peers.insert(peer_id);
            self.client_event_queue
                .push(NetworkEvent::UpdateKnownPeers(self.known_peers.clone()));
        }
    }
}

impl NetworkBehaviourEventProcess<RequestResponseEvent<DirectMessageRequest, DirectMessageResponse>>
    for NetworkDef
{
    fn inject_event(
        &mut self,
        event: RequestResponseEvent<DirectMessageRequest, DirectMessageResponse>,
    ) {
        match event {
            RequestResponseEvent::InboundFailure {
                peer, request_id, ..
            }
            | RequestResponseEvent::OutboundFailure {
                peer, request_id, ..
            } => {
                if let Some((request, _)) = self.in_progress_rr.get(&request_id).cloned() {
                    let new_request = self
                        .request_response
                        .send_request(&peer, DirectMessageRequest(request.clone()));
                    self.in_progress_rr.remove(&request_id);
                    self.in_progress_rr.insert(new_request, (request, peer));
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
                    self.client_event_queue
                        .push(NetworkEvent::DirectRequest(msg, peer, channel));
                }
                RequestResponseMessage::Response {
                    request_id,
                    response: DirectMessageResponse(msg),
                } => {
                    if let Some((_, peer_id)) = self.in_progress_rr.remove(&request_id) {
                        self.client_event_queue
                            .push(NetworkEvent::DirectResponse(msg, peer_id));
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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum BootstrapState {
    NotStarted,
    Started,
    Finished,
}
