use crate::{
    direct_message::{DirectMessageCodec, DirectMessageRequest, DirectMessageResponse},
    network::{NetworkError, NetworkEvent},
};
use either::Either;
use futures::channel::oneshot::Sender;
use libp2p::{
    gossipsub::{Gossipsub, GossipsubEvent, IdentTopic as Topic},
    identify::{Identify, IdentifyEvent},
    kad::{
        store::MemoryStore, GetClosestPeersOk, GetRecordOk, GetRecordResult, Kademlia,
        KademliaEvent, QueryId, QueryResult, Quorum, Record,
    },
    request_response::{
        RequestId, RequestResponse, RequestResponseEvent, RequestResponseMessage, ResponseChannel,
    },
    swarm::{
        NetworkBehaviour, NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters,
    },
    Multiaddr, NetworkBehaviour, PeerId,
};
use phaselock_utils::subscribable_rwlock::{ReadView, SubscribableRwLock};
use rand::{prelude::IteratorRandom, thread_rng};
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    num::NonZeroUsize,
    sync::Arc,
    task::{Context, Poll},
};
use tracing::{debug, error, info, warn};

use super::ConnectionData;

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

    /// connection data
    #[behaviour(ignore)]
    connection_data: Arc<SubscribableRwLock<ConnectionData>>,

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
    /// query id -> (notify_channel to client, quorum size, key)
    #[behaviour(ignore)]
    in_progress_get_record_queries: HashMap<QueryId, KadGetQuery>,
    /// query_id -> (notify_channel to client)
    #[behaviour(ignore)]
    in_progress_put_record_queries: HashMap<Either<usize, QueryId>, KadPutQuery>,
    #[behaviour(ignore)]
    in_progress_put_record_uid: usize,
    #[behaviour(ignore)]
    peer_discovery_in_progress: bool,
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
            connection_data: Arc::default(),
            client_event_queue: Vec::new(),
            bootstrap_state: BootstrapState::NotStarted,
            pruning_enabled,
            in_progress_rr: HashMap::new(),
            in_progress_gossip: Vec::new(),
            unknown_addrs: HashSet::new(),
            in_progress_get_record_queries: HashMap::new(),
            in_progress_put_record_queries: HashMap::new(),
            in_progress_put_record_uid: 0,
            // currently only functionality is to "not prune" these nodes
            ignored_peers,
            peer_discovery_in_progress: false,
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
        if self.bootstrap_state == BootstrapState::NotStarted {
            if self.kadem.bootstrap().is_ok() {
                self.bootstrap_state = BootstrapState::Started;
                return Ok(());
            }
            error!("Failed to initiate bootstrap! Not enough peers.");
            return Err(NetworkError::NoKnownPeers);
        }
        info!("already bootstrapped");
        Ok(())
    }

    /// Returns true if the bootstrap state is finished
    pub fn is_bootstrapped(&self) -> bool {
        self.bootstrap_state == BootstrapState::Finished
    }

    /// Returns true if a peer discovery query is in progress
    pub fn is_discovering_peers(&self) -> bool {
        self.peer_discovery_in_progress
    }

    /// Returns true if the bootstrap state is not started
    pub fn should_bootstrap(&self) -> bool {
        self.bootstrap_state == BootstrapState::NotStarted
    }

    /// Returns a reference to the internal `ConnectionData`
    pub fn connection_data(&self) -> Arc<SubscribableRwLock<ConnectionData>> {
        self.connection_data.clone()
    }

    /// Mark that we're now discovering peers
    pub fn discovering_peers_off(&mut self) {
        self.peer_discovery_in_progress = false;
    }

    /// Mark that we're no longer discovering peers
    pub fn discovering_peers_on(&mut self) {
        self.peer_discovery_in_progress = true;
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

    /// Return a list of peers to prune, based on given `max_num_peers`
    pub fn get_peers_to_prune(&self, max_num_peers: usize) -> Vec<PeerId> {
        let cd = self.connection_data().cloned();
        if !self.is_bootstrapped()
            || !self.pruning_enabled
            || cd.connected_peers.len() <= max_num_peers
        {
            return Vec::new();
        }
        let peers_to_rm = cd
            .connected_peers
            .iter()
            .copied()
            .choose_multiple(&mut thread_rng(), cd.connected_peers.len() - max_num_peers);
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
            error!("error publishing gossip message {:?}", res);
            self.in_progress_gossip.push((topic, contents));
        }
    }

    /// Subscribe to a given topic
    pub fn subscribe_gossip(&mut self, t: &str) {
        if self.gossipsub.subscribe(&Topic::new(t)).is_err() {
            error!("error subscribing to topic {}", t);
        } else {
            info!("subscribed to {:?}", t);
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
        if self.is_bootstrapped() {
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

    /// attempt to put all unstarted put querys to DHT
    pub fn retry_put_dht(&mut self) {
        if self.is_bootstrapped() {
            // must collect and then iterate otherwise multiple
            // mutable reeferences to self
            let records: Vec<_> = self.in_progress_put_record_queries.drain().collect();
            info!("Retrying DHT put on: {:?}", records);
            for (k, v) in records {
                if v.progress == DHTProgress::NotStarted {
                    self.put_record(v);
                } else {
                    // skip over still in progress queries
                    self.in_progress_put_record_queries.insert(k, v);
                }
            }
        } else {
            warn!("DHT put request skipped because not bootstrapped.");
        }
    }
}

/// DHT functions
impl NetworkDef {
    /// Publish a key/value to the kv store.
    /// Once replicated upon all nodes, the caller is notified over
    /// `chan`. If there is an error, a [`DHTError`] is
    /// sent instead.
    pub fn put_record(&mut self, query: KadPutQuery) {
        let record = Record::new(query.key.clone(), query.value.clone());
        let uid = self.new_put_uid();

        // match self.kadem.put_record(record, Quorum::N(std::num::NonZeroUsize::new(5).unwrap())) {
        match self.kadem.put_record(record, Quorum::Majority) {
            Err(e) => {
                error!("Error publishing to DHT: {e:?}");
                self.in_progress_put_record_queries
                    .insert(Either::Left(uid), query);
            }
            Ok(qid) => {
                let query = KadPutQuery {
                    progress: DHTProgress::InProgress(qid),
                    ..query
                };
                self.in_progress_put_record_queries
                    .insert(Either::Right(qid), query);
            }
        }
    }

    /// generates new uid for put record query
    pub fn new_put_uid(&mut self) -> usize {
        let uid = self.in_progress_put_record_uid;
        self.in_progress_put_record_uid += 1;
        uid
    }

    /// Retrieve a value for a key from the DHT.
    /// Value (serialized) is sent over `chan`, and if a value is not found,
    /// a [`DHTError`] is sent instead.
    pub fn get_record(&mut self, key: Vec<u8>, chan: Sender<Vec<u8>>, factor: NonZeroUsize) {
        let qid = self.kadem.get_record(key.clone().into(), Quorum::N(factor));
        let query = KadGetQuery {
            progress: DHTProgress::InProgress(qid),
            notify: chan,
            num_replicas: factor,
            key,
        };
        self.in_progress_get_record_queries.insert(qid, query);
    }

    /// If we receive a get query, either send response/error to client,
    /// or initiate new get query to more nodes
    fn handle_get_query(&mut self, record_results: GetRecordResult, id: QueryId) {
        if let Some(KadGetQuery {
            progress,
            notify,
            num_replicas,
            key,
        }) = self.in_progress_get_record_queries.remove(&id)
        {
            // if channel has been dropped, cancel request
            if notify.is_canceled() {
                return;
            }

            match record_results {
                Ok(GetRecordOk {
                    records,
                    cache_candidates: _,
                }) => {
                    let mut results: HashMap<Vec<u8>, usize> = HashMap::new();

                    // count the number of records that agree on each value
                    for record in &records {
                        if record.record.key.to_vec() == key {
                            let value = record.record.value.clone();
                            let old_val: usize = results.get(&value.clone()).copied().unwrap_or(0);
                            results.insert(value, old_val + 1);
                        }
                    }
                    // agreement on two or more nodes => success
                    // NOTE case where multiple nodes agree on different
                    // values is not handles
                    if let Some((r, _)) = results.into_iter().find(|(_, v)| *v >= 2) {
                        if notify.send(r).is_err() {
                            error!("channel closed before get record request result could be sent");
                        }
                    }
                    // lack of replication => error
                    else if records.len() < NUM_REPLICATED_TO_TRUST {
                        error!("Get DHT: Record not replicated enough for {:?}! requerying with more nodes", progress);
                        self.get_record(key, notify, num_replicas);
                    }
                    // many records that don't match => disagreement
                    else if records.len() > MAX_DHT_QUERY_SIZE {
                        error!(
                            "Get DHT: Record disagreed upon; {:?}! requerying with more nodes",
                            progress
                        );
                        self.get_record(key, notify, num_replicas);
                    }
                    // disagreement => query more nodes
                    else {
                        // there is some internal disagreement.
                        // Initiate new query that hits more replicas
                        let new_factor =
                            NonZeroUsize::new(num_replicas.get() + 1).unwrap_or(num_replicas);

                        self.get_record(key, notify, new_factor);
                        error!("Get DHT: Internal disagreement for get dht request {:?}! requerying with more nodes", progress);
                    }
                }
                Err(e) => {
                    error!("Get DHT: Internal error {:?}. Requerying {:?}", e, progress);
                    self.get_record(key, notify, num_replicas);
                }
            };
        } else {
            error!("completed DHT query that is no longer tracked.");
        }
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
                self.bootstrap_state = BootstrapState::Finished;
            }
            KademliaEvent::OutboundQueryCompleted {
                result: QueryResult::Bootstrap(Err(_)),
                ..
            } => {
                // bootstrap failed. try again
                self.bootstrap_state = BootstrapState::NotStarted;
            }
            KademliaEvent::OutboundQueryCompleted {
                result: QueryResult::GetClosestPeers(r),
                ..
            } => {
                if let Ok(GetClosestPeersOk { peers, .. }) = r {
                    for peer in peers {
                        self.connection_data.modify(|s| {
                            s.known_peers.insert(peer);
                        });
                    }
                }
                self.peer_discovery_in_progress = false;
            }
            KademliaEvent::RoutingUpdated { peer, .. } => {
                self.connection_data.modify(|s| {
                    // TODO make this event driven
                    s.known_peers.insert(peer);
                });
            }
            KademliaEvent::OutboundQueryCompleted {
                result: QueryResult::GetRecord(record_results),
                id,
                ..
            } => {
                self.handle_get_query(record_results, id);
            }
            KademliaEvent::OutboundQueryCompleted {
                result: QueryResult::PutRecord(r),
                id,
                ..
            } => {
                if let Some(mut query) = self
                    .in_progress_put_record_queries
                    .remove(&Either::Right(id))
                {
                    if query.notify.is_canceled() {
                        return;
                    }

                    match r {
                        Ok(_) => {
                            if query.notify.send(()).is_err() {
                                error!("Put DHT: client channel closed before put record request could be sent");
                            }
                        }
                        Err(e) => {
                            query.progress = DHTProgress::NotStarted;
                            let uid = self.new_put_uid();

                            // push back onto the queue
                            self.in_progress_put_record_queries
                                .insert(Either::Left(uid), query);

                            error!("Put DHT: error performing put: {:?}. Retrying.", e);
                        }
                    }
                } else {
                    error!("Put DHT: completed DHT query that is no longer tracked.");
                }
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
            self.connection_data.modify(|s| {
                s.known_peers.insert(peer_id);
            });
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

/// represents progress through DHT
#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
pub enum DHTProgress {
    InProgress(QueryId),
    NotStarted,
}

/// Metadata holder for get query
#[derive(Debug)]
pub(crate) struct KadGetQuery {
    /// progress through DHT query
    pub(crate) progress: DHTProgress,
    /// notify client of result
    pub(crate) notify: Sender<Vec<u8>>,
    /// number of replicas required to replicate over
    pub(crate) num_replicas: NonZeroUsize,
    /// the key to look up
    pub(crate) key: Vec<u8>,
}

/// Metadata holder for get query
#[derive(Debug)]
pub struct KadPutQuery {
    /// progress through DHT query
    pub(crate) progress: DHTProgress,
    /// notify client of result
    pub(crate) notify: Sender<()>,
    /// the key to put
    pub(crate) key: Vec<u8>,
    /// the value to put
    pub(crate) value: Vec<u8>,
}
