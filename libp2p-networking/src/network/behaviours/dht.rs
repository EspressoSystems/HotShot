use std::{
    collections::{HashMap, HashSet, VecDeque},
    num::NonZeroUsize,
    task::Poll,
    time::Duration,
};

use futures::channel::oneshot::Sender;
use libp2p::{
    core::transport::ListenerId,
    kad::{
        store::MemoryStore, BootstrapError, BootstrapOk, GetClosestPeersOk, GetRecordOk,
        GetRecordResult, Kademlia, KademliaEvent, PutRecordResult, QueryId, QueryResult, Quorum,
        Record, ProgressStep,
    },
    swarm::{NetworkBehaviour, NetworkBehaviourAction},
    Multiaddr, PeerId,
};
use tracing::{error, info, warn};

use super::exponential_backoff::ExponentialBackoff;

/// Behaviour wrapping libp2p's kademlia
/// included:
/// - publishing API
/// - Request API
/// - bootstrapping into the network
/// - peer discovery
pub struct DHTBehaviour {
    /// client approval to begin bootstrap
    pub begin_bootstrap: bool,
    /// in progress queries for nearby peers
    pub in_progress_get_closest_peers: HashMap<QueryId, Sender<()>>,
    /// bootstrap nodes
    pub bootstrap_nodes: HashMap<PeerId, HashSet<Multiaddr>>,
    /// List of kademlia events
    pub event_queue: Vec<DHTEvent>,
    /// List of in-progress get requests
    in_progress_get_record_queries: HashMap<QueryId, KadGetQuery>,
    /// List of in-progress put requests
    in_progress_put_record_queries: HashMap<QueryId, KadPutQuery>,
    /// List of previously failled get requests
    queued_get_record_queries: VecDeque<KadGetQuery>,
    /// List of previously failled put requests
    queued_put_record_queries: VecDeque<KadPutQuery>,
    /// Kademlia behaviour
    pub kadem: Kademlia<MemoryStore>,
    /// State of bootstrapping
    pub bootstrap_state: Bootstrap,
    /// State of last random walk
    pub random_walk: RandomWalk,
    /// the peer id (useful only for debugging right now)
    pub peer_id: PeerId,
    /// replication factor
    pub replication_factor: NonZeroUsize,
}

/// State of bootstrapping
#[derive(Debug, Clone)]
pub struct Bootstrap {
    /// State of bootstrap
    pub state: State,
    /// Retry timeout
    pub backoff: ExponentialBackoff,
}

/// State of the periodic random walk
pub struct RandomWalk {
    /// State of random walk
    state: State,
    /// Retry timeout
    backoff: ExponentialBackoff,
}

/// State used for random walk and bootstrapping
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Not in progress
    NotStarted,
    /// In progress
    Started,
    /// Sucessfully completed
    Finished,
}

/// DHT event enum
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DHTEvent {
    /// Only event tracked currently is when we successfully bootstrap into the network
    IsBootstrapped,
}

impl DHTBehaviour {
    /// Begin the bootstrap process
    pub fn begin_bootstrap(&mut self) {
        self.begin_bootstrap = true;
    }

    /// Start a query for the closest peers
    pub fn query_closest_peers(&mut self, random_peer: PeerId) {
        self.kadem.get_closest_peers(random_peer);
    }

    /// Create a new DHT behaviour
    pub fn new(
        kadem: Kademlia<MemoryStore>,
        pid: PeerId,
        replication_factor: NonZeroUsize,
    ) -> Self {
        Self {
            begin_bootstrap: false,
            bootstrap_nodes: HashMap::default(),
            peer_id: pid,
            event_queue: Vec::default(),
            in_progress_get_record_queries: HashMap::default(),
            in_progress_put_record_queries: HashMap::default(),
            queued_get_record_queries: VecDeque::default(),
            queued_put_record_queries: VecDeque::default(),
            kadem,
            bootstrap_state: Bootstrap {
                state: State::NotStarted,
                backoff: ExponentialBackoff::new(2, Duration::from_secs(1)),
            },
            random_walk: RandomWalk {
                state: State::NotStarted,
                // TODO jr this may be way too frequent
                backoff: ExponentialBackoff::new(2, Duration::from_secs(1)),
            },
            in_progress_get_closest_peers: HashMap::default(),
            replication_factor,
        }
    }

