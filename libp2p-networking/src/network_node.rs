use crate::direct_message::DirectMessageProtocol;
use crate::direct_message::{DirectMessageCodec, DirectMessageResponse};
use crate::network::NetworkDef;
use async_std::task::{sleep, spawn};
use bincode::Options;
use flume::{unbounded, Receiver, Sender};
use futures::{select, FutureExt, StreamExt};
use libp2p::swarm::{ConnectionHandlerUpgrErr, DialError};
use libp2p::{
    build_multiaddr,
    core::{either::EitherError, muxing::StreamMuxerBox, transport::Boxed, upgrade},
    dns,
    gossipsub::{
        error::{GossipsubHandlerError, PublishError},
        Gossipsub, GossipsubConfigBuilder, GossipsubMessage, IdentTopic as Topic,
        MessageAuthenticity, MessageId, ValidationMode,
    },
    identify::{Identify, IdentifyConfig},
    identity::Keypair,
    kad::{self, store::MemoryStore, Kademlia, KademliaConfig},
    mplex, noise,
    request_response::{ProtocolSupport, RequestResponse, RequestResponseConfig, ResponseChannel},
    swarm::SwarmEvent,
    tcp, websocket, yamux, Multiaddr, PeerId, Swarm, Transport, TransportError,
};
use rand::{seq::IteratorRandom, thread_rng};
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};
use std::fmt::Debug;
use std::str::FromStr;
use std::{collections::HashSet, io::Error, iter, time::Duration};
use tracing::{debug, error, info, info_span, instrument, trace, warn, Instrument};

/// metadata about connections
#[derive(Default, Debug, Clone)]
pub struct ConnectionData {
    /// set of currently connecting peers
    pub connected_peers: HashSet<PeerId>,
    /// set of peers that were at one point connected
    pub connecting_peers: HashSet<PeerId>,
    /// set of known peers
    pub known_peers: HashSet<PeerId>,
    /// set of peers that are immune to pruning
    pub ignored_peers: HashSet<PeerId>,
}

/// this is mostly to estimate how many network connections
/// a node should allow
#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum NetworkNodeType {
    /// bootstrap node accepts all connections
    Bootstrap,
    /// regular node has a limit to the
    /// number of connections to accept
    Regular,
    /// conductor node is never pruned
    Conductor,
}

impl FromStr for NetworkNodeType {
    type Err = String;

    fn from_str(input: &str) -> Result<NetworkNodeType, Self::Err> {
        match input {
            "Conductor" => Ok(NetworkNodeType::Conductor),
            "Regular" => Ok(NetworkNodeType::Regular),
            "Bootstrap" => Ok(NetworkNodeType::Bootstrap),
            _ => Err(
                "Couldn't parse node type. Must be one of Conductor, Bootstrap, Regular"
                    .to_string(),
            ),
        }
    }
}

/// serialize an arbitrary message
/// # Errors
/// when unable to serialize a message
pub fn serialize_msg<T: Serialize>(msg: &T) -> Result<Vec<u8>, Box<bincode::ErrorKind>> {
    let bincode_options = bincode::DefaultOptions::new().with_limit(16_384);
    bincode_options.serialize(&msg)
}

/// deserialize an arbitrary message
/// # Errors
/// when unable to deserialize a message
pub fn deserialize_msg<'a, T: Deserialize<'a>>(
    msg: &'a [u8],
) -> Result<T, Box<bincode::ErrorKind>> {
    let bincode_options = bincode::DefaultOptions::new().with_limit(16_384);
    bincode_options.deserialize(msg)
}

impl Default for NetworkNodeType {
    fn default() -> Self {
        Self::Bootstrap
    }
}

/// Network definition
#[derive(custom_debug::Debug)]
pub struct NetworkNode {
    /// pub/private key from with peer_id is derived
    pub identity: Keypair,
    /// peer id of network node
    pub peer_id: PeerId,
    /// the swarm of networkbehaviours
    #[debug(skip)]
    pub swarm: Swarm<NetworkDef>,
    /// the configuration parameters of the netework
    pub config: NetworkNodeConfig,
}

