mod config;
mod handle;

use crate::network::{behaviours::{direct_message_codec::{MAX_MSG_SIZE, DirectMessageCodec, DirectMessageProtocol}, dht::{DHTBehaviour, KadPutQuery, DHTProgress}, direct_message::DMBehaviour, exponential_backoff::ExponentialBackoff}, def::NUM_REPLICATED_TO_TRUST};

pub use self::{
    config::{NetworkNodeConfig, NetworkNodeConfigBuilder, NetworkNodeConfigBuilderError},
    handle::{network_node_handle_error, NetworkNodeHandle, NetworkNodeHandleError},
};
use super::{
    error::{GossipsubBuildSnafu, GossipsubConfigSnafu, NetworkError, TransportSnafu},
    gen_transport, ClientRequest, ConnectionData, NetworkDef, NetworkEvent, NetworkNodeType, behaviours::gossip::GossipBehaviour,
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
    swarm::{ConnectionHandlerUpgrErr, SwarmEvent, SwarmBuilder},
    Multiaddr, PeerId, Swarm,
};
use phaselock_utils::subscribable_rwlock::{SubscribableRwLock, ReadView};

use snafu::ResultExt;
use std::{io::Error, num::NonZeroUsize, sync::Arc, time::Duration};
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

            // TODO insert into config.
            let (mesh_n_high, mesh_n_low, mesh_outbound_min, mesh_n) = match config.node_type {
                NetworkNodeType::Bootstrap => {
                    (50, 4, 2, 12)
                },
                NetworkNodeType::Regular => {
                    (20, 4, 2, 12)
                },
                NetworkNodeType::Conductor => {
                    // (1000, 50, 2, 100)
                    (20, 4, 2, 12)
                },
            };

            // NOTE: can vary on obvious basis

            // Create a custom gossipsub
            // TODO: Extract these defaults into some sort of config
            // mesh_outbound_min <= mesh_n_low <= mesh_n <= mesh_n_high
            // mesh_outbound_min <= self.config.mesh_n / 2
            let gossipsub_config = GossipsubConfigBuilder::default()
                .heartbeat_interval(Duration::from_secs(1))
                // Force all messages to have valid signatures
                .validation_mode(ValidationMode::Strict)
                .history_gossip(50)
                .idle_timeout(Duration::from_secs(0))
                .mesh_n_high(mesh_n_high)
                .mesh_n_low(mesh_n_low)
                .mesh_outbound_min(mesh_outbound_min)
                .mesh_n(mesh_n)
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
            let mut identify_cfg = IdentifyConfig::new(
                "Phaselock validation gossip 0.1".to_string(),
                identity.public(),
            );
            identify_cfg.cache_size = 3;
            // identify_cfg.push_listen_addr_updates = true;
            let identify = Identify::new(identify_cfg);

            // - Build DHT needed for peer discovery
            let mut kconfig = KademliaConfig::default();
            kconfig.set_connection_idle_timeout(Duration::from_secs(0));
            kconfig.set_query_timeout(Duration::from_secs(1));

            if let Some(factor) = config.replication_factor {
                kconfig.set_replication_factor(factor);
            }
            let kadem = Kademlia::with_config(peer_id, MemoryStore::new(peer_id), kconfig);

            let mut rrconfig = RequestResponseConfig::default();
            rrconfig.set_request_timeout(Duration::from_secs(1));
            rrconfig.set_connection_keep_alive(Duration::from_secs(0));

            // request response for direct messages
            let request_response = RequestResponse::new(
                DirectMessageCodec(),
                [(DirectMessageProtocol(), ProtocolSupport::Full)].into_iter(),
                rrconfig
            );

            let network = NetworkDef::new(
                GossipBehaviour::new(gossipsub),
                DHTBehaviour::new(kadem),
                identify,
                DMBehaviour::new(request_response),
                false,
                config.ignored_peers.clone(),
                config.to_connect_addrs.clone(),
            );
            config.max_num_peers = 0;
            SwarmBuilder::new(transport, network, peer_id).build()
        };


        Ok(Self {
            identity,
            peer_id,
            swarm,
            config,
        })
    }

    /// Keep the number of open connections between threshold specified by
    /// the swarm
    #[instrument(skip(self))]
    fn handle_num_connections(&mut self) {
        let used_peers = self.swarm.behaviour().get_peers();

        // if used_peers.len() < self.swarm.behaviour().to_connect_addrs.len()
        if used_peers.len() < 1
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
                            backoff: ExponentialBackoff::default()
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
                    Pruning(_is_enabled) => {
                        // FIXME chop this behaviour
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
                behaviour.add_address(&peer_id, endpoint.get_remote_address().clone());
                let tmp = behaviour.connection_data().cloned();
                error!("connection established!! {} connected: {:?} ",
                       tmp.connected_peers.len(), tmp.connected_peers);
            }
            ConnectionClosed {
                peer_id,
                endpoint: e,
                num_established: _,
                cause: _,
            } => {
                info!("connection closed btwn {:?}, {:?}", self.peer_id, peer_id);
                behaviour.remove_connected_peer(peer_id);
                let _addr = match e {
                    libp2p::core::ConnectedPoint::Dialer { address, role_override: _ } => address,
                    libp2p::core::ConnectedPoint::Listener { local_addr: _, send_back_addr } => send_back_addr,
                };
                // self.swarm.
                // FIXME remove stale address, not *all* addresses
                // behaviour.dht.remove_address(&peer_id, &addr);
            }
            Dialing(_)
            | NewListenAddr { .. }
            | ExpiredListenAddr { .. }
            | ListenerClosed { .. }
            | IncomingConnection { .. }
            | BannedPeer { .. } => {},
            Behaviour(b) => {
                // forward messages directly to Client
                send_to_client
                    .send_async(b)
                    .await
                    .map_err(|_e| NetworkError::StreamClosed)?;
            }
            OutgoingConnectionError { peer_id, error } => {
                error!(?error, "OUTGOING CONNECTION ERROR, {:?}", error);
                if let Some(peer_id) = peer_id {
                    behaviour.remove_peer(peer_id);
                }
            }
            IncomingConnectionError { local_addr, send_back_addr, error } => {
                // behaviour.kadem.remove_address(&peer_id, &send_back_addr);
                error!("INCOMING CONNECTION ERROR: {:?} {:?} {:?}", local_addr, send_back_addr, error);
            }
            ListenerError { listener_id, error } => {
                // behaviour.kadem.remove_address(&peer_id, &send_back_addr);
                error!("LISTENER ERROR {:?} {:?}", listener_id, error);
            }
        }
        Ok(())
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
        let mut time_since_print_num_connections = Duration::ZERO;

        let _sending_thresh = Duration::from_secs(1);
        let _peer_discovery_thresh = Duration::from_secs(2);
        let _prune_num_connections_thresh = Duration::from_secs(1);
        let _retry_put_dht_thresh = Duration::from_millis(250);
        let handle_num_connections_thresh = Duration::from_secs(1);
        let print_num_connections_thres = Duration::from_secs(1);

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
                            time_since_print_num_connections += lowest_increment;

                            if time_since_print_num_connections > print_num_connections_thres {
                                error!("num connections is {}", self.connection_data().cloned().connected_peers.len());
                                time_since_print_num_connections = Duration::from_secs(0);

                            }



                            if time_since_handle_num_connections >= handle_num_connections_thresh {
                                self.handle_num_connections();
                                time_since_handle_num_connections = Duration::ZERO;
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