    /// query a peer (e.g. obtain its address if it exists)
    pub fn lookup_peer(&mut self, peer_id: PeerId, chan: Sender<()>) {
        let qid = self.kadem.get_closest_peers(peer_id);
        self.in_progress_get_closest_peers.insert(qid, chan);
    }

    /// print out the routing table to stderr
    pub fn print_routing_table(&mut self) {
        let mut err = format!("KBUCKETS: PID: {:?}, ", self.peer_id);
        let v = self.kadem.kbuckets().collect::<Vec<_>>();
        for i in v {
            for j in i.iter() {
                let s = format!(
                    "node: key: {:?}, val {:?}, status: {:?}",
                    j.node.key, j.node.value, j.status
                );
                err.push_str(&s);
            }
        }
        error!("{:?}", err);
    }

    /// Passthru to kademlia
    /// Associate address with kademlia peer
    pub fn add_address(&mut self, peer_id: &PeerId, addr: Multiaddr) {
        // add address to kademlia
        self.kadem.add_address(peer_id, addr);
    }

    /// Save in case kademlia forgets about bootstrap nodes
    pub fn add_bootstrap_nodes(&mut self, nodes: HashMap<PeerId, HashSet<Multiaddr>>) {
        for (k, v) in nodes {
            self.bootstrap_nodes.insert(k, v);
        }
    }

    /// Publish a key/value to the kv store.
    /// Once replicated upon all nodes, the caller is notified over
    /// `chan`. If there is an error, a [`crate::network::error::DHTError`] is
    /// sent instead.
    pub fn put_record(&mut self, mut query: KadPutQuery) {
        let record = Record::new(query.key.clone(), query.value.clone());

        match self
            .kadem
            .put_record(record, Quorum::N(self.replication_factor))
        {
            Err(e) => {
                // failed try again later
                query.progress = DHTProgress::NotStarted;
                query.backoff.start_next(false);
                warn!("Error publishing to DHT: {e:?} for peer {:?}", self.peer_id);
                self.queued_put_record_queries.push_back(query);
            }
            Ok(qid) => {
                info!("Success publishing {:?} to DHT", qid);
                let query = KadPutQuery {
                    progress: DHTProgress::InProgress(qid),
                    ..query
                };
                self.in_progress_put_record_queries.insert(qid, query);
            }
        }
    }

    /// Retrieve a value for a key from the DHT.
    /// Value (serialized) is sent over `chan`, and if a value is not found,
    /// a [`crate::network::error::DHTError`] is sent instead.
    /// NOTE: noop if `retry_count` is 0
    pub fn get_record(
        &mut self,
        key: Vec<u8>,
        chan: Sender<Vec<u8>>,
        factor: NonZeroUsize,
        backoff: ExponentialBackoff,
        retry_count: u8,
    ) {
        // noop
        if retry_count == 0 {
            return;
        }

        let qid = self.kadem.get_record(key.clone().into());
        let query = KadGetQuery {
            backoff,
            progress: DHTProgress::InProgress(qid),
            notify: chan,
            num_replicas: factor,
            key,
            retry_count: retry_count - 1,
        };
        self.in_progress_get_record_queries.insert(qid, query);
    }

    /// update state based on recv-ed get query
    fn handle_get_query(&mut self, record_results: GetRecordResult, id: QueryId) {
        if let Some(KadGetQuery {
            backoff,
            progress,
            notify,
            num_replicas,
            key,
            retry_count,
        }) = self.in_progress_get_record_queries.remove(&id)
        {
            // if channel has been dropped, cancel request
            if notify.is_canceled() {
                return;
            }
            match record_results {
                Ok(records) => {
                    match records {
                        GetRecordOk::FoundRecord(record) => {
                            if notify.send(record.record.value).is_err() {
                                warn!("Get DHT: channel closed before get record request result could be sent");
                            }
                        },
                        GetRecordOk::FinishedWithNoAdditionalRecord { .. } => {},
                    }
                },
                Err(_e) => {
                    let mut new_query = KadGetQuery {
                        backoff,
                        progress: DHTProgress::NotStarted,
                        notify,
                        num_replicas,
                        key,
                        retry_count,
                    };
                    new_query.backoff.start_next(false);
                    self.queued_get_record_queries.push_back(new_query);
                }
            };
        } else {
            warn!("completed DHT query {:?} that is no longer tracked.", id);
        }
    }

