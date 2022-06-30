use std::{num::NonZeroUsize, collections::{HashMap, VecDeque}, task::Poll};


use futures::channel::oneshot::Sender;
use libp2p::{kad::{QueryId, KademliaEvent, Kademlia, store::MemoryStore, QueryResult, GetClosestPeersOk, GetRecordResult, PutRecordResult, GetRecordOk, Quorum, Record}, swarm::{NetworkBehaviourEventProcess, NetworkBehaviour, NetworkBehaviourAction}, PeerId, Multiaddr};
use tracing::{error, info};
pub(crate) const NUM_REPLICATED_TO_TRUST: usize = 2;
const MAX_DHT_QUERY_SIZE: usize = 5;

use super::exponential_backoff::ExponentialBackoff;

pub struct DHTBehaviour {
    /// NOTE this is currently unused but if we ever generated an event queue...
    event_queue: Vec<DHTEvent>,
    in_progress_get_record_queries: HashMap<QueryId, KadGetQuery>,
    in_progress_put_record_queries: HashMap<QueryId, KadPutQuery>,
    queued_get_record_queries: VecDeque<KadGetQuery>,
    queued_put_record_queries: VecDeque<KadPutQuery>,
    kadem: Kademlia<MemoryStore>,
    bootstrap_state: Bootstrap,
}

pub struct Bootstrap {
    state: BootstrapState,
    backoff: ExponentialBackoff
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum BootstrapState {
    NotStarted,
    Started,
    Finished,
}

pub enum DHTEvent {
}

impl DHTBehaviour {
    /// Start a query for the closest peers
    pub fn query_closest_peers(&mut self, random_peer: PeerId) {
        self.kadem.get_closest_peers(random_peer);
    }

    pub fn new(kadem: Kademlia<MemoryStore>) -> Self {
        Self {
            event_queue: Vec::default(),
            in_progress_get_record_queries: HashMap::default(),
            in_progress_put_record_queries: HashMap::default(),
            queued_get_record_queries: VecDeque::default(),
            queued_put_record_queries: VecDeque::default(),
            kadem,
            bootstrap_state: Bootstrap {
                state: BootstrapState::NotStarted,
                backoff: ExponentialBackoff::default(),
            },
        }
    }

    /// Passthru to kademlia
    pub fn add_address(&mut self, peer_id: &PeerId, addr: Multiaddr) {
        // add address to kademlia
        self.kadem.add_address(&peer_id, addr.clone());
    }

    /// Publish a key/value to the kv store.
    /// Once replicated upon all nodes, the caller is notified over
    /// `chan`. If there is an error, a [`DHTError`] is
    /// sent instead.
    pub fn put_record(&mut self, mut query: KadPutQuery) {
        let record = Record::new(query.key.clone(), query.value.clone());

        match self.kadem.put_record(record, Quorum::Majority) {
            Err(e) => {
                // failed try again later
                query.progress = DHTProgress::NotStarted;
                query.backoff.start_next(false);
                error!("Error publishing to DHT: {e:?}");
                self.queued_put_record_queries
                    .push_back(query);
            }
            Ok(qid) => {
                let query = KadPutQuery {
                    progress: DHTProgress::InProgress(qid),
                    ..query
                };
                self.in_progress_put_record_queries
                    .insert(qid, query);
            }
        }
    }


    /// Retrieve a value for a key from the DHT.
    /// Value (serialized) is sent over `chan`, and if a value is not found,
    /// a [`DHTError`] is sent instead.
    pub fn get_record(&mut self, key: Vec<u8>, chan: Sender<Vec<u8>>, factor: NonZeroUsize, backoff: ExponentialBackoff) {
        let qid = self.kadem.get_record(key.clone().into(), Quorum::N(factor));
        let query = KadGetQuery {
            backoff,
            progress: DHTProgress::InProgress(qid),
            notify: chan,
            num_replicas: factor,
            key,
        };
        self.in_progress_get_record_queries.insert(qid, query);
    }

    pub fn handle_get_query(&mut self, record_results: GetRecordResult, id: QueryId) {
        if let Some(KadGetQuery {
            backoff,
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
                        self.get_record(key, notify, num_replicas, backoff);
                    }
                    // many records that don't match => disagreement
                    else if records.len() > MAX_DHT_QUERY_SIZE {
                        error!(
                            "Get DHT: Record disagreed upon; {:?}! requerying with more nodes",
                            progress
                        );
                        self.get_record(key, notify, num_replicas, backoff);
                    }
                    // disagreement => query more nodes
                    else {
                        // there is some internal disagreement.
                        // Initiate new query that hits more replicas
                        let new_factor =
                            NonZeroUsize::new(num_replicas.get() + 1).unwrap_or(num_replicas);

                        self.get_record(key, notify, new_factor, backoff);
                        error!("Get DHT: Internal disagreement for get dht request {:?}! requerying with more nodes", progress);
                    }
                }
                Err(_e) => {
                    let mut new_query = KadGetQuery {
                        backoff,
                        progress: DHTProgress::NotStarted, notify, num_replicas, key
                    };
                    new_query.backoff.start_next(false);
                    self.queued_get_record_queries.push_back(new_query);
                }
            };
        } else {
            error!("completed DHT query {:?} that is no longer tracked.", id);
        }

    }
    pub fn handle_put_query(&mut self, record_results: PutRecordResult, id: QueryId) {
        if let Some(mut query) = self
            .in_progress_put_record_queries
                .remove(&id)
                {
                    // dropped so we handle further
                    if query.notify.is_canceled() {
                        return;
                    }

                    match record_results {
                        Ok(_) => {
                            if query.notify.send(()).is_err() {
                                error!("Put DHT: client channel closed before put record request could be sent");
                            }
                        }
                        Err(e) => {
                            query.progress = DHTProgress::NotStarted;
                            query.backoff.start_next(false);

                            error!("Put DHT: error performing put: {:?}. Retrying.", e);
                            // push back onto the queue
                            self.queued_put_record_queries.push_back(query);
                        }
                    }
                } else {
                    error!("Put DHT: completed DHT query that is no longer tracked.");
                }

    }
}

