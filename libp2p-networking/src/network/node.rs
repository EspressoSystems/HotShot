mod config;
mod handle;

pub use self::{
    config::{NetworkNodeConfig, NetworkNodeConfigBuilder, NetworkNodeConfigBuilderError},
    handle::{network_node_handle_error, NetworkNodeHandle, NetworkNodeHandleError},
};
use super::{
    error::{GossipsubBuildSnafu, GossipsubConfigSnafu, NetworkError, TransportSnafu},
    gen_transport, ClientRequest, ConnectionData, NetworkDef, NetworkEvent, NetworkNodeType,
};
use crate::{
    direct_message::{DirectMessageCodec, DirectMessageProtocol, MAX_MSG_SIZE},
    network::def::{DHTProgress, KadPutQuery, NUM_REPLICATED_TO_TRUST},
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
    swarm::{ConnectionHandlerUpgrErr, SwarmEvent, SwarmBuilder, ConnectionLimits},
    Multiaddr, PeerId, Swarm,
};
use phaselock_utils::subscribable_rwlock::{SubscribableRwLock, ReadView};
use rand::{seq::IteratorRandom, thread_rng};
use snafu::ResultExt;
use std::{collections::HashSet, io::Error, iter, num::NonZeroUsize, sync::Arc, time::Duration};
use tracing::{debug, error, info, info_span, instrument, trace, warn, Instrument};