    /// Update state based on put query
    fn handle_put_query(&mut self, record_results: PutRecordResult, id: QueryId) {
        if let Some(mut query) = self.in_progress_put_record_queries.remove(&id) {
            // dropped so we handle further
            if query.notify.is_canceled() {
                return;
            }

            match record_results {
                Ok(_) => {
                    if query.notify.send(()).is_err() {
                        warn!("Put DHT: client channel closed before put record request could be sent");
                    }
                }
                Err(e) => {
                    query.progress = DHTProgress::NotStarted;
                    query.backoff.start_next(false);

                    warn!(
                        "Put DHT: error performing put: {:?}. Retrying on pid {:?}.",
                        e, self.peer_id
                    );
                    // push back onto the queue
                    self.queued_put_record_queries.push_back(query);
                }
            }
        } else {
            warn!("Put DHT: completed DHT query that is no longer tracked.");
        }
    }
}

impl DHTBehaviour {
    #![allow(clippy::too_many_lines)]
    fn dht_handle_event(&mut self, event: KademliaEvent) {
        match event {
            KademliaEvent::OutboundQueryProgressed {
                result: QueryResult::PutRecord(record_results),
                id,
                step: ProgressStep { last: true, .. },
                ..
            } => {
                self.handle_put_query(record_results, id);
            }
            KademliaEvent::OutboundQueryProgressed {
                result: QueryResult::GetClosestPeers(r),
                id: query_id,
                stats,
                step: ProgressStep { last: true, .. },
                ..
            } => match r {
                Ok(GetClosestPeersOk { key, peers }) => {
                    if let Some(chan) = self.in_progress_get_closest_peers.remove(&query_id) {
                        if chan.send(()).is_err() {
                            warn!("DHT: finished query but client no longer interested");
                        };
                    } else {
                        self.random_walk.state = State::NotStarted;
                        self.random_walk.backoff.start_next(true);
                    }
                    info!(
                        "peer {:?} successfully completed get closest peers for {:?} with peers {:?}",
                        self.peer_id, key, peers
                    );
                }
                Err(e) => {
                    if let Some(chan) = self.in_progress_get_closest_peers.remove(&query_id) {
                        let _ = chan.send(());
                    } else {
                        self.random_walk.state = State::NotStarted;
                        self.random_walk.backoff.start_next(true);
                    }
                    warn!(
                        "peer {:?} failed to get closest peers with {:?} and stats {:?}",
                        self.peer_id, e, stats
                    );
                }
            },
            KademliaEvent::OutboundQueryProgressed {
                result: QueryResult::GetRecord(record_results),
                id,
                step: ProgressStep { last: true, .. },
                ..
            } => {
                self.handle_get_query(record_results, id);
            }
            KademliaEvent::OutboundQueryProgressed {
                result:
                    QueryResult::Bootstrap(Ok(BootstrapOk {
                        peer: _,
                        num_remaining,
                    })),
                step: ProgressStep { last: true, .. },
                ..
            } => {
                if num_remaining == 0 {
                    // if bootstrap is successful, restart.
                    error!("Finished bootstrap for peer {:?}", self.peer_id);
                    self.bootstrap_state.state = State::Finished;
                    self.event_queue.push(DHTEvent::IsBootstrapped);
                    self.begin_bootstrap = false;
                } else {
                    error!(
                        "Bootstrap in progress: num remaining nodes to ping {:?}",
                        num_remaining
                    );
                }
            }
            KademliaEvent::OutboundQueryProgressed {
                result: QueryResult::Bootstrap(Err(e)),
                ..
            } => {
                error!("DHT: Bootstrap attempt failed. Retrying shortly.");
                let BootstrapError::Timeout { num_remaining, .. } = e;
                if num_remaining.is_none() {
                    error!(
                        "Peer {:?} failed bootstrap with error {:?}. This should not happen and means all bootstrap nodes are down or were evicted from our local DHT. Readding bootstrap nodes {:?}",
                        self.peer_id, e, self.bootstrap_nodes
                    );
                    for (peer, addrs) in self.bootstrap_nodes.clone() {
                        for addr in addrs {
                            self.kadem.add_address(&peer, addr);
                        }
                    }
                }
                self.bootstrap_state.state = State::NotStarted;
                self.bootstrap_state.backoff.start_next(true);
            }
            KademliaEvent::RoutablePeer { peer, address: _ } => {
                info!("on peer {:?} found routable peer {:?}", self.peer_id, peer);
            }
            KademliaEvent::PendingRoutablePeer { peer, address: _ } => {
                info!(
                    "on peer {:?} have pending routable peer {:?}",
                    self.peer_id, peer
                );
            }
            KademliaEvent::UnroutablePeer { peer } => {
                info!("on peer {:?} have unroutable peer {:?}", self.peer_id, peer);
            }
            KademliaEvent::InboundRequest { request: _r } => {}
            KademliaEvent::RoutingUpdated {
                peer: _,
                is_new_peer: _,
                addresses: _,
                bucket_range: _,
                old_peer: _,
            } => {}
            e @ KademliaEvent::OutboundQueryProgressed { .. } => {
                info!("Not handling dht event {:?}", e);
            }
        }
    }
}

