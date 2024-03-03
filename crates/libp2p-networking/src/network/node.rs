/// configuration for the libp2p network (e.g. how it should be built)
mod config;

/// libp2p network handle
/// allows for control over the libp2p network
mod handle;

pub use self::{
    config::{
        MeshParams, NetworkNodeConfig, NetworkNodeConfigBuilder, NetworkNodeConfigBuilderError,
    },
    handle::{
        network_node_handle_error, spawn_network_node, NetworkNodeHandle, NetworkNodeHandleError,
        NetworkNodeReceiver,
    },
};

use super::{
    error::{GossipsubBuildSnafu, GossipsubConfigSnafu, NetworkError, TransportSnafu},
    gen_transport, BoxedTransport, ClientRequest, NetworkDef, NetworkEvent, NetworkEventInternal,
    NetworkNodeType,
};

use crate::network::behaviours::{
    dht::{DHTBehaviour, DHTEvent, DHTProgress, KadPutQuery, NUM_REPLICATED_TO_TRUST},
    direct_message::{DMBehaviour, DMEvent},
    exponential_backoff::ExponentialBackoff,
};
use async_compatibility_layer::{
    art::async_spawn,
    channel::{unbounded, UnboundedReceiver, UnboundedRecvError, UnboundedSender},
};
use futures::{select, FutureExt, StreamExt};
use hotshot_constants::KAD_DEFAULT_REPUB_INTERVAL_SEC;
use libp2p::{core::transport::ListenerId, StreamProtocol};
use libp2p::{
    gossipsub::{
        Behaviour as Gossipsub, ConfigBuilder as GossipsubConfigBuilder, Event as GossipEvent,
        Message as GossipsubMessage, MessageAuthenticity, MessageId, Topic, ValidationMode,
    },
    identify::{
        Behaviour as IdentifyBehaviour, Config as IdentifyConfig, Event as IdentifyEvent,
        Info as IdentifyInfo,
    },
    identity::Keypair,
    kad::{store::MemoryStore, Behaviour, Config},
    request_response::{
        Behaviour as RequestResponse, Config as RequestResponseConfig, ProtocolSupport,
    },
    swarm::SwarmEvent,
    Multiaddr, Swarm, SwarmBuilder,
};
use libp2p_identity::PeerId;
use rand::{prelude::SliceRandom, thread_rng};
use snafu::ResultExt;
use std::{
    collections::{HashMap, HashSet},
    iter,
    num::{NonZeroU32, NonZeroUsize},
    time::Duration,
};
use tracing::{debug, error, info, info_span, instrument, trace, warn, Instrument};

/// Maximum size of a message
pub const MAX_GOSSIP_MSG_SIZE: usize = 200_000_000;

/// Wrapped num of connections
pub const ESTABLISHED_LIMIT: NonZeroU32 =
    unsafe { NonZeroU32::new_unchecked(ESTABLISHED_LIMIT_UNWR) };
/// Number of connections to a single peer before logging an error
pub const ESTABLISHED_LIMIT_UNWR: u32 = 10;

/// Network definition
#[derive(custom_debug::Debug)]
pub struct NetworkNode {
    /// pub/private key from with peer_id is derived
    identity: Keypair,
    /// peer id of network node
    peer_id: PeerId,
    /// the swarm of networkbehaviours
    #[debug(skip)]
    swarm: Swarm<NetworkDef>,
    /// the configuration parameters of the netework
    config: NetworkNodeConfig,
    /// the listener id we are listening on, if it exists
    listener_id: Option<ListenerId>,
}

impl NetworkNode {
    /// Returns number of peers this node is connected to
    pub fn num_connected(&self) -> usize {
        self.swarm.connected_peers().count()
    }

    /// return hashset of PIDs this node is connected to
    pub fn connected_pids(&self) -> HashSet<PeerId> {
        self.swarm.connected_peers().copied().collect()
    }

