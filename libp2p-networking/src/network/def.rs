use crate::{
    // direct_message::{DirectMessageCodec, DirectMessageRequest, DirectMessageResponse},
    network::{NetworkEvent},
};

use futures::channel::oneshot::Sender;
use libp2p::{
    gossipsub::{IdentTopic as Topic},
    identify::{Identify, IdentifyEvent, IdentifyInfo},
    request_response::{
        ResponseChannel,
    },
    swarm::{
        NetworkBehaviour, NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters,
    },
    Multiaddr, NetworkBehaviour, PeerId,
};
use phaselock_utils::subscribable_rwlock::{ReadView, SubscribableRwLock};

use std::{
    collections::{HashSet},
    num::NonZeroUsize,
    sync::Arc,
    task::{Context, Poll},
};
use tracing::{debug};

use super::{ConnectionData, behaviours::{direct_message_codec::{DirectMessageResponse}, gossip::{GossipBehaviour, GossipEvent}, direct_message::{DMBehaviour, DMEvent, DMRequest}, dht::{DHTBehaviour, DHTEvent, KadPutQuery}, exponential_backoff::ExponentialBackoff}};

pub(crate) const NUM_REPLICATED_TO_TRUST: usize = 2;
const MAX_DHT_QUERY_SIZE: usize = 5;

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
    /// <https://github.com/libp2p/rust-libp2p/issues/2398>
    #[debug(skip)]
    gossipsub: GossipBehaviour,

    /// purpose: peer routing
    #[debug(skip)]
    pub dht: DHTBehaviour,

    /// purpose: peer discovery
    #[debug(skip)]
    identify: Identify,

    /// purpose: directly messaging peer
    #[debug(skip)]
    request_response: DMBehaviour,

    /// connection data
    #[behaviour(ignore)]
    connection_data: Arc<SubscribableRwLock<ConnectionData>>,

    /// set of events to send to UI
    #[behaviour(ignore)]
    #[debug(skip)]
    client_event_queue: Vec<NetworkEvent>,

    // track unknown addrs
    #[behaviour(ignore)]
    unknown_addrs: HashSet<Multiaddr>,

    #[behaviour(ignore)]
    pub to_connect_addrs: HashSet<Multiaddr>,

    #[behaviour(ignore)]
    pub to_connect_peers: HashSet<PeerId>,
}



impl NetworkDef {
    /// Returns a reference to the internal `ConnectionData`
    pub fn connection_data(&self) -> Arc<SubscribableRwLock<ConnectionData>> {
        self.connection_data.clone()
    }

