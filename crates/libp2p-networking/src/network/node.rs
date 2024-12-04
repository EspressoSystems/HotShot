// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

/// configuration for the libp2p network (e.g. how it should be built)
pub mod config;

/// libp2p network handle
/// allows for control over the libp2p network
mod handle;

use std::{
    collections::HashSet,
    num::{NonZeroU32, NonZeroUsize},
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context};
use config::{KademliaConfig, Libp2pConfig};
use futures::{channel::mpsc, SinkExt, StreamExt};
use hotshot_types::traits::node_implementation::NodeType;
use libp2p::{
    autonat,
    core::transport::ListenerId,
    gossipsub::{
        Behaviour as Gossipsub, ConfigBuilder as GossipsubConfigBuilder, Event as GossipEvent,
        Message as GossipsubMessage, MessageAuthenticity, MessageId, Topic, ValidationMode,
    },
    identify::{
        Behaviour as IdentifyBehaviour, Config as IdentifyConfig, Event as IdentifyEvent,
        Info as IdentifyInfo,
    },
    kad::{store::MemoryStore, Behaviour, Config, Mode, Record},
    multiaddr::Protocol,
    request_response::{
        Behaviour as RequestResponse, Config as Libp2pRequestResponseConfig, ProtocolSupport,
    },
    swarm::SwarmEvent,
    Multiaddr, StreamProtocol, Swarm, SwarmBuilder,
};
use libp2p_identity::PeerId;
use tokio::{
    select, spawn,
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    time::timeout,
};
use tracing::{debug, error, info, info_span, instrument, trace, warn, Instrument};

pub use self::{
    config::{GossipConfig, RequestResponseConfig},
    handle::{spawn_network_node, NetworkNodeHandle, NetworkNodeReceiver},
};
use super::{
    behaviours::dht::{
        bootstrap::{DHTBootstrapTask, InputEvent},
        store::{file_backed::FileBackedStore, validated::ValidatedStore},
    },
    cbor::Cbor,
    gen_transport, BoxedTransport, ClientRequest, NetworkDef, NetworkError, NetworkEvent,
    NetworkEventInternal,
};
use crate::network::behaviours::{
    dht::{DHTBehaviour, DHTProgress, KadPutQuery, NUM_REPLICATED_TO_TRUST},
    direct_message::{DMBehaviour, DMRequest},
    exponential_backoff::ExponentialBackoff,
};

/// Maximum size of a message
pub const MAX_GOSSIP_MSG_SIZE: usize = 2_000_000_000;

/// Wrapped num of connections
pub const ESTABLISHED_LIMIT: NonZeroU32 =
    unsafe { NonZeroU32::new_unchecked(ESTABLISHED_LIMIT_UNWR) };
/// Number of connections to a single peer before logging an error
pub const ESTABLISHED_LIMIT_UNWR: u32 = 10;

/// Network definition
pub struct NetworkNode<T: NodeType> {
    /// The swarm of network behaviours
    swarm: Swarm<NetworkDef<T::SignatureKey>>,
    /// The listener id we are listening on, if it is initialized
    listener_id: Option<ListenerId>,
    /// The handler for direct messages
    direct_message_handler: DMBehaviour,
    /// The handler for DHT events
    dht_handler: DHTBehaviour<T::SignatureKey>,
    /// Channel to resend requests (set when we call `spawn_listeners`)
    resend_tx: Option<UnboundedSender<ClientRequest>>,

    /// The Kademlia config
    kademlia_config: KademliaConfig<T>,
}

impl<T: NodeType> NetworkNode<T> {
    /// Returns number of peers this node is connected to
    pub fn num_connected(&self) -> usize {
        self.swarm.connected_peers().count()
    }

    /// return hashset of PIDs this node is connected to
    pub fn connected_pids(&self) -> HashSet<PeerId> {
        self.swarm.connected_peers().copied().collect()
    }