/// describe the configuration of the network
#[derive(Clone, Default, derive_builder::Builder, custom_debug::Debug)]
pub struct NetworkNodeConfig {
    /// max number of connections a node may have before it begins
    /// to disconnect. Only applies if `node_type` is `Regular`
    pub max_num_peers: usize,
    /// Min number of connections a node may have before it begins
    /// to connect to more. Only applies if `node_type` is `Regular`
    pub min_num_peers: usize,
    /// The type of node:
    /// Either bootstrap (greedily connect to all peers)
    /// or regular (respect `min_num_peers`/`max num peers`)
    #[builder(default)]
    pub node_type: NetworkNodeType,
    /// optional identity
    #[builder(setter(into, strip_option), default)]
    #[debug(skip)]
    pub identity: Option<Keypair>,
    /// nodes to ignore
    #[builder(default)]
    #[debug(skip)]
    pub ignored_peers: HashSet<PeerId>,
    /// address to bind to
    #[builder(setter(into, strip_option), default)]
    pub bound_addr: Option<Multiaddr>,
}

/// Actions to send from the client to the swarm
#[derive(Debug)]
pub enum ClientRequest {
    /// kill the swarm
    Shutdown,
    /// broadcast a serialized message
    GossipMsg(Topic, Vec<u8>),
    /// send the peer id
    GetId(Sender<PeerId>),
    /// subscribe to a topic
    Subscribe(String),
    /// unsubscribe from a topic
    Unsubscribe(String),
    /// client request to send a direct message a serialized message
    DirectRequest(PeerId, Vec<u8>),
    /// client request to send a direct reply to a message
    DirectResponse(ResponseChannel<DirectMessageResponse>, Vec<u8>),
    /// disable or enable pruning of connections
    Pruning(bool),
    /// add vec of known peers or addresses
    AddKnownPeers(Vec<(Option<PeerId>, Multiaddr)>),
    /// Ignore peers. Only here for debugging purposes.
    /// Allows us to have nodes that are never pruned
    IgnorePeers(Vec<PeerId>),
}

/// events generated by the swarm that we wish
/// to relay to the client
#[derive(Debug)]
pub enum NetworkEvent {
    /// connected to a new peer
    UpdateConnectedPeers(HashSet<PeerId>),
    /// discovered a new peer
    UpdateKnownPeers(HashSet<PeerId>),
    /// recv-ed a broadcast
    GossipMsg(Vec<u8>),
    /// recv-ed a direct message from a node
    DirectRequest(Vec<u8>, PeerId, ResponseChannel<DirectMessageResponse>),
    /// recv-ed a direct response from a node (that hopefully was initiated by this node)
    DirectResponse(Vec<u8>, PeerId),
}

/// bind all interfaces on port `port`
/// TODO something more general
pub fn gen_multiaddr(port: u16) -> Multiaddr {
    build_multiaddr!(Ip4([0, 0, 0, 0]), Tcp(port))
}