impl NetworkBehaviourEventProcess<KademliaEvent> for DHTBehaviour {
    fn inject_event(&mut self, event: KademliaEvent) {
        match event {
            KademliaEvent::OutboundQueryCompleted {
                result: QueryResult::PutRecord(record_results),
                id,
                ..
            } => {
                self.handle_put_query(record_results, id);
            }
            KademliaEvent::OutboundQueryCompleted {
                result: QueryResult::GetRecord(record_results),
                id,
                ..
            } => {
                self.handle_get_query(record_results, id);
            }
            KademliaEvent::InboundRequest { .. } => {
            },
            KademliaEvent::OutboundQueryCompleted {
                result: QueryResult::GetClosestPeers(r),
                ..
            } => {
                if let Ok(GetClosestPeersOk { peers: _, .. }) = r {
                }
            }
            KademliaEvent::OutboundQueryCompleted {
                result: QueryResult::Bootstrap(Ok(_)),
                ..
            } => {
                self.bootstrap_state.state = BootstrapState::Finished;
                self.bootstrap_state.backoff.reset();
            }
            KademliaEvent::OutboundQueryCompleted {
                result: QueryResult::Bootstrap(Err(_)),
                ..
            } => {
                // bootstrap failed. try again
                self.bootstrap_state.state = BootstrapState::NotStarted;
                self.bootstrap_state.backoff.start_next(false);
            }
            // KademliaEvent::OutboundQueryCompleted { .. } => {
            // },
            KademliaEvent::RoutingUpdated { .. } => {},
            KademliaEvent::UnroutablePeer { .. } => {},
            KademliaEvent::RoutablePeer { .. } => {},
            KademliaEvent::PendingRoutablePeer { ..} => {},
            e => {
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
    InProgress(QueryId),
    NotStarted,
}

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
    ) -> std::task::Poll<libp2p::swarm::NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
        if matches!(self.bootstrap_state.state, BootstrapState::NotStarted) && self.bootstrap_state.backoff.is_expired() {
            match self.kadem.bootstrap() {
                Ok(_) => {
                    self.bootstrap_state.backoff.reset();
                    self.bootstrap_state.state = BootstrapState::Started;
                },
                Err(_) => self.bootstrap_state.backoff.start_next(false),
            }
        }
        // retry put/gets if they are ready
        while let Some(req) = self.queued_get_record_queries.pop_front() {
            if req.backoff.is_expired() {
                self.get_record(req.key, req.notify, req.num_replicas, req.backoff);
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
        loop {
            match NetworkBehaviour::poll(&mut self.kadem, cx, params) {
                Poll::Ready(ready) => {
                    match ready {
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
        if !self.event_queue.is_empty(){
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(self.event_queue.remove(0)))
        }
        let f: Poll<
            NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>,
            > = Self::poll(self, cx, params);
        f
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
        self.kadem.inject_connection_established(peer_id, connection_id, endpoint, failed_addresses, other_established);
    }

    fn inject_connection_closed(
        &mut self,
        pid: &PeerId,
        cid: &libp2p::core::connection::ConnectionId,
        cp: &libp2p::core::ConnectedPoint,
        handler: <Self::ConnectionHandler as libp2p::swarm::IntoConnectionHandler>::Handler,
        remaining_established: usize,
    ) {
        self.kadem.inject_connection_closed(pid, cid, cp, handler, remaining_established);
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
        self.kadem.inject_dial_failure(peer_id, handler, error);
    }

    fn inject_listen_failure(
        &mut self,
        local_addr: &libp2p::Multiaddr,
        send_back_addr: &libp2p::Multiaddr,
        handler: Self::ConnectionHandler,
    ) {
        self.kadem.inject_listen_failure(local_addr, send_back_addr, handler);
    }

    fn inject_new_listener(&mut self, id: libp2p::core::connection::ListenerId) {
        self.kadem.inject_new_listener(id);
    }

    fn inject_new_listen_addr(&mut self, id: libp2p::core::connection::ListenerId, addr: &libp2p::Multiaddr) {
        self.kadem.inject_new_listen_addr(id, addr);
    }

    fn inject_expired_listen_addr(&mut self, id: libp2p::core::connection::ListenerId, addr: &libp2p::Multiaddr) {
        self.kadem.inject_expired_listen_addr(id, addr);
    }

    fn inject_listener_error(&mut self, id: libp2p::core::connection::ListenerId, err: &(dyn std::error::Error + 'static)) {
        self.kadem.inject_listener_error(id, err);
    }

    fn inject_listener_closed(&mut self, id: libp2p::core::connection::ListenerId, reason: Result<(), &std::io::Error>) {
        self.kadem.inject_listener_closed(id, reason);
    }

    fn inject_new_external_addr(&mut self, addr: &libp2p::Multiaddr) {
        self.kadem.inject_new_external_addr(addr);
    }

    fn inject_expired_external_addr(&mut self, addr: &libp2p::Multiaddr) {
        self.kadem.inject_expired_external_addr(addr);
    }

}