    /// starts the swarm listening on `listen_addr`
    /// and optionally dials into peer `known_peer`
    /// returns the address the swarm is listening upon
    #[instrument(skip(self))]
    pub async fn start_listen(
        &mut self,
        listen_addr: Multiaddr,
    ) -> Result<Multiaddr, NetworkError> {
        self.listener_id = Some(self.swarm.listen_on(listen_addr).context(TransportSnafu)?);
        let addr = loop {
            if let Some(SwarmEvent::NewListenAddr { address, .. }) = self.swarm.next().await {
                break address;
            }
        };
        info!("peerid {:?} started on addr: {:?}", self.peer_id, addr);
        Ok(addr)
    }

    /// initialize the DHT with known peers
    /// add the peers to kademlia and then
    /// the `spawn_listeners` function
    /// will start connecting to peers
    #[instrument(skip(self))]
    pub fn add_known_peers(&mut self, known_peers: &[(Option<PeerId>, Multiaddr)]) {
        info!("Adding nodes {:?} to {:?}", known_peers, self.peer_id);
        let behaviour = self.swarm.behaviour_mut();
        let mut bs_nodes = HashMap::<PeerId, HashSet<Multiaddr>>::new();
        let mut shuffled = known_peers.iter().collect::<Vec<_>>();
        shuffled.shuffle(&mut thread_rng());
        for (peer_id, addr) in shuffled {
            match peer_id {
                Some(peer_id) => {
                    // if we know the peerid, add address.
                    if *peer_id != self.peer_id {
                        behaviour.dht.add_address(peer_id, addr.clone());
                        bs_nodes.insert(*peer_id, iter::once(addr.clone()).collect());
                    }
                }
                None => {
                    // <https://github.com/EspressoSystems/hotshot/issues/290>
                    // TODO actually implement this part
                    // if we don't know the peerid, dial to find out what the peerid is
                }
            }
        }
        behaviour.dht.add_bootstrap_nodes(bs_nodes);
    }