/// Metadata holder for get query
#[derive(Debug)]
pub(crate) struct KadGetQuery {
    /// Exponential retry backoff
    pub(crate) backoff: ExponentialBackoff,
    /// progress through DHT query
    pub(crate) progress: DHTProgress,
    /// notify client of result
    pub(crate) notify: Sender<Vec<u8>>,
    /// number of replicas required to replicate over
    pub(crate) num_replicas: NonZeroUsize,
    /// the key to look up
    pub(crate) key: Vec<u8>,
    /// the number of remaining retries before giving up
    pub(crate) retry_count: u8,
}

/// Metadata holder for get query
#[derive(Debug)]
pub struct KadPutQuery {
    /// Exponential retry backoff
    pub(crate) backoff: ExponentialBackoff,
    /// progress through DHT query
    pub(crate) progress: DHTProgress,
    /// notify client of result
    pub(crate) notify: Sender<()>,
    /// the key to put
    pub(crate) key: Vec<u8>,
    /// the value to put
    pub(crate) value: Vec<u8>,
}

/// represents progress through DHT
#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub enum DHTProgress {
    /// The query has been started
    InProgress(QueryId),
    /// The query has not been started
    NotStarted,
}


// Diagnostics:
// 1. use of deprecated associated function `libp2p::libp2p_swarm::NetworkBehaviour::inject_event`: Implement `NetworkBehaviour::on_connection_handler_event` instead. The default implementation of this `inject_*` method delegates to it.


impl NetworkBehaviour for DHTBehaviour {
    type ConnectionHandler = <Kademlia<MemoryStore> as NetworkBehaviour>::ConnectionHandler;