/// Generate authenticated transport, copied from `development_transport`
/// <http://noiseprotocol.org/noise.html#payload-security-properties> for definition of XX
/// # Errors
/// could not sign the noise key with `identity`
#[instrument(skip(identity))]
pub async fn gen_transport(
    identity: Keypair,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>, NetworkError> {
    let transport = {
        let tcp = tcp::TcpConfig::new().nodelay(true);
        let dns_tcp = dns::DnsConfig::system(tcp)
            .await
            .context(TransportLaunchSnafu)?;
        let ws_dns_tcp = websocket::WsConfig::new(dns_tcp.clone());
        dns_tcp.or_transport(ws_dns_tcp)
    };

    // keys for signing messages
    let noise_keys = noise::Keypair::<noise::X25519Spec>::new()
        .into_authentic(&identity)
        .expect("Signing libp2p-noise static DH keypair failed.");

    Ok(transport
        .upgrade(upgrade::Version::V1)
        // authentication: messages are signed
        .authenticate(noise::NoiseConfig::xx(noise_keys).into_authenticated())
        // muxxing streams
        // useful because only one connection opened
        // https://docs.libp2p.io/concepts/stream-multiplexing/
        .multiplex(upgrade::SelectUpgrade::new(
            yamux::YamuxConfig::default(),
            mplex::MplexConfig::default(),
        ))
        .timeout(std::time::Duration::from_secs(20))
        .boxed())
}

impl NetworkNode {
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
    pub async fn add_known_peers(&mut self, known_peers: &[(Option<PeerId>, Multiaddr)]) {
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
        behaviour.notify_update_known_peers();
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
        let swarm: Swarm<NetworkDef> = {
            // Use the hash of the message's contents as the ID
            // Use blake3 for much paranoia at very high speeds
            let message_id_fn = |message: &GossipsubMessage| {
                let hash = blake3::hash(&message.data);
                MessageId::from(hash.as_bytes().to_vec())
            };
            // Create a custom gossipsub
            // TODO: Extract these defaults into some sort of config
            let gossipsub_config = GossipsubConfigBuilder::default()
                .heartbeat_interval(Duration::from_secs(1))
                // Force all messages to have valid signatures
                .validation_mode(ValidationMode::Strict)
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
                "Spectrum validation gossip 0.1".to_string(),
                identity.public(),
            ));

            // - Build DHT needed for peer discovery
            let mut kconfig = KademliaConfig::default();
            kconfig.set_caching(kad::KademliaCaching::Disabled);
            let kadem = Kademlia::with_config(peer_id, MemoryStore::new(peer_id), kconfig);

            // request response for direct messages
            let request_response = RequestResponse::new(
                DirectMessageCodec(),
                iter::once((DirectMessageProtocol(), ProtocolSupport::Full)),
                RequestResponseConfig::default(),
            );

            let pruning_enabled = config.node_type == NetworkNodeType::Regular;

            let network = NetworkDef::new(
                gossipsub,
                kadem,
                identify,
                request_response,
                pruning_enabled,
                config.ignored_peers.clone(),
            );

            Swarm::new(transport, network, peer_id)
        };

        Ok(Self {
            identity,
            peer_id,
            swarm,
            config,
        })
    }

    /// peer discovery mechanism
    /// looks up a random peer
    #[instrument(skip(self))]
    fn handle_peer_discovery(&mut self) {
        if self.swarm.behaviour().is_bootstrapped() {
            let random_peer = PeerId::random();
            self.swarm.behaviour_mut().query_closest_peers(random_peer);
        }
    }

    /// Keep the number of open connections between threshold specified by
    /// the swarm
    #[instrument(skip(self))]
    fn handle_num_connections(&mut self) {
        let used_peers = self.swarm.behaviour().get_peers();

        // otherwise periodically get more peers if needed
        if used_peers.len() <= self.config.min_num_peers {
            // Calcuate the list of "new" peers, once not currently used for
            // a connection
            let potential_peers: HashSet<PeerId> = self
                .swarm
                .behaviour()
                .known_peers()
                .difference(&used_peers)
                .copied()
                .collect();
            // Number of peers we want to try connecting to
            let num_to_connect = self.config.min_num_peers + 1 - used_peers.len();
            // Random(?) subset of the availible peers to try connecting to
            let mut chosen_peers = potential_peers
                .iter()
                .copied()
                .choose_multiple(&mut thread_rng(), num_to_connect)
                .into_iter()
                .collect::<HashSet<_>>();
            chosen_peers.remove(&self.peer_id);
            // Try dialing each random peer
            for a_peer in &chosen_peers {
                if *a_peer != self.peer_id {
                    match self.swarm.dial(*a_peer) {
                        Ok(_) => {
                            info!("Peer {:?} dial {:?} working!", self.peer_id, a_peer);
                            self.swarm.behaviour_mut().add_connecting_peer(*a_peer);
                        }
                        Err(e) => {
                            warn!("Peer {:?} dial {:?} failed: {:?}", self.peer_id, a_peer, e);
                        }
                    };
                }
            }

            // if we don't know any peers, start dialing random peers if we have any.
            if chosen_peers.is_empty() {
                let chosen_addrs = self
                    .swarm
                    .behaviour()
                    .iter_unknown_addressess()
                    .choose_multiple(&mut thread_rng(), num_to_connect);
                for a_addr in chosen_addrs {
                    match self.swarm.dial(a_addr.clone()) {
                        Ok(_) => {
                            info!("Peer {:?} dial {:?} working!", self.peer_id, a_addr);
                        }
                        Err(e) => {
                            info!("Peer {:?} dial {:?} failed: {:?}", self.peer_id, a_addr, e);
                        }
                    }
                }
            }
        }
    }

    #[instrument(skip(self))]
    fn prune_num_connections(&mut self) {
        let swarm = self.swarm.behaviour_mut();
        // If we are connected to too many peers, try disconnecting from
        // a random subset. Bootstrap nodes accept all connections and
        // attempt to connect to all
        if self.config.node_type == NetworkNodeType::Regular {
            for peer_id in swarm.get_peers_to_prune(self.config.max_num_peers) {
                let _ = self.swarm.disconnect_peer_id(peer_id);
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
                    IgnorePeers(peers) => {
                        behaviour.extend_ignored_peers(peers);
                    }
                    Shutdown => {
                        warn!("Libp2p listener shutting down");
                        return Ok(true);
                    }
                    GossipMsg(topic, contents) => {
                        behaviour.publish_gossip(topic, contents);
                    }
                    GetId(reply_chan) => {
                        // FIXME proper error handling
                        reply_chan
                            .send_async(self.peer_id)
                            .await
                            .map_err(|_e| NetworkError::StreamClosed)?;
                    }
                    Subscribe(t) => {
                        behaviour.subscribe_gossip(&t);
                    }
                    Unsubscribe(t) => {
                        behaviour.unsubscribe_gossip(&t);
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
                        self.add_known_peers(&peers).await;
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
                EitherError<EitherError<GossipsubHandlerError, Error>, Error>,
                ConnectionHandlerUpgrErr<Error>,
            >,
        >,
        send_to_client: &Sender<NetworkEvent>,
    ) -> Result<(), NetworkError> {
        // Make the match cleaner
        #[allow(clippy::enum_glob_use)]
        use SwarmEvent::*;
        warn!("event observed {:?}", event);
        let behaviour = self.swarm.behaviour_mut();

        match event {
            ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                behaviour.add_address(&peer_id, endpoint.get_remote_address().clone());
                behaviour.add_connected_peer(peer_id);

                // now we have at least one peer so we can bootstrap
                if behaviour.should_bootstrap() {
                    behaviour
                        .bootstrap()
                        .map_err(|_e| NetworkError::NoKnownPeers)?;
                }
                let connected_peers = behaviour.connected_peers();
                send_to_client
                    .send_async(NetworkEvent::UpdateConnectedPeers(connected_peers))
                    .await
                    .map_err(|_e| NetworkError::StreamClosed)?;
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

                let connected_peers = behaviour.connected_peers();
                send_to_client
                    .send_async(NetworkEvent::UpdateConnectedPeers(connected_peers))
                    .await
                    .map_err(|_e| NetworkError::StreamClosed)?;
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
                error!("connecting error {:?}", error);
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

        spawn(
            async move {
                loop {
                    // TODO variable on futures times
                    select! {
                        _ = sleep(Duration::from_millis(25)).fuse() => {
                            self.handle_sending();
                        },
                        _ = sleep(Duration::from_secs(1)).fuse() => {
                            self.handle_peer_discovery();
                        },
                        _ = sleep(Duration::from_millis(25)).fuse() => {
                            self.handle_num_connections();
                        }
                        _ = sleep(Duration::from_secs(5)).fuse() => {
                            self.prune_num_connections();
                        }
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
}

/// wrapper type for errors generated by the `Network`
#[derive(Debug, Snafu)]
pub enum NetworkError {
    /// Error initiating dial of peer
    DialError {
        /// The underlying source of the error
        source: DialError,
    },
    /// Error during dialing or listening
    Transport {
        /// The underlying source of the error
        source: TransportError<std::io::Error>,
    },
    /// Error establishing backend connection
    TransportLaunch {
        /// The underlying source of the error
        source: std::io::Error,
    },
    /// Error building the gossipsub configuration
    #[snafu(display("Error building the gossipsub configuration: {message}"))]
    GossipsubConfig {
        /// The underlying source of the error
        message: String,
    },
    /// Error building the gossipsub instance
    #[snafu(display("Error building the gossipsub implementation {message}"))]
    GossipsubBuild {
        /// The underlying source of the error
        message: String,
    },
    /// Error if one of the channels to or from the swarm is closed
    /// FIXME ideally include more information
    /// run into lifetime errors when making NetworkError generic over
    /// the type of message.
    StreamClosed,
    /// Error publishing a gossipsub message
    PublishError {
        /// The underlying source of the error
        source: PublishError,
    },
    /// Error when there are no known peers to bootstrap off
    NoKnownPeers,
}
