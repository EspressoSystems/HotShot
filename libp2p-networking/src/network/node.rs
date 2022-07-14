mod config;
mod handle;

use crate::network::{
    behaviours::{
        dht::{DHTBehaviour, DHTProgress, KadPutQuery},
        direct_message::DMBehaviour,
        direct_message_codec::{DirectMessageCodec, DirectMessageProtocol, MAX_MSG_SIZE_DM},
        exponential_backoff::ExponentialBackoff,
    },
    def::NUM_REPLICATED_TO_TRUST,
};

pub use self::{
    config::{
        MeshParams, NetworkNodeConfig, NetworkNodeConfigBuilder, NetworkNodeConfigBuilderError,
    },
    handle::{network_node_handle_error, NetworkNodeHandle, NetworkNodeHandleError},
};
use super::{
    behaviours::gossip::GossipBehaviour,
    error::{GossipsubBuildSnafu, GossipsubConfigSnafu, NetworkError, TransportSnafu},
    gen_transport, ClientRequest, NetworkDef, NetworkEvent, NetworkNodeType,
};
use async_std::task::{sleep, spawn};
use flume::{unbounded, Receiver, Sender};
use futures::{select, FutureExt, StreamExt};
use libp2p::{
    core::{either::EitherError, muxing::StreamMuxerBox, transport::Boxed},
    gossipsub::{
        error::GossipsubHandlerError, Gossipsub, GossipsubConfigBuilder, GossipsubMessage,
        MessageAuthenticity, MessageId, Topic, ValidationMode,
    },
    identify::{Identify, IdentifyConfig},
    identity::Keypair,
    kad::{store::MemoryStore, Kademlia, KademliaConfig},
    request_response::{ProtocolSupport, RequestResponse, RequestResponseConfig},
    swarm::{ConnectionHandlerUpgrErr, SwarmBuilder, SwarmEvent},
    Multiaddr, PeerId, Swarm,
};
use rand::{prelude::SliceRandom, thread_rng};
use snafu::ResultExt;
use std::{
    collections::{HashMap, HashSet},
    io::Error,
    iter,
    num::NonZeroUsize,
    time::Duration,
};
use tracing::{debug, error, info, info_span, instrument, trace, warn, Instrument};

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
        self.swarm.listen_on(listen_addr).context(TransportSnafu)?;
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
    ///       TCP + DNS + Websocket + XX auth
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
        let transport: Boxed<(PeerId, StreamMuxerBox)> = gen_transport(identity.clone()).await?;
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
                        mesh_n_high: 50,
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
                .idle_timeout(Duration::from_secs(5))
                .mesh_n_high(params.mesh_n_high)
                .mesh_n_low(params.mesh_n_low)
                .mesh_outbound_min(params.mesh_outbound_min)
                .mesh_n(params.mesh_n)
                .history_length(500)
                .max_transmit_size(2 * MAX_MSG_SIZE_DM)
                // Use the (blake3) hash of a message as its ID
                .message_id_fn(message_id_fn)
                .build()
                .map_err(|s| GossipsubConfigSnafu { message: s }.build())?;

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

            // - Build a identify network behavior needed for own
            //   node connection information
            //   E.g. this will answer the question: how are other nodes
            //   seeing the peer from behind a NAT
            let identify_cfg =
                IdentifyConfig::new("HotShot/identify/1.0".to_string(), identity.public());
            let identify = Identify::new(identify_cfg);

            // - Build DHT needed for peer discovery
            let mut kconfig = KademliaConfig::default();

            if let Some(factor) = config.replication_factor {
                kconfig.set_replication_factor(factor);
            }
            let kadem = Kademlia::with_config(peer_id, MemoryStore::new(peer_id), kconfig);

            let rrconfig = RequestResponseConfig::default();

            let request_response = RequestResponse::new(
                DirectMessageCodec(),
                [(DirectMessageProtocol(), ProtocolSupport::Full)].into_iter(),
                rrconfig,
            );

            let network = NetworkDef::new(
                GossipBehaviour::new(gossipsub),
                DHTBehaviour::new(kadem, peer_id),
                identify,
                DMBehaviour::new(request_response),
                HashSet::default(),
            );
            SwarmBuilder::new(transport, network, peer_id)
                .max_negotiating_inbound_streams(20)
                .dial_concurrency_factor(std::num::NonZeroU8::new(2).unwrap())
                // .connection_event_buffer_size(1)
                // .notify_handler_buffer_size(NonZeroUsize::new(1).unwrap())
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
        msg: Result<ClientRequest, flume::RecvError>,
    ) -> Result<bool, NetworkError> {
        let behaviour = self.swarm.behaviour_mut();
        match msg {
            Ok(msg) => {
                #[allow(clippy::enum_glob_use)]
                use ClientRequest::*;
                match msg {
                    LookupPeer(pid, chan) => {
                        self.swarm.behaviour_mut().dht.lookup_peer(pid, chan);
                    }
                    GetRoutingTable(chan) => {
                        self.swarm.behaviour_mut().dht.print_routing_table();
                        if chan.send(()).is_err() {
                            warn!("Tried to notify client but client not tracking anymore");
                        }
                    }
                    PutDHT { key, value, notify } => {
                        let query = KadPutQuery {
                            progress: DHTProgress::NotStarted,
                            notify,
                            key,
                            value,
                            backoff: ExponentialBackoff::default(),
                        };
                        self.swarm.behaviour_mut().put_record(query);
                    }
                    GetConnectedPeerNum(s) => {
                        if s.send(self.num_connected()).is_err() {
                            error!("error sending peer number to client");
                        }
                    }
                    GetConnectedPeers(s) => {
                        if s.send(self.connected_pids()).is_err() {
                            error!("error sending peer set to client");
                        }
                    }
                    GetDHT { key, notify } => {
                        self.swarm.behaviour_mut().get_record(
                            key,
                            notify,
                            NonZeroUsize::new(NUM_REPLICATED_TO_TRUST).unwrap(),
                        );
                    }
                    IgnorePeers(_peers) => {
                        // NOTE used by test with conductor only
                    }
                    Shutdown => {
                        warn!("Libp2p listener shutting down");
                        return Ok(true);
                    }
                    GossipMsg(topic, contents) => {
                        behaviour.publish_gossip(Topic::new(topic), contents);
                    }
                    Subscribe(t, chan) => {
                        behaviour.subscribe_gossip(&t);
                        if let Some(chan) = chan {
                            if chan.send(()).is_err() {
                                error!("finished subscribing but response channel dropped");
                            }
                        }
                    }
                    Unsubscribe(t, chan) => {
                        behaviour.unsubscribe_gossip(&t);
                        if chan.send(()).is_err() {
                            error!("finished unsubscribing but response channel dropped");
                        }
                    }
                    DirectRequest(pid, msg) => {
                        error!("pid {:?} adding direct request", self.peer_id);
                        behaviour.add_direct_request(pid, msg);
                    }
                    DirectResponse(chan, msg) => {
                        behaviour.add_direct_response(chan, msg);
                    }
                    AddKnownPeers(peers) => {
                        self.add_known_peers(&peers);
                    }
                    Prune(pid) => {
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
        event: SwarmEvent<
            NetworkEvent,
            EitherError<
                EitherError<EitherError<GossipsubHandlerError, Error>, Error>,
                ConnectionHandlerUpgrErr<Error>,
            >,
        >,
        send_to_client: &Sender<NetworkEvent>,
    ) -> Result<(), NetworkError> {
        // Make the match cleaner
        #[allow(clippy::enum_glob_use)]
        use SwarmEvent::*;
        info!("event observed {:?}", event);

        match event {
            ConnectionEstablished {
                peer_id,
                endpoint,
                num_established,
                concurrent_dial_errors,
            } => {
                info!("peerid {:?} connection is established to {:?} with endpoint {:?} with concurrent dial errors {:?}. {:?} connections left", self.peer_id, peer_id, endpoint, concurrent_dial_errors, num_established);
            }
            ConnectionClosed {
                peer_id,
                endpoint,
                num_established,
                cause,
            } => {
                info!("peerid {:?} connection is closed to {:?} with endpoint {:?} with cause {:?}. {:?} connections left", self.peer_id, peer_id, endpoint, cause, num_established);
            }
            Dialing(p) => {
                info!("{:?} is dialing {:?}", self.peer_id, p);
            }
            BannedPeer {
                peer_id,
                endpoint: _,
            } => {
                error!("Peer {:?} is banning peer {:?}!!", self.peer_id, peer_id);
            }
            ListenerClosed {
                listener_id: _,
                addresses: _,
                reason: _,
            }
            | NewListenAddr {
                listener_id: _,
                address: _,
            }
            | ExpiredListenAddr {
                listener_id: _,
                address: _,
            }
            | IncomingConnection {
                local_addr: _,
                send_back_addr: _,
            } => {}
            Behaviour(b) => {
                // forward messages directly to Client
                send_to_client
                    .send_async(b)
                    .await
                    .map_err(|_e| NetworkError::StreamClosed)?;
            }
            OutgoingConnectionError { peer_id: _, error } => {
                info!(?error, "OUTGOING CONNECTION ERROR, {:?}", error);
            }
            IncomingConnectionError {
                local_addr,
                send_back_addr,
                error,
            } => {
                info!(
                    "INCOMING CONNECTION ERROR: {:?} {:?} {:?}",
                    local_addr, send_back_addr, error
                );
            }
            ListenerError { listener_id, error } => {
                info!("LISTENER ERROR {:?} {:?}", listener_id, error);
            }
        }
        Ok(())
    }

    /// Spawn a task to listen for requests on the returned channel
    /// as well as any events produced by libp2p
    #[allow(clippy::panic)]
    #[instrument]
    pub async fn spawn_listeners(
        mut self,
    ) -> Result<(Sender<ClientRequest>, Receiver<NetworkEvent>), NetworkError> {
        let (s_input, s_output) = unbounded::<ClientRequest>();
        let (r_input, r_output) = unbounded::<NetworkEvent>();
        let mut time_since_print_num_connections = Duration::ZERO;

        let print_num_connections_thres = Duration::from_secs(10);

        let lowest_increment = Duration::from_millis(5);

        spawn(
            async move {
                loop {
                    select! {
                        // TODO move this out of th eevent loop and into the handle_client_requests
                        // event loop?
                        // <https://github.com/EspressoSystems/hotshot/issues/291>
                        _ = sleep(lowest_increment).fuse() => {
                            time_since_print_num_connections += lowest_increment;
                            if time_since_print_num_connections > print_num_connections_thres {
                                info!("peer id {:?} state is {:?} expired {:?} overall state {:?}", self.swarm.behaviour().dht.peer_id, self.swarm.behaviour().dht.bootstrap_state.state, self.swarm.behaviour().dht.bootstrap_state.backoff.is_expired(), self.swarm.behaviour().dht.bootstrap_state);
                                let peers = self.swarm.connected_peers().collect::<Vec<_>>();
                                info!("num connections is len {} for peer {:?}, which are {:?}", peers.len(), self.peer_id.clone(), peers);
                                info!("connection info is: {:?} for peer {:?}", self.swarm.network_info(), self.peer_id.clone());
                                time_since_print_num_connections = Duration::from_secs(0);
                            }
                        },
                        event = self.swarm.next() => {
                            if let Some(event) = event {
                                info!("peerid {:?}\t\thandling event {:?}", self.peer_id, event);
                                self.handle_swarm_events(event, &r_input).await?;
                            }
                        },
                        msg = s_output.recv_async() => {
                            let shutdown = self.handle_client_requests(msg).await?;
                            if shutdown {
                                break
                            }
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