    /// Creates a new `Network` with the given settings.
    ///
    /// Currently:
    ///   * Generates a random key pair and associated [`PeerId`]
    ///   * Launches a hopefully production ready transport:
    ///       QUIC v1 (RFC 9000) + DNS
    ///   * Generates a connection to the "broadcast" topic
    ///   * Creates a swarm to manage peers and events
    #[instrument]
    pub async fn new(config: NetworkNodeConfig) -> Result<Self, NetworkError> {
        // Generate a random PeerId
        let identity = if let Some(ref kp) = config.identity {
            kp.clone()
        } else {
            Keypair::generate_ed25519()
        };
        let peer_id = PeerId::from(identity.public());
        debug!(?peer_id);
        let transport: BoxedTransport = gen_transport(identity.clone()).await?;
        trace!("Launched network transport");
        // Generate the swarm
        let mut swarm: Swarm<NetworkDef> = {
            // Use the hash of the message's contents as the ID
            // Use blake3 for much paranoia at very high speeds
            let message_id_fn = |message: &GossipsubMessage| {
                let hash = blake3::hash(&message.data);
                MessageId::from(hash.as_bytes().to_vec())
            };

            let params = if let Some(ref params) = config.mesh_params {
                params.clone()
            } else {
                // NOTE this should most likely be a builder pattern
                // at some point in the future.
                match config.node_type {
                    NetworkNodeType::Bootstrap => MeshParams {
                        mesh_n_high: 1000, // make this super high in case we end up scaling to 1k
                        // nodes
                        mesh_n_low: 10,
                        mesh_outbound_min: 5,
                        mesh_n: 15,
                    },
                    NetworkNodeType::Regular => MeshParams {
                        mesh_n_high: 15,
                        mesh_n_low: 8,
                        mesh_outbound_min: 4,
                        mesh_n: 12,
                    },
                    NetworkNodeType::Conductor => MeshParams {
                        mesh_n_high: 21,
                        mesh_n_low: 8,
                        mesh_outbound_min: 4,
                        mesh_n: 12,
                    },
                }
            };

            // Create a custom gossipsub
            let gossipsub_config = GossipsubConfigBuilder::default()
                .opportunistic_graft_ticks(3)
                .heartbeat_interval(Duration::from_secs(1))
                // Force all messages to have valid signatures
                .validation_mode(ValidationMode::Strict)
                .history_gossip(50)
                .mesh_n_high(params.mesh_n_high)
                .mesh_n_low(params.mesh_n_low)
                .mesh_outbound_min(params.mesh_outbound_min)
                .mesh_n(params.mesh_n)
                .history_length(500)
                .max_transmit_size(MAX_GOSSIP_MSG_SIZE)
                // Use the (blake3) hash of a message as its ID
                .message_id_fn(message_id_fn)
                .build()
                .map_err(|s| {
                    GossipsubConfigSnafu {
                        message: s.to_string(),
                    }
                    .build()
                })?;

            // - Build a gossipsub network behavior
            let gossipsub: Gossipsub = Gossipsub::new(
                // TODO do we even need this?
                // <https://github.com/EspressoSystems/hotshot/issues/42>
                // if messages are signed at the the consensus level AND the network
                // level (noise), this feels redundant.
                MessageAuthenticity::Signed(identity.clone()),
                gossipsub_config,
            )
            .map_err(|s| GossipsubBuildSnafu { message: s }.build())?;

            //   Build a identify network behavior needed for own
            //   node connection information
            //   E.g. this will answer the question: how are other nodes
            //   seeing the peer from behind a NAT
            let identify_cfg =
                IdentifyConfig::new("HotShot/identify/1.0".to_string(), identity.public());
            let identify = IdentifyBehaviour::new(identify_cfg);

            // - Build DHT needed for peer discovery
            let mut kconfig = Config::default();
            // 8 hours by default
            let record_republication_interval = config
                .republication_interval
                .unwrap_or(Duration::from_secs(KAD_DEFAULT_REPUB_INTERVAL_SEC));
            let ttl = Some(config.ttl.unwrap_or(16 * record_republication_interval));
            kconfig
                .set_parallelism(NonZeroUsize::new(5).unwrap())
                .set_provider_publication_interval(Some(record_republication_interval))
                .set_publication_interval(Some(record_republication_interval))
                .set_record_ttl(ttl);

            // allowing panic here because something is very wrong if this fales
            #[allow(clippy::panic)]
            if let Some(factor) = config.replication_factor {
                kconfig.set_replication_factor(factor);
            } else {
                panic!("Replication factor not set");
            }

            let kadem = Behaviour::with_config(peer_id, MemoryStore::new(peer_id), kconfig);

            let rrconfig = RequestResponseConfig::default();

            let request_response: libp2p::request_response::cbor::Behaviour<Vec<u8>, Vec<u8>> =
                RequestResponse::new(
                    [(
                        StreamProtocol::new("/HotShot/request_response/1.0"),
                        ProtocolSupport::Full,
                    )]
                    .into_iter(),
                    rrconfig,
                );

            let network = NetworkDef::new(
                gossipsub,
                DHTBehaviour::new(
                    kadem,
                    peer_id,
                    config
                        .replication_factor
                        .unwrap_or_else(|| NonZeroUsize::new(4).unwrap()),
                ),
                identify,
                DMBehaviour::new(request_response),
            );

            // build swarm
            let swarm = SwarmBuilder::with_existing_identity(identity.clone());
            #[cfg(async_executor_impl = "async-std")]
            let swarm = swarm.with_async_std();
            #[cfg(async_executor_impl = "tokio")]
            let swarm = swarm.with_tokio();

            swarm
                .with_other_transport(|_| transport)
                .unwrap()
                .with_behaviour(|_| network)
                .unwrap()
                .build()
        };
        for (peer, addr) in &config.to_connect_addrs {
            if let Some(peer) = peer {
                if peer != swarm.local_peer_id() {
                    swarm.behaviour_mut().add_address(peer, addr.clone());
                }
            }
        }

        Ok(Self {
            identity,
            peer_id,
            swarm,
            config,
            listener_id: None,
        })
    }