    /// Create a new instance of a `NetworkDef`
    pub fn new(
        gossipsub: GossipBehaviour,
        dht: DHTBehaviour,
        identify: Identify,
        request_response: DMBehaviour,
        _pruning_enabled: bool,
        ignored_peers: HashSet<PeerId>,
        to_connect_addrs: HashSet<Multiaddr>,
    ) -> NetworkDef {
        Self {
            gossipsub,
            dht,
            identify,
            request_response,
            connection_data: Arc::new(SubscribableRwLock::new(ConnectionData {
                connected_peers: HashSet::new(),
                connecting_peers: HashSet::new(),
                known_peers: HashSet::new(),
                ignored_peers,
            })),
            client_event_queue: Vec::new(),
            unknown_addrs: HashSet::new(),
            to_connect_peers: HashSet::default(),
            to_connect_addrs,
        }
    }

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<NetworkEvent, <Self as NetworkBehaviour>::ConnectionHandler>>
    {
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

/// Address functions
impl NetworkDef {
    /// Add an address
    pub fn add_address(&mut self, peer_id: &PeerId, address: Multiaddr) {
        self.dht.add_address(peer_id, address.clone());
        self.request_response.add_address(peer_id, address)
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
        self.connection_data.modify(|s| {
            s.connected_peers.insert(peer_id);
            s.connecting_peers.remove(&peer_id);
        });
    }

    /// Remove a connected peer
    pub fn remove_connected_peer(&mut self, peer_id: PeerId) {
        self.connection_data.modify(|s| {
            s.connected_peers.remove(&peer_id);
        });
    }

    /// Get a list of the connected peers
    pub fn connected_peers(&self) -> HashSet<PeerId> {
        self.connection_data.cloned().connected_peers
    }

    /// Add a known peer
    pub fn add_known_peer(&mut self, peer_id: PeerId) {
        self.connection_data.modify(|s| {
            s.known_peers.insert(peer_id);
        });
    }

    /// Get a list of the known peers
    pub fn known_peers(&self) -> HashSet<PeerId> {
        self.connection_data.cloned().known_peers
    }

    /// Add a connecting peer
    pub fn add_connecting_peer(&mut self, a_peer: PeerId) {
        self.connection_data.modify(|s| {
            s.connecting_peers.insert(a_peer);
        });
    }

    /// Remove a peer, both connecting and connected
    pub fn remove_peer(&mut self, peer_id: PeerId) {
        self.connection_data.modify(|s| {
            s.connected_peers.remove(&peer_id);
            s.connecting_peers.remove(&peer_id);
        });
    }

    /// Get a list of peers, both connecting and connected
    pub fn get_peers(&self) -> HashSet<PeerId> {
        let cd = self.connection_data.cloned();
        cd.connecting_peers
            .union(&cd.connected_peers)
            .copied()
            .collect()
    }

    /// Start a query for the closest peers
    pub fn query_closest_peers(&mut self, random_peer: PeerId) {
        self.dht.query_closest_peers(random_peer);
    }

    /// Add a list of peers to the ignored peers list
    pub fn extend_ignored_peers(&mut self, peers: Vec<PeerId>) {
        self.connection_data
            .modify(|s| s.ignored_peers.extend(peers.into_iter()));
    }
}

/// Gossip functions
impl NetworkDef {
    /// Publish a given gossip
    pub fn publish_gossip(&mut self, topic: Topic, contents: Vec<u8>) {
        // TODO might be better just to push this into the queue and not try to
        // send here
        let _res = self.gossipsub.publish_gossip(topic.clone(), contents.clone());
    }

    /// Subscribe to a given topic
    pub fn subscribe_gossip(&mut self, t: &str) {
        self.gossipsub.subscribe_gossip(t)
    }

    /// Unsubscribe from a given topic
    pub fn unsubscribe_gossip(&mut self, t: &str) {
        self.gossipsub.unsubscribe_gossip(t)
    }
}

/// DHT functions
impl NetworkDef {
    /// Publish a key/value to the kv store.
    /// Once replicated upon all nodes, the caller is notified over
    /// `chan`. If there is an error, a [`DHTError`] is
    /// sent instead.
    pub fn put_record(&mut self, query: KadPutQuery) {
        self.dht.put_record(query)
    }

    /// Retrieve a value for a key from the DHT.
    /// Value (serialized) is sent over `chan`, and if a value is not found,
    /// a [`DHTError`] is sent instead.
    pub fn get_record(&mut self, key: Vec<u8>, chan: Sender<Vec<u8>>, factor: NonZeroUsize) {
        self.dht.get_record(key, chan, factor, ExponentialBackoff::default())
    }
}

/// Request/response functions
impl NetworkDef {
    /// Add a direct request for a given peer
    pub fn add_direct_request(&mut self, peer_id: PeerId, data: Vec<u8>) {
        let request = DMRequest {
            peer_id,
            data,
            backoff: ExponentialBackoff::default()
        };
        self.request_response.add_direct_request(request)
    }

    /// Add a direct response for a channel
    pub fn add_direct_response(
        &mut self,
        chan: ResponseChannel<DirectMessageResponse>,
        msg: Vec<u8>,
    ) {
        self.request_response.add_direct_response(chan, msg);
    }
}

impl NetworkBehaviourEventProcess<GossipEvent> for NetworkDef {
    fn inject_event(&mut self, event: GossipEvent) {
        match event {
            GossipEvent::GossipMsg( data, topic ) => {
                self.client_event_queue
                    .push(NetworkEvent::GossipMsg(data, topic));
            }
        }
    }
}

impl NetworkBehaviourEventProcess<DHTEvent> for NetworkDef {
    fn inject_event(&mut self, _event: DHTEvent) {
        // no events really
    }
}

impl NetworkBehaviourEventProcess<IdentifyEvent> for NetworkDef {
    fn inject_event(&mut self, event: IdentifyEvent) {
        // NOTE feed identified peers into kademlia's routing table for peer discovery.
        if let IdentifyEvent::Received { peer_id, info: IdentifyInfo { listen_addrs, protocols: _, .. } } = event {
            for addr in listen_addrs {
                self.dht.add_address(&peer_id, addr.clone());
            }
            self.connection_data.modify(|s| {
                s.known_peers.insert(peer_id);
            });
        }
    }
}

impl NetworkBehaviourEventProcess<DMEvent>
    for NetworkDef
{
    fn inject_event(
        &mut self,
        _event: DMEvent,
    ) {
    }
}