pub const PRIORITY_PEER_EXCESS: f32 = 0.2;
pub const MIN_OUTBOUND_ONLY_FACTOR: f32 = 0.2;
pub const TARGET_OUTBOUND_ONLY_FACTOR: f32 = 0.3;
pub const PEER_EXCESS_FACTOR: f32 = 0.1;

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
    /// Return a reference to the network
    #[instrument(skip(self))]
    fn connection_data(&mut self) -> Arc<SubscribableRwLock<ConnectionData>> {
        self.swarm.behaviour().connection_data()
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
        let behaviour = self.swarm.behaviour_mut();
        for (peer_id, addr) in known_peers {
            match peer_id {
                Some(peer_id) => {
                    // if we know the peerid, add address.
                    // if we don't know the peerid, dial to find out what the peerid is
                    if *peer_id != self.peer_id {
                        // FIXME why can't I pattern match this?
                        behaviour.add_address(peer_id, addr.clone());
                    }
                    behaviour.add_known_peer(*peer_id);
                }
                None => {
                    behaviour.add_unknown_address(addr.clone());
                }
            }
        }
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
    pub async fn new(mut config: NetworkNodeConfig) -> Result<Self, NetworkError> {
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
        let swarm: Swarm<NetworkDef> = {
            // Use the hash of the message's contents as the ID
            // Use blake3 for much paranoia at very high speeds
            let message_id_fn = |message: &GossipsubMessage| {
                let hash = blake3::hash(&message.data);
                MessageId::from(hash.as_bytes().to_vec())
            };
            // Create a custom gossipsub
            // TODO: Extract these defaults into some sort of config
            // mesh_outbound_min <= mesh_n_low <= mesh_n <= mesh_n_high
            // mesh_outbound_min <= self.config.mesh_n / 2
            let gossipsub_config = GossipsubConfigBuilder::default()
                .heartbeat_interval(Duration::from_secs(3))
                // Force all messages to have valid signatures
                .validation_mode(ValidationMode::Strict)
                .history_gossip(50)
                .idle_timeout(Duration::from_secs(1))
                .mesh_n_high(5)
                .mesh_n_low(2)
                .mesh_outbound_min(1)
                .mesh_n(4)
                .history_length(500)
                .max_transmit_size(2 * MAX_MSG_SIZE)
                // Use the (blake3) hash of a message as its ID
                .message_id_fn(message_id_fn)
                .build()
                .map_err(|s| GossipsubConfigSnafu { message: s }.build())?;
            // - Build a gossipsub network behavior
            let gossipsub: Gossipsub = Gossipsub::new(
                // TODO do we even need this?
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
            let identify = Identify::new(IdentifyConfig::new(
                "Phaselock validation gossip 0.1".to_string(),
                identity.public(),
            ));

            // - Build DHT needed for peer discovery
            let mut kconfig = KademliaConfig::default();
            kconfig.set_connection_idle_timeout(Duration::from_secs(1));

            if let Some(factor) = config.replication_factor {
                kconfig.set_replication_factor(factor);
            }
            let kadem = Kademlia::with_config(peer_id, MemoryStore::new(peer_id), kconfig);

            // request response for direct messages
            let request_response = RequestResponse::new(
                DirectMessageCodec(),
                iter::once((DirectMessageProtocol(), ProtocolSupport::Full)),
                RequestResponseConfig::default(),
            );

            let pruning_enabled = config.node_type == NetworkNodeType::Regular || config.node_type == NetworkNodeType::Bootstrap;

            let (limits, num_incoming, num_outgoing) = match config.node_type {
                NetworkNodeType::Bootstrap => {
                    (50, 25, 25)
                },
                NetworkNodeType::Regular => {
                    (10, 5, 5)
                },
                NetworkNodeType::Conductor => {
                    (5000, 2500, 2500)
                },
            };

            let network = NetworkDef::new(
                gossipsub,
                kadem,
                identify,
                request_response,
                pruning_enabled,
                config.ignored_peers.clone(),
                config.to_connect_addrs.clone(),
                (limits, num_incoming, num_outgoing),
            );
            config.max_num_peers = limits as usize;



                // if config.nod == 0 {
                //     5000
                // } else {
                //     config.max_num_peers as u32 * 5
                // };




            // TODO READD
            let connection_limits =
                ConnectionLimits::default()
                .with_max_pending_incoming(Some(num_incoming))
                // we are not going to connect to everyone.
                .with_max_pending_outgoing(Some(num_outgoing))
                .with_max_established_incoming(Some(
                    (limits as f32
                        * (1.0 + PEER_EXCESS_FACTOR - MIN_OUTBOUND_ONLY_FACTOR))
                        .ceil() as u32,
                ))
                .with_max_established_outgoing(Some(
                    (limits as f32 * (1.0 + PEER_EXCESS_FACTOR)).ceil() as u32,
                ))
                .with_max_established(Some(
                    (limits as f32 * (1.0 + PEER_EXCESS_FACTOR + PRIORITY_PEER_EXCESS))
                        .ceil() as u32,
                ))
                .with_max_established_per_peer(Some(2))
                ;

            SwarmBuilder::new(transport, network, peer_id)
                .connection_limits(connection_limits)
                .notify_handler_buffer_size(NonZeroUsize::new(32).unwrap())
                .connection_event_buffer_size(1024)
                .build()
            // Swarm::new(transport, network, peer_id)

        };


        Ok(Self {
            identity,
            peer_id,
            swarm,
            config,
        })
    }

    /// Active peer discovery mechanism. It does this by looking up a random peer
    /// - must be bootstrapped
    /// - must not have such a query in progress.
    #[instrument(skip(self))]
    fn handle_peer_discovery(&mut self) {
        // if self.swarm.behaviour().is_bootstrapped()
        // &&
        if !self.swarm.behaviour().is_discovering_peers() {
            let random_peer = PeerId::random();
            self.swarm.behaviour_mut().query_closest_peers(random_peer);
            self.swarm.behaviour_mut().set_discovering_peers(true);
        }
    }

    /// Keep the number of open connections between threshold specified by
    /// the swarm
    #[instrument(skip(self))]
    fn handle_num_connections(&mut self) {
        let used_peers = self.swarm.behaviour().get_peers();

        if self.swarm.behaviour_mut().bootstrap().is_err() {
            warn!("Failed to bootstrap. Trying again later.");
        }

        if used_peers.len() < self.swarm.behaviour().to_connect_addrs.len() - 3
            // && (self.config.node_type == NetworkNodeType::Regular
            // || self.config.node_type == NetworkNodeType::Conductor)
        {
            // let potential_peers: HashSet<PeerId> = self
            //     .swarm
            //     .behaviour()
            //     .to_connect_peers
            //     .difference(&used_peers)
            //     .copied()
            //     .collect();
            //
            // for a_peer in &potential_peers {
            //     if *a_peer != self.peer_id {
            //         match self.swarm.dial(*a_peer) {
            //             Ok(_) => {
            //                 info!("Peer {:?} dial {:?} working!", self.peer_id, a_peer);
            //                 self.swarm.behaviour_mut().add_connecting_peer(*a_peer);
            //             }
            //             Err(e) => {
            //                 info!("Peer {:?} dial {:?} failed: {:?}", self.peer_id, a_peer, e);
            //             }
            //         };
            //     }
            // }
            //
            // if potential_peers.is_empty() {
                for a_peer in &self.swarm.behaviour().to_connect_addrs.clone() {
                    match self.swarm.dial(a_peer.clone()) {
                        Ok(_) => {
                            // info!("Peer {:?} dial {:?} working!", self.peer_id, a_peer);
                            // self.swarm.behaviour_mut().add_connecting_peer(*a_peer);
                        }
                        Err(e) => {
                            info!("Peer {:?} dial {:?} failed: {:?}", self.peer_id, a_peer, e);
                        }
                    };
                }
            // }
        }

        // otherwise periodically get more peers if needed
        // if used_peers.len() <= self.config.min_num_peers
        //     // TODO matches macro
        //     && (self.config.node_type == NetworkNodeType::Regular
        //     || self.config.node_type == NetworkNodeType::Conductor)
        // {
        //     // Calcuate the list of "new" peers, once not currently used for
        //     // a connection
        //     let potential_peers: HashSet<PeerId> = self
        //         .swarm
        //         .behaviour()
        //         .known_peers()
        //         .difference(&used_peers)
        //         .copied()
        //         .collect();
        //     // Number of peers we want to try connecting to
        //     let num_to_connect = self.config.min_num_peers + 1 - used_peers.len();
        //     // Random(?) subset of the availible peers to try connecting to
        //     let mut chosen_peers = potential_peers
        //         .iter()
        //         .copied()
        //         .choose_multiple(&mut thread_rng(), num_to_connect)
        //         .into_iter()
        //         .collect::<HashSet<_>>();
        //     chosen_peers.remove(&self.peer_id);
        //
        //     // Try dialing each random peer
        //     for a_peer in &chosen_peers {
        //         if *a_peer != self.peer_id {
        //             match self.swarm.dial(*a_peer) {
        //                 Ok(_) => {
        //                     info!("Peer {:?} dial {:?} working!", self.peer_id, a_peer);
        //                     self.swarm.behaviour_mut().add_connecting_peer(*a_peer);
        //                 }
        //                 Err(e) => {
        //                     info!("Peer {:?} dial {:?} failed: {:?}", self.peer_id, a_peer, e);
        //                 }
        //             };
        //         }
        //     }

        // if we don't know any peers, start dialing random peers if we have any.
        // if chosen_peers.is_empty() {
        //     let chosen_addrs = self
        //         .swarm
        //         .behaviour()
        //         .iter_unknown_addressess()
        //         .choose_multiple(&mut thread_rng(), num_to_connect);
        //     for a_addr in chosen_addrs {
        //         match self.swarm.dial(a_addr.clone()) {
        //             Ok(_) => {
        //                 info!("Peer {:?} dial {:?} working!", self.peer_id, a_addr);
        //             }
        //             Err(e) => {
        //                 info!("Peer {:?} dial {:?} failed: {:?}", self.peer_id, a_addr, e);
        //             }
        //         }
        //     }
        // }
        //
        // if chosen_peers.is_empty() {
        //     // self.swarm.behaviour().
        //     let chosen_addrs = self
        //         .swarm
        //         .behaviour()
        //         .iter_unknown_addressess()
        //         .choose_multiple(&mut thread_rng(), num_to_connect);
        //     for a_addr in chosen_addrs {
        //         match self.swarm.dial(a_addr.clone()) {
        //             Ok(_) => {
        //                 info!("Peer {:?} dial {:?} working!", self.peer_id, a_addr);
        //             }
        //             Err(e) => {
        //                 info!("Peer {:?} dial {:?} failed: {:?}", self.peer_id, a_addr, e);
        //             }
        //         }
        //     }
        // }
    }

    #[instrument(skip(self))]
    fn prune_num_connections(&mut self) {
        error!("CALLING PRUNE!!!!!");
        let swarm = self.swarm.behaviour_mut();
        // If we are connected to too many peers, try disconnecting from
        // a random subset. Bootstrap nodes accept all connections and
        // attempt to connect to all
        if self.config.node_type != NetworkNodeType::Conductor {
            error!("PRUNING TIME!");
            let peers_to_prune = swarm.get_peers_to_prune(self.config.max_num_peers) ;
            error!("pruning {} peers, connected to {} peers", peers_to_prune.len(), swarm.connection_data().cloned().connected_peers.len());
            for peer_id in peers_to_prune {
                let _ = self.swarm.disconnect_peer_id(peer_id);
                error!("Disconnected a peer. {} left", self.connection_data().cloned().connected_peers.len());
            }
        }
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
                    PutDHT { key, value, notify } => {
                        let query = KadPutQuery {
                            progress: DHTProgress::NotStarted,
                            notify,
                            key,
                            value,
                        };
                        self.swarm.behaviour_mut().put_record(query);
                    }
                    GetDHT { key, notify } => {
                        self.swarm.behaviour_mut().get_record(
                            key,
                            notify,
                            NonZeroUsize::new(NUM_REPLICATED_TO_TRUST).unwrap(),
                            );
                    }
                    IgnorePeers(peers) => {
                        behaviour.extend_ignored_peers(peers);
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
                        behaviour.add_direct_request(pid, msg);
                    }
                    DirectResponse(chan, msg) => {
                        behaviour.add_direct_response(chan, msg);
                    }
                    Pruning(is_enabled) => {
                        behaviour.toggle_pruning(is_enabled);
                    }
                    AddKnownPeers(peers) => {
                        self.add_known_peers(&peers);
                    },
                    Prune(pid) => {
                        //FIXME deal with error handling
                        let _ = self.swarm.disconnect_peer_id(pid);
                    }
                }
            }
            Err(e) => {
                error!("Error receiving msg: {:?}", e);
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
        // if we want to comment out identify
        // EitherError</* EitherError< */GossipsubHandlerError/* , Error *//* > */, Error>,
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
        let behaviour = self.swarm.behaviour_mut();

        match event {
            ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                behaviour.add_connected_peer(peer_id);

                error!("add address is called here?");
                behaviour.add_address(&peer_id, endpoint.get_remote_address().clone());
                let tmp = behaviour.connection_data().cloned();
                error!("connection established!! {} connected: {:?} ",
                       tmp.connected_peers.len(), tmp.connected_peers);

                // now we have at least one peer so we can bootstrap
                if behaviour.should_bootstrap() {
                    behaviour
                        .bootstrap()
                        .map_err(|_e| NetworkError::NoKnownPeers)?;
                }
            }
            ConnectionClosed {
                peer_id,
                endpoint: _,
                ..
            } => {
                info!("connection closed btwn {:?}, {:?}", self.peer_id, peer_id);
                behaviour.remove_connected_peer(peer_id);
                // FIXME remove stale address, not *all* addresses
                // swarm.kadem.remove_peer(&peer_id);
                // swarm.kadem.remove_address();
                // swarm.request_response.remove_address(peer, address)
            }
            Dialing(_)
            | NewListenAddr { .. }
            | ExpiredListenAddr { .. }
            | ListenerClosed { .. }
            | IncomingConnection { .. }
            | BannedPeer { .. }
            | IncomingConnectionError { .. }
            | ListenerError { .. } => {}
            Behaviour(b) => {
                // forward messages directly to Client
                send_to_client
                    .send_async(b)
                    .await
                    .map_err(|_e| NetworkError::StreamClosed)?;
            }
            OutgoingConnectionError { peer_id, error } => {
                warn!(?error);
                if let Some(peer_id) = peer_id {
                    behaviour.remove_peer(peer_id);
                }
            }
        }
        Ok(())
    }

    /// periodically resend gossip messages if they have failed
    #[allow(clippy::panic)]
    #[instrument(skip(self))]
    pub fn handle_sending(&mut self) {
        self.swarm.behaviour_mut().drain_publish_gossips();
        self.swarm.behaviour_mut().drain_rr();
    }

    /// periodically retry put requests to dht if they've failed
    #[allow(clippy::panic)]
    #[instrument(skip(self))]
    pub fn retry_put_dht(&mut self) {
        self.swarm.behaviour_mut().retry_put_dht();
    }

    /// Spawn a task to listen for requests on the returned channel
    /// as well as any events produced by libp2p
    #[allow(clippy::panic)]
    #[instrument(skip(self))]
    pub async fn spawn_listeners(
        mut self,
        ) -> Result<(Sender<ClientRequest>, Receiver<NetworkEvent>), NetworkError> {
        let (s_input, s_output) = unbounded::<ClientRequest>();
        let (r_input, r_output) = unbounded::<NetworkEvent>();
        // NOTE this could be entirely event driven
        // by having one future sleep then send a message to
        // a separate future which would then execute these
        // periodic handlers
        // while verbose, the same effect may be achieved
        // while avoiding the overhead of more futures + channels
        let mut time_since_sending = Duration::ZERO;
        let mut time_since_peer_discovery = Duration::ZERO;
        let mut time_since_prune_num_connections = Duration::ZERO;
        let mut time_since_retry_put_dht = Duration::ZERO;
        let mut time_since_handle_num_connections = Duration::ZERO;

        let sending_thresh = Duration::from_millis(250);
        let peer_discovery_thresh = Duration::from_millis(250);
        let prune_num_connections_thresh = Duration::from_secs(2);
        let retry_put_dht_thresh = Duration::from_millis(250);
        let handle_num_connections_thresh = Duration::from_secs(1);

        let lowest_increment = Duration::from_millis(50);

        spawn(
            async move {
                loop {
                    select! {
                        _ = sleep(lowest_increment).fuse() => {
                            time_since_sending += lowest_increment;
                            time_since_peer_discovery += lowest_increment;
                            time_since_prune_num_connections += lowest_increment;
                            time_since_retry_put_dht += lowest_increment;
                            time_since_handle_num_connections += lowest_increment;

                            if time_since_sending >= sending_thresh {
                                self.handle_sending();
                                time_since_sending = Duration::ZERO;
                            }

                            if time_since_peer_discovery >= peer_discovery_thresh {
                                self.handle_peer_discovery();
                                time_since_peer_discovery = Duration::ZERO;
                            }


                            if time_since_prune_num_connections >= prune_num_connections_thresh {
                                self.prune_num_connections();
                                time_since_prune_num_connections = Duration::ZERO;
                            }

                            if time_since_retry_put_dht >= retry_put_dht_thresh {
                                // FIXME uncommment soon
                                // self.retry_put_dht();
                                time_since_retry_put_dht = Duration::ZERO;
                            }

                            if time_since_handle_num_connections >= handle_num_connections_thresh {
                                self.handle_num_connections();
                                time_since_handle_num_connections = Duration::ZERO;
                            } else {
                                error!("waiting to handle num connections {:?} {:?}", time_since_handle_num_connections, handle_num_connections_thresh);
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