    type OutEvent = DHTEvent;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        self.kadem.new_handler()
    }

    fn inject_event(
        &mut self,
        peer_id: libp2p::PeerId,
        connection: libp2p::core::connection::ConnectionId,
        event: <<Self::ConnectionHandler as libp2p::swarm::IntoConnectionHandler>::Handler as libp2p::swarm::ConnectionHandler>::OutEvent,
    ) {
        // pass event GENERATED by handler from swarm to kademlia
        self.kadem.inject_event(peer_id, connection, event);
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
        params: &mut impl libp2p::swarm::PollParameters,
    ) -> std::task::Poll<
        libp2p::swarm::NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>,
    > {
        if matches!(self.bootstrap_state.state, State::NotStarted)
            && self.bootstrap_state.backoff.is_expired()
            && self.begin_bootstrap
        {
            match self.kadem.bootstrap() {
                Ok(_) => {
                    info!("started bootstrap for peer {:?}", self.peer_id);
                    self.bootstrap_state.state = State::Started;
                }
                Err(e) => {
                    error!(
                        "peer id {:?} FAILED TO START BOOTSTRAP {:?} adding peers {:?}",
                        self.peer_id, e, self.bootstrap_nodes
                    );
                    for (peer, addrs) in self.bootstrap_nodes.clone() {
                        for addr in addrs {
                            self.kadem.add_address(&peer, addr);
                        }
                    }
                }
            }
        }

        if matches!(self.random_walk.state, State::NotStarted)
            && self.random_walk.backoff.is_expired()
            && matches!(self.bootstrap_state.state, State::Finished)
        {
            self.kadem.get_closest_peers(PeerId::random());
            self.random_walk.state = State::Started;
        }

        // retry put/gets if they are ready
        while let Some(req) = self.queued_get_record_queries.pop_front() {
            if req.backoff.is_expired() {
                self.get_record(
                    req.key,
                    req.notify,
                    req.num_replicas,
                    req.backoff,
                    req.retry_count,
                );
            } else {
                self.queued_get_record_queries.push_back(req);
            }
        }
        while let Some(req) = self.queued_put_record_queries.pop_front() {
            if req.backoff.is_expired() {
                self.put_record(req);
            } else {
                self.queued_put_record_queries.push_back(req);
            }
        }

        // poll behaviour which is a passthrough and call inject event
        while let Poll::Ready(ready) = NetworkBehaviour::poll(&mut self.kadem, cx, params) {
            match ready {
                NetworkBehaviourAction::GenerateEvent(e) => {
                    self.dht_handle_event(e);
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
        if !self.event_queue.is_empty() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                self.event_queue.remove(0),
            ));
        }
        Poll::Pending
    }

    fn addresses_of_peer(&mut self, pid: &PeerId) -> Vec<libp2p::Multiaddr> {
        self.kadem.addresses_of_peer(pid)
    }

    fn inject_connection_established(
        &mut self,
        peer_id: &PeerId,
        connection_id: &libp2p::core::connection::ConnectionId,
        endpoint: &libp2p::core::ConnectedPoint,
        failed_addresses: Option<&Vec<libp2p::Multiaddr>>,
        other_established: usize,
    ) {
        self.kadem.inject_connection_established(
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
        self.kadem
            .inject_connection_closed(pid, cid, cp, handler, remaining_established);
    }

    fn inject_address_change(
        &mut self,
        pid: &PeerId,
        cid: &libp2p::core::connection::ConnectionId,
        old: &libp2p::core::ConnectedPoint,
        new: &libp2p::core::ConnectedPoint,
    ) {
        self.kadem.inject_address_change(pid, cid, old, new);
    }

    fn inject_dial_failure(
        &mut self,
        peer_id: Option<PeerId>,
        handler: Self::ConnectionHandler,
        error: &libp2p::swarm::DialError,
    ) {
        info!(
            "{:?} dial failure for peer id: {:?} with error {:?}",
            self.peer_id, peer_id, error
        );
        // NOTE if there are no addresses
        // initiate query searching
        self.kadem.inject_dial_failure(peer_id, handler, error);
    }

    fn inject_listen_failure(
        &mut self,
        local_addr: &libp2p::Multiaddr,
        send_back_addr: &libp2p::Multiaddr,
        handler: Self::ConnectionHandler,
    ) {
        self.kadem
            .inject_listen_failure(local_addr, send_back_addr, handler);
    }

    fn inject_new_listener(&mut self, id: ListenerId) {
        self.kadem.inject_new_listener(id);
    }

    fn inject_new_listen_addr(&mut self, id: ListenerId, addr: &libp2p::Multiaddr) {
        self.kadem.inject_new_listen_addr(id, addr);
    }

    fn inject_expired_listen_addr(&mut self, id: ListenerId, addr: &libp2p::Multiaddr) {
        self.kadem.inject_expired_listen_addr(id, addr);
    }

    fn inject_listener_error(&mut self, id: ListenerId, err: &(dyn std::error::Error + 'static)) {
        self.kadem.inject_listener_error(id, err);
    }

    fn inject_listener_closed(&mut self, id: ListenerId, reason: Result<(), &std::io::Error>) {
        self.kadem.inject_listener_closed(id, reason);
    }

    fn inject_new_external_addr(&mut self, addr: &libp2p::Multiaddr) {
        self.kadem.inject_new_external_addr(addr);
    }

    fn inject_expired_external_addr(&mut self, addr: &libp2p::Multiaddr) {
        self.kadem.inject_expired_external_addr(addr);
    }

    fn on_swarm_event(&mut self, _event: libp2p::swarm::derive_prelude::FromSwarm<Self::ConnectionHandler>) {}

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: libp2p::swarm::derive_prelude::ConnectionId,
        _event: <<Self::ConnectionHandler as libp2p::swarm::IntoConnectionHandler>::Handler as
        libp2p::swarm::ConnectionHandler>::OutEvent,
    ) {
        // TODO fill this out
    }
}