    /// event handler for client events
    /// currectly supported actions include
    /// - shutting down the swarm
    /// - gossipping a message to known peers on the `global` topic
    /// - returning the id of the current peer
    /// - subscribing to a topic
    /// - unsubscribing from a toipc
    /// - direct messaging a peer
    #[instrument(skip(self))]
    async fn handle_client_requests(
        &mut self,
        msg: Result<ClientRequest, UnboundedRecvError>,
    ) -> Result<bool, NetworkError> {
        let behaviour = self.swarm.behaviour_mut();
        match msg {
            Ok(msg) => {
                match msg {
                    ClientRequest::BeginBootstrap => {
                        self.swarm.behaviour_mut().dht.begin_bootstrap();
                    }
                    ClientRequest::LookupPeer(pid, chan) => {
                        self.swarm.behaviour_mut().dht.lookup_peer(pid, chan);
                    }
                    ClientRequest::GetRoutingTable(chan) => {
                        self.swarm.behaviour_mut().dht.print_routing_table();
                        if chan.send(()).is_err() {
                            warn!("Tried to notify client but client not tracking anymore");
                        }
                    }
                    ClientRequest::PutDHT { key, value, notify } => {
                        let query = KadPutQuery {
                            progress: DHTProgress::NotStarted,
                            notify,
                            key,
                            value,
                            backoff: ExponentialBackoff::default(),
                        };
                        self.swarm.behaviour_mut().put_record(query);
                    }
                    ClientRequest::GetConnectedPeerNum(s) => {
                        if s.send(self.num_connected()).is_err() {
                            error!("error sending peer number to client");
                        }
                    }
                    ClientRequest::GetConnectedPeers(s) => {
                        if s.send(self.connected_pids()).is_err() {
                            error!("error sending peer set to client");
                        }
                    }
                    ClientRequest::GetDHT {
                        key,
                        notify,
                        retry_count,
                    } => {
                        self.swarm.behaviour_mut().get_record(
                            key,
                            notify,
                            NonZeroUsize::new(NUM_REPLICATED_TO_TRUST).unwrap(),
                            retry_count,
                        );
                    }
                    ClientRequest::IgnorePeers(_peers) => {
                        // NOTE used by test with conductor only
                    }
                    ClientRequest::Shutdown => {
                        if let Some(listener_id) = self.listener_id {
                            self.swarm.remove_listener(listener_id);
                        }

                        return Ok(true);
                    }
                    ClientRequest::GossipMsg(topic, contents) => {
                        behaviour.publish_gossip(Topic::new(topic.clone()), contents.clone());
                    }
                    ClientRequest::Subscribe(t, chan) => {
                        behaviour.subscribe_gossip(&t);
                        if let Some(chan) = chan {
                            if chan.send(()).is_err() {
                                error!("finished subscribing but response channel dropped");
                            }
                        }
                    }
                    ClientRequest::Unsubscribe(t, chan) => {
                        behaviour.unsubscribe_gossip(&t);
                        if let Some(chan) = chan {
                            if chan.send(()).is_err() {
                                error!("finished unsubscribing but response channel dropped");
                            }
                        }
                    }
                    ClientRequest::DirectRequest {
                        pid,
                        contents,
                        retry_count,
                    } => {
                        info!("pid {:?} adding direct request", self.peer_id);
                        behaviour.add_direct_request(pid, contents, retry_count);
                    }
                    ClientRequest::DirectResponse(chan, msg) => {
                        behaviour.add_direct_response(chan, msg);
                    }
                    ClientRequest::AddKnownPeers(peers) => {
                        self.add_known_peers(&peers);
                    }
                    ClientRequest::Prune(pid) => {
                        if self.swarm.disconnect_peer_id(pid).is_err() {
                            error!(
                                "Peer {:?} could not disconnect from pid {:?}",
                                self.peer_id, pid
                            );
                        }
                    }
                }
            }
            Err(e) => {
                error!("Error receiving msg in main behaviour loop: {:?}", e);
            }
        }
        Ok(false)
    }

