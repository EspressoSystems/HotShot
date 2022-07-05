use crate::network::NetworkEvent;

use futures::channel::oneshot::Sender;
use libp2p::{
    gossipsub::IdentTopic as Topic,
    identify::{Identify, IdentifyEvent, IdentifyInfo},
    request_response::ResponseChannel,
    swarm::{
        NetworkBehaviour, NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters,
    },
    Multiaddr, NetworkBehaviour, PeerId,
};
use phaselock_utils::subscribable_rwlock::SubscribableRwLock;

use std::{
    collections::HashSet,
    num::NonZeroUsize,
    sync::Arc,
    task::{Context, Poll},
};
use tracing::{debug, error};

use super::{
    behaviours::{
        dht::{DHTBehaviour, DHTEvent, KadPutQuery},
        direct_message::{DMBehaviour, DMEvent, DMRequest},
        direct_message_codec::DirectMessageResponse,
        exponential_backoff::ExponentialBackoff,
        gossip::{GossipBehaviour, GossipEvent},
    },
    ConnectionData,
};

pub(crate) const NUM_REPLICATED_TO_TRUST: usize = 2;

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
    pub request_response: DMBehaviour,

    /// connection data
    #[behaviour(ignore)]
    connection_data: Arc<SubscribableRwLock<ConnectionData>>,

    /// set of events to send to UI
    #[behaviour(ignore)]
    #[debug(skip)]
    client_event_queue: Vec<NetworkEvent>,

    /// Addresses to connect to at init
    #[behaviour(ignore)]
    pub to_connect_addrs: HashSet<Multiaddr>,
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
        self.dht.add_address(peer_id, address);
        // self.request_response.add_address(peer_id, address)
    }
}

/// Gossip functions
impl NetworkDef {
    /// Publish a given gossip
    pub fn publish_gossip(&mut self, topic: Topic, contents: Vec<u8>) {
        self.gossipsub.publish_gossip(topic, contents);
    }

    /// Subscribe to a given topic
    pub fn subscribe_gossip(&mut self, t: &str) {
        self.gossipsub.subscribe_gossip(t);
    }

    /// Unsubscribe from a given topic
    pub fn unsubscribe_gossip(&mut self, t: &str) {
        self.gossipsub.unsubscribe_gossip(t);
    }
}

/// DHT functions
impl NetworkDef {
    /// Publish a key/value to the kv store.
    /// Once replicated upon all nodes, the caller is notified over
    /// `chan`. If there is an error, a [`DHTError`] is
    /// sent instead.
    pub fn put_record(&mut self, query: KadPutQuery) {
        self.dht.put_record(query);
    }

    /// Retrieve a value for a key from the DHT.
    /// Value (serialized) is sent over `chan`, and if a value is not found,
    /// a [`DHTError`] is sent instead.
    pub fn get_record(&mut self, key: Vec<u8>, chan: Sender<Vec<u8>>, factor: NonZeroUsize) {
        self.dht
            .get_record(key, chan, factor, ExponentialBackoff::default());
    }
}

/// Request/response functions
impl NetworkDef {
    /// Add a direct request for a given peer
    pub fn add_direct_request(&mut self, peer_id: PeerId, data: Vec<u8>) {
        let request = DMRequest {
            peer_id,
            data,
            backoff: ExponentialBackoff::default(),
        };
        self.request_response.add_direct_request(request);
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
            GossipEvent::GossipMsg(data, topic) => {
                self.client_event_queue
                    .push(NetworkEvent::GossipMsg(data, topic));
            }
        }
    }
}

impl NetworkBehaviourEventProcess<DHTEvent> for NetworkDef {
    fn inject_event(&mut self, event: DHTEvent) {
        match event {
            DHTEvent::IsBootstrapped => {
                self.client_event_queue.push(NetworkEvent::IsBootstrapped);
            }
        }
    }
}

impl NetworkBehaviourEventProcess<IdentifyEvent> for NetworkDef {
    fn inject_event(&mut self, event: IdentifyEvent) {
        // NOTE feed identified peers into kademlia's routing table for peer discovery.
        if let IdentifyEvent::Received {
            peer_id,
            info:
                IdentifyInfo {
                    listen_addrs,
                    protocols: _,
                    public_key: _,
                    protocol_version: _,
                    agent_version: _,
                    observed_addr: _,
                },
        } = event
        {
            // NOTE in practice, we will want to NOT include this. E.g. only DNS/non localhost IPs
            for addr in listen_addrs {
                // if addr.to_string().contains("127.0.0.1"){
                error!("ADDING ADDRESS {:?} TO DHT", addr);
                self.dht.add_address(&peer_id, addr.clone());
                // self.request_response.add_address(&peer_id, addr.clone());
                // }
            }
            // self.connection_data.modify(|s| {
            //     s.known_peers.insert(peer_id);
            // });
        }
    }
}

impl NetworkBehaviourEventProcess<DMEvent> for NetworkDef {
    fn inject_event(&mut self, event: DMEvent) {
        let out_event = match event {
            DMEvent::DirectRequest(data, pid, chan) => {
                error!("EMITTING REQUEST EVENT");
                NetworkEvent::DirectRequest(data, pid, chan)
            }
            DMEvent::DirectResponse(data, pid) => {
                error!("EMITTING RESPONSE EVENT");
                NetworkEvent::DirectResponse(data, pid)
            }
        };

        error!("EMITTING EVENT");
        self.client_event_queue.push(out_event);
    }
}