    /// Bind the swarm to a given address
    #[instrument(skip(self))]
    pub async fn bind_to(&mut self, bind_address: &Multiaddr) -> anyhow::Result<()> {
        // Debug log the address we are binding to
        debug!("Libp2p binding to {:?}", bind_address);

        // Start listening on the given address
        self.listener_id = Some(
            self.swarm
                .listen_on(bind_address.clone())
                .with_context(|| "failed to bind to address")?,
        );

        // Wait for the listener to be bound with a 60s timeout
        let start_time = Instant::now();
        loop {
            match timeout(Duration::from_secs(60), self.swarm.next()).await {
                // If we successfully get a NewListenAddr event, break
                Ok(Some(SwarmEvent::NewListenAddr { .. })) => break,
                // If we timeout, return an error
                Err(_) => {
                    return Err(anyhow!("Timed out binding to address"));
                }
                // If we get any other event, continue waiting
                _ => {}
            }

            // If we've been waiting for more than 60 seconds, return an error
            if start_time.elapsed() > Duration::from_secs(60) {
                return Err(anyhow!("Timed out binding to address"));
            }
        }

        // Log the address we are listening on
        info!("Libp2p listening on {:?}", bind_address);

        Ok(())
    }

    /// Creates a new `NetworkNode` with the given Libp2p configuration
    ///
    /// # Errors
    /// If the network node cannot be created
    ///
    /// # Panics
    /// If `5 == 0`
    #[allow(clippy::too_many_lines)]
    pub async fn new(config: &Libp2pConfig<T>) -> anyhow::Result<Self> {
        // Get the `PeerId` from the `KeyPair`
        let peer_id = PeerId::from(config.keypair.public());

        // Generate the transport from the keypair, stake table, and auth message
        let transport: BoxedTransport = gen_transport::<T>(
            config.keypair.clone(),
            config.quorum_membership.clone(),
            config.auth_message.clone(),
        )
        .await?;

        // Generate the swarm
        let mut swarm: Swarm<NetworkDef<T::SignatureKey>> = {
            // Use the `Blake3` hash of the message's contents as the ID
            let message_id_fn = |message: &GossipsubMessage| {
                let hash = blake3::hash(&message.data);
                MessageId::from(hash.as_bytes().to_vec())
            };

            // Derive a `Gossipsub` config from our gossip config
            let gossipsub_config = GossipsubConfigBuilder::default()
                .message_id_fn(message_id_fn) // Use the (blake3) hash of a message as its ID
                .validation_mode(ValidationMode::Strict) // Force all messages to have valid signatures
                .heartbeat_interval(config.gossip_config.heartbeat_interval) // Time between gossip heartbeats
                .history_gossip(config.gossip_config.history_gossip) // Number of heartbeats to gossip about
                .history_length(config.gossip_config.history_length) // Number of heartbeats to remember the full message for
                .mesh_n(config.gossip_config.mesh_n) // Target number of mesh peers
                .mesh_n_high(config.gossip_config.mesh_n_high) // Upper limit of mesh peers
                .mesh_n_low(config.gossip_config.mesh_n_low) // Lower limit of mesh peers
                .mesh_outbound_min(config.gossip_config.mesh_outbound_min) // Minimum number of outbound peers in mesh
                .max_transmit_size(config.gossip_config.max_transmit_size) // Maximum size of a message
                .max_ihave_length(config.gossip_config.max_ihave_length) // Maximum number of messages to include in an IHAVE message
                .max_ihave_messages(config.gossip_config.max_ihave_messages) // Maximum number of IHAVE messages to accept from a peer within a heartbeat
                .published_message_ids_cache_time(
                    config.gossip_config.published_message_ids_cache_time,
                ) // Cache duration for published message IDs
                .iwant_followup_time(config.gossip_config.iwant_followup_time) // Time to wait for a message requested through IWANT following an IHAVE advertisement
                .max_messages_per_rpc(config.gossip_config.max_messages_per_rpc) // The maximum number of messages we will process in a given RPC
                .gossip_retransimission(config.gossip_config.gossip_retransmission) // Controls how many times we will allow a peer to request the same message id through IWANT gossip before we start ignoring them.
                .flood_publish(config.gossip_config.flood_publish) // If enabled newly created messages will always be sent to all peers that are subscribed to the topic and have a good enough score.
                .duplicate_cache_time(config.gossip_config.duplicate_cache_time) // The time period that messages are stored in the cache
                .fanout_ttl(config.gossip_config.fanout_ttl) // Time to live for fanout peers
                .heartbeat_initial_delay(config.gossip_config.heartbeat_initial_delay) // Initial delay in each heartbeat
                .gossip_factor(config.gossip_config.gossip_factor) // Affects how many peers we will emit gossip to at each heartbeat
                .gossip_lazy(config.gossip_config.gossip_lazy) // Minimum number of peers to emit gossip to during a heartbeat
                .build()
                .map_err(|err| {
                    NetworkError::ConfigError(format!("error building gossipsub config: {err:?}"))
                })?;

            // Create the Gossipsub behaviour
            let gossipsub: Gossipsub = Gossipsub::new(
                MessageAuthenticity::Signed(config.keypair.clone()),
                gossipsub_config,
            )
            .map_err(|err| {
                NetworkError::ConfigError(format!("error building gossipsub behaviour: {err:?}"))
            })?;

            // Configure and create the Identify behaviour
            let identify_cfg =
                IdentifyConfig::new("HotShot/identify/1.0".to_string(), config.keypair.public());
            let identify = IdentifyBehaviour::new(identify_cfg);

            // Configure the Kademlia behaviour
            let mut kconfig = Config::default();
            kconfig
                .set_parallelism(NonZeroUsize::new(5).unwrap())
                .set_provider_publication_interval(config.kademlia_config.publication_interval)
                .set_publication_interval(config.kademlia_config.publication_interval)
                .set_record_ttl(config.kademlia_config.record_ttl);

            // Create the Kademlia behaviour
            let mut kadem = Behaviour::with_config(
                peer_id,
                FileBackedStore::new(
                    ValidatedStore::new(MemoryStore::new(peer_id)),
                    config.kademlia_config.file_path.clone(),
                    10,
                ),
                kconfig,
            );
            kadem.set_mode(Some(Mode::Server));

            // Use the default request response config
            let rrconfig = Libp2pRequestResponseConfig::default();

            // Create a new `cbor` codec with the given request and response sizes
            let cbor = Cbor::new(
                config.request_response_config.request_size_maximum,
                config.request_response_config.response_size_maximum,
            );

            // Create the direct message behaviour with our configured codec
            let direct_message: super::cbor::Behaviour<Vec<u8>, Vec<u8>> =
                RequestResponse::with_codec(
                    cbor,
                    [(
                        StreamProtocol::new("/HotShot/direct_message/1.0"),
                        ProtocolSupport::Full,
                    )]
                    .into_iter(),
                    rrconfig.clone(),
                );

            let autonat_config = autonat::Config {
                only_global_ips: false,
                ..Default::default()
            };

            let network = NetworkDef::new(
                gossipsub,
                kadem,
                identify,
                direct_message,
                autonat::Behaviour::new(peer_id, autonat_config),
            );

            // Build the swarm
            let swarm = SwarmBuilder::with_existing_identity(config.keypair.clone());
            let swarm = swarm.with_tokio();

            swarm
                .with_other_transport(|_| transport)
                .unwrap()
                .with_behaviour(|_| network)
                .unwrap()
                .build()
        };

        // Add the known peers to Libp2p
        let mut num_known_peers_added = 0;
        for mut known_peer in config.known_peers.clone() {
            // Lob off the last protocol from the multiaddr
            if let Some(protocol) = known_peer.pop() {
                // Make sure it is the P2P protocol (which includes the peer id)
                if let Protocol::P2p(peer_id) = protocol {
                    // Add the address to Libp2p behaviors
                    swarm.behaviour_mut().add_address(&peer_id, known_peer);
                    num_known_peers_added += 1;
                } else {
                    warn!("Known peer {:?} has no P2P address", known_peer);
                }
            } else {
                warn!("Known peer {:?} has no address", known_peer);
            }
        }

        // If we hadn't found any suitable known peers, return an error
        if num_known_peers_added == 0 {
            return Err(anyhow!("No suitable known peers known"));
        }

        // Create our own DHT handler
        let dht_handler = DHTBehaviour::new();

        // Create and return the new network node
        Ok(Self {
            swarm,
            listener_id: None,
            direct_message_handler: DMBehaviour::default(),
            dht_handler,
            resend_tx: None,
            kademlia_config: config.kademlia_config.clone(),
        })
    }