    /// event handler for events emited from the swarm
    #[allow(clippy::type_complexity)]
    #[instrument(skip(self))]
    async fn handle_swarm_events(
        &mut self,
        event: SwarmEvent<NetworkEventInternal>,
        send_to_client: &UnboundedSender<NetworkEvent>,
    ) -> Result<(), NetworkError> {
        // Make the match cleaner
        info!("event observed {:?}", event);

        #[allow(deprecated)]
        match event {
            SwarmEvent::ConnectionEstablished {
                connection_id: _,
                peer_id,
                endpoint,
                num_established,
                concurrent_dial_errors,
                established_in: _established_in,
            } => {
                if num_established > ESTABLISHED_LIMIT {
                    error!(
                        "Num concurrent connections to a single peer exceeding {:?} at {:?}!",
                        ESTABLISHED_LIMIT, num_established
                    );
                } else {
                    info!("peerid {:?} connection is established to {:?} with endpoint {:?} with concurrent dial errors {:?}. {:?} connections left", self.peer_id, peer_id, endpoint, concurrent_dial_errors, num_established);
                }
            }
            SwarmEvent::ConnectionClosed {
                connection_id: _,
                peer_id,
                endpoint,
                num_established,
                cause,
            } => {
                if num_established > ESTABLISHED_LIMIT_UNWR {
                    error!(
                        "Num concurrent connections to a single peer exceeding {:?} at {:?}!",
                        ESTABLISHED_LIMIT, num_established
                    );
                } else {
                    info!("peerid {:?} connection is closed to {:?} with endpoint {:?}. {:?} connections left. Cause: {:?}", self.peer_id, peer_id, endpoint, num_established, cause);
                }
            }
            SwarmEvent::Dialing {
                peer_id,
                connection_id: _,
            } => {
                info!("{:?} is dialing {:?}", self.peer_id, peer_id);
            }
            SwarmEvent::ListenerClosed {
                listener_id: _,
                addresses: _,
                reason: _,
            }
            | SwarmEvent::NewListenAddr {
                listener_id: _,
                address: _,
            }
            | SwarmEvent::ExpiredListenAddr {
                listener_id: _,
                address: _,
            }
            | SwarmEvent::NewExternalAddrCandidate { .. }
            | SwarmEvent::ExternalAddrConfirmed { .. }
            | SwarmEvent::ExternalAddrExpired { .. }
            | SwarmEvent::IncomingConnection {
                connection_id: _,
                local_addr: _,
                send_back_addr: _,
            } => {}
            SwarmEvent::Behaviour(b) => {
                let maybe_event = match b {
                    NetworkEventInternal::DHTEvent(e) => match e {
                        DHTEvent::IsBootstrapped => Some(NetworkEvent::IsBootstrapped),
                    },
                    NetworkEventInternal::IdentifyEvent(e) => {
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
                                    observed_addr,
                                },
                        } = *e
                        {
                            let behaviour = self.swarm.behaviour_mut();
                            // NOTE in practice, we will want to NOT include this. E.g. only DNS/non localhost IPs
                            // NOTE I manually checked and peer_id corresponds to listen_addrs.
                            // NOTE Once we've tested on DNS addresses, this should be swapped out to play nicely
                            // with autonat
                            info!(
                                "local peer {:?} IDENTIFY ADDRS LISTEN: {:?} for peer {:?}, ADDRS OBSERVED: {:?} ",
                                behaviour.dht.peer_id, peer_id, listen_addrs, observed_addr
                                );
                            // into hashset to delete duplicates (I checked: there are duplicates)
                            for addr in listen_addrs.iter().collect::<HashSet<_>>() {
                                behaviour.dht.add_address(&peer_id, addr.clone());
                            }
                        }
                        None
                    }
                    NetworkEventInternal::GossipEvent(e) => match *e {
                        GossipEvent::Message {
                            propagation_source: _peer_id,
                            message_id: _id,
                            message,
                        } => Some(NetworkEvent::GossipMsg(message.data)),
                        GossipEvent::Subscribed { peer_id, topic } => {
                            info!("Peer: {:?}, Subscribed to topic: {:?}", peer_id, topic);
                            None
                        }
                        GossipEvent::Unsubscribed { peer_id, topic } => {
                            info!("Peer: {:?}, Unsubscribed from topic: {:?}", peer_id, topic);
                            None
                        }
                        GossipEvent::GossipsubNotSupported { peer_id } => {
                            info!("Peer: {:?}, Does not support Gossip", peer_id);
                            None
                        }
                    },
                    NetworkEventInternal::DMEvent(e) => Some(match e {
                        DMEvent::DirectRequest(data, pid, chan) => {
                            NetworkEvent::DirectRequest(data, pid, chan)
                        }
                        DMEvent::DirectResponse(data, pid) => {
                            NetworkEvent::DirectResponse(data, pid)
                        }
                    }),
                };

                if let Some(event) = maybe_event {
                    // forward messages directly to Client
                    send_to_client
                        .send(event)
                        .await
                        .map_err(|_e| NetworkError::StreamClosed)?;
                }
            }
            SwarmEvent::OutgoingConnectionError {
                connection_id: _,
                peer_id: _,
                error,
            } => {
                info!(?error, "OUTGOING CONNECTION ERROR, {:?}", error);
            }
            SwarmEvent::IncomingConnectionError {
                connection_id: _,
                local_addr,
                send_back_addr,
                error,
            } => {
                info!(
                    "INCOMING CONNECTION ERROR: {:?} {:?} {:?}",
                    local_addr, send_back_addr, error
                );
            }
            SwarmEvent::ListenerError { listener_id, error } => {
                info!("LISTENER ERROR {:?} {:?}", listener_id, error);
            }
            _ => {
                error!(
                    "Unhandled swarm event {:?}. This should not be possible.",
                    event
                );
            }
        }
        Ok(())
    }

    /// Spawn a task to listen for requests on the returned channel
    /// as well as any events produced by libp2p
    #[instrument]
    pub async fn spawn_listeners(
        mut self,
    ) -> Result<
        (
            UnboundedSender<ClientRequest>,
            UnboundedReceiver<NetworkEvent>,
        ),
        NetworkError,
    > {
        let (s_input, s_output) = unbounded::<ClientRequest>();
        let (r_input, r_output) = unbounded::<NetworkEvent>();

        async_spawn(
            async move {
                let mut fuse = s_output.recv().boxed().fuse();
                loop {
                    select! {
                        event = self.swarm.next() => {
                            debug!("peerid {:?}\t\thandling maybe event {:?}", self.peer_id, event);
                            if let Some(event) = event {
                                info!("peerid {:?}\t\thandling event {:?}", self.peer_id, event);
                                self.handle_swarm_events(event, &r_input).await?;
                            }
                        },
                        msg = fuse => {
                            debug!("peerid {:?}\t\thandling msg {:?}", self.peer_id, msg);
                            let shutdown = self.handle_client_requests(msg).await?;
                            if shutdown {
                                break
                            }
                            fuse = s_output.recv().boxed().fuse();
                        }
                    }
                }
                Ok::<(), NetworkError>(())
            }
            .instrument(info_span!("Libp2p NetworkBehaviour Handler")),
        );
        Ok((s_input, r_output))
    }

    /// Get a reference to the network node's peer id.
    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }
}