    /// Publish a key/value to the record store.
    ///
    /// # Panics
    /// If the default replication factor is `None`
    pub fn put_record(&mut self, mut query: KadPutQuery) {
        let record = Record::new(query.key.clone(), query.value.clone());
        match self.swarm.behaviour_mut().dht.put_record(
            record,
            libp2p::kad::Quorum::N(
                NonZeroUsize::try_from(self.kademlia_config.replication_factor / 2)
                    .expect("replication factor should be bigger than 0"),
            ),
        ) {
            Err(e) => {
                // failed try again later
                query.progress = DHTProgress::NotStarted;
                query.backoff.start_next(false);
                warn!("Error publishing to DHT: {e:?}");
            }
            Ok(qid) => {
                debug!("Published record to DHT with qid {:?}", qid);
                let query = KadPutQuery {
                    progress: DHTProgress::InProgress(qid),
                    ..query
                };
                self.dht_handler.put_record(qid, query);
            }
        }
    }

    /// event handler for client events
    /// currently supported actions include
    /// - shutting down the swarm
    /// - gossipping a message to known peers on the `global` topic
    /// - returning the id of the current peer
    /// - subscribing to a topic
    /// - unsubscribing from a toipc
    /// - direct messaging a peer
    #[instrument(skip(self))]
    async fn handle_client_requests(
        &mut self,
        msg: Option<ClientRequest>,
    ) -> Result<bool, NetworkError> {
        let behaviour = self.swarm.behaviour_mut();
        match msg {
            Some(msg) => {
                match msg {
                    ClientRequest::BeginBootstrap => {
                        debug!("Beginning Libp2p bootstrap");
                        let _ = self.swarm.behaviour_mut().dht.bootstrap();
                    }
                    ClientRequest::LookupPeer(pid, chan) => {
                        let id = self.swarm.behaviour_mut().dht.get_closest_peers(pid);
                        self.dht_handler
                            .in_progress_get_closest_peers
                            .insert(id, chan);
                    }
                    ClientRequest::GetRoutingTable(chan) => {
                        self.dht_handler
                            .print_routing_table(&mut self.swarm.behaviour_mut().dht);
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
                        self.put_record(query);
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
                        self.dht_handler.get_record(
                            key,
                            notify,
                            NonZeroUsize::new(NUM_REPLICATED_TO_TRUST).unwrap(),
                            ExponentialBackoff::default(),
                            retry_count,
                            &mut self.swarm.behaviour_mut().dht,
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
                        debug!("Sending direct request to {:?}", pid);
                        let id = behaviour.add_direct_request(pid, contents.clone());
                        let req = DMRequest {
                            peer_id: pid,
                            data: contents,
                            backoff: ExponentialBackoff::default(),
                            retry_count,
                        };
                        self.direct_message_handler.add_direct_request(req, id);
                    }
                    ClientRequest::DirectResponse(chan, msg) => {
                        behaviour.add_direct_response(chan, msg);
                    }
                    ClientRequest::Prune(pid) => {
                        if self.swarm.disconnect_peer_id(pid).is_err() {
                            warn!("Could not disconnect from {:?}", pid);
                        }
                    }
                }
            }
            None => {
                error!("Error receiving msg in main behaviour loop: channel closed");
            }
        }
        Ok(false)
    }

    /// event handler for events emitted from the swarm
    #[allow(clippy::type_complexity)]
    #[instrument(skip(self))]
    async fn handle_swarm_events(
        &mut self,
        event: SwarmEvent<NetworkEventInternal>,
        send_to_client: &UnboundedSender<NetworkEvent>,
    ) -> Result<(), NetworkError> {
        // Make the match cleaner
        debug!("Swarm event observed {:?}", event);

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
                    debug!(
                        "Connection established with {:?} at {:?} with {:?} concurrent dial errors",
                        peer_id, endpoint, concurrent_dial_errors
                    );
                }

                // Send the number of connected peers to the client
                send_to_client
                    .send(NetworkEvent::ConnectedPeersUpdate(self.num_connected()))
                    .map_err(|err| NetworkError::ChannelSendError(err.to_string()))?;
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
                    debug!(
                        "Connection closed with {:?} at {:?} due to {:?}",
                        peer_id, endpoint, cause
                    );
                }

                // Send the number of connected peers to the client
                send_to_client
                    .send(NetworkEvent::ConnectedPeersUpdate(self.num_connected()))
                    .map_err(|err| NetworkError::ChannelSendError(err.to_string()))?;
            }
            SwarmEvent::Dialing {
                peer_id,
                connection_id: _,
            } => {
                debug!("Attempting to dial {:?}", peer_id);
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
            | SwarmEvent::ExternalAddrExpired { .. }
            | SwarmEvent::IncomingConnection {
                connection_id: _,
                local_addr: _,
                send_back_addr: _,
            } => {}
            SwarmEvent::Behaviour(b) => {
                let maybe_event = match b {
                    NetworkEventInternal::DHTEvent(e) => self
                        .dht_handler
                        .dht_handle_event(e, self.swarm.behaviour_mut().dht.store_mut()),
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
                                    observed_addr: _,
                                },
                        } = *e
                        {
                            let behaviour = self.swarm.behaviour_mut();

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
                            debug!("Peer {:?} subscribed to topic {:?}", peer_id, topic);
                            None
                        }
                        GossipEvent::Unsubscribed { peer_id, topic } => {
                            debug!("Peer {:?} unsubscribed from topic {:?}", peer_id, topic);
                            None
                        }
                        GossipEvent::GossipsubNotSupported { peer_id } => {
                            warn!("Peer {:?} does not support gossipsub", peer_id);
                            None
                        }
                    },
                    NetworkEventInternal::DMEvent(e) => self
                        .direct_message_handler
                        .handle_dm_event(e, self.resend_tx.clone()),
                    NetworkEventInternal::AutonatEvent(e) => {
                        match e {
                            autonat::Event::InboundProbe(_) => {}
                            autonat::Event::OutboundProbe(e) => match e {
                                autonat::OutboundProbeEvent::Request { .. }
                                | autonat::OutboundProbeEvent::Response { .. } => {}
                                autonat::OutboundProbeEvent::Error {
                                    probe_id: _,
                                    peer,
                                    error,
                                } => {
                                    warn!(
                                        "AutoNAT Probe failed to peer {:?} with error: {:?}",
                                        peer, error
                                    );
                                }
                            },
                            autonat::Event::StatusChanged { old, new } => {
                                debug!("AutoNAT Status changed. Old: {:?}, New: {:?}", old, new);
                            }
                        };
                        None
                    }
                };

                if let Some(event) = maybe_event {
                    // forward messages directly to Client
                    send_to_client
                        .send(event)
                        .map_err(|err| NetworkError::ChannelSendError(err.to_string()))?;
                }
            }
            SwarmEvent::OutgoingConnectionError {
                connection_id: _,
                peer_id,
                error,
            } => {
                warn!("Outgoing connection error to {:?}: {:?}", peer_id, error);
            }
            SwarmEvent::IncomingConnectionError {
                connection_id: _,
                local_addr: _,
                send_back_addr: _,
                error,
            } => {
                warn!("Incoming connection error: {:?}", error);
            }
            SwarmEvent::ListenerError {
                listener_id: _,
                error,
            } => {
                warn!("Listener error: {:?}", error);
            }
            SwarmEvent::ExternalAddrConfirmed { address } => {
                let my_id = *self.swarm.local_peer_id();
                self.swarm
                    .behaviour_mut()
                    .dht
                    .add_address(&my_id, address.clone());
            }
            SwarmEvent::NewExternalAddrOfPeer { peer_id, address } => {
                self.swarm
                    .behaviour_mut()
                    .dht
                    .add_address(&peer_id, address.clone());
            }
            _ => {
                debug!("Unhandled swarm event {:?}", event);
            }
        }
        Ok(())
    }

    /// Spawn a task to listen for requests on the returned channel
    /// as well as any events produced by libp2p
    ///
    /// # Errors
    /// If the listeners cannot be spawned
    pub fn spawn_listeners(
        mut self,
    ) -> Result<
        (
            UnboundedSender<ClientRequest>,
            UnboundedReceiver<NetworkEvent>,
        ),
        NetworkError,
    > {
        let (s_input, mut s_output) = unbounded_channel::<ClientRequest>();
        let (r_input, r_output) = unbounded_channel::<NetworkEvent>();
        let (mut bootstrap_tx, bootstrap_rx) = mpsc::channel(100);
        self.resend_tx = Some(s_input.clone());
        self.dht_handler.set_bootstrap_sender(bootstrap_tx.clone());

        DHTBootstrapTask::run(bootstrap_rx, s_input.clone());
        spawn(
            async move {
                loop {
                    select! {
                        event = self.swarm.next() => {
                            if let Some(event) = event {
                                trace!("Libp2p handling event {:?}", event);
                                self.handle_swarm_events(event, &r_input).await?;
                            }
                        },
                        msg = s_output.recv() => {
                            trace!("Libp2p handling client request {:?}", msg);
                            let shutdown = self.handle_client_requests(msg).await?;
                            if shutdown {
                                let _ = bootstrap_tx.send(InputEvent::ShutdownBootstrap).await;
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
}
