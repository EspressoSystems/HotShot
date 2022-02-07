use crate::direct_message::{DirectMessageCodec, DirectMessageRequest, DirectMessageResponse};
use async_std::task::{sleep, spawn};
use libp2p::ping::{Ping, PingEvent, PingConfig, Failure};
use rand::{seq::IteratorRandom, thread_rng};
use std::fmt::Debug;
use std::{
    collections::HashSet,
    io::Error,
    iter,
    task::{Context, Poll},
    time::Duration,
};

use flume::{unbounded, Receiver, Sender};
use futures::{select, FutureExt, StreamExt};
use libp2p::{
    build_multiaddr,
    core::{
        either::EitherError, muxing::StreamMuxerBox, transport::Boxed, upgrade, ConnectedPoint,
    },
    dns,
    gossipsub::{
        error::{GossipsubHandlerError, PublishError},
        Gossipsub, GossipsubConfigBuilder, GossipsubEvent, GossipsubMessage, IdentTopic as Topic,
        MessageAuthenticity, MessageId, ValidationMode,
    },
    identify::{Identify, IdentifyConfig, IdentifyEvent},
    identity::Keypair,
    kad::{
        self, store::MemoryStore, GetClosestPeersOk, Kademlia, KademliaConfig, KademliaEvent,
        QueryResult,
    },
    mplex, noise,
    request_response::{
        ProtocolSupport, RequestResponse, RequestResponseConfig, RequestResponseEvent,
        RequestResponseMessage, ResponseChannel,
    },
    swarm::{
        NetworkBehaviour, NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters,
        ProtocolsHandlerUpgrErr, SwarmEvent,
    },
    tcp, websocket, yamux, Multiaddr, NetworkBehaviour, PeerId, Swarm, Transport, TransportError,
};
use snafu::{ResultExt, Snafu};
use tracing::{debug, error, info, info_span, instrument, trace, warn, Instrument};

use crate::direct_message::DirectMessageProtocol;

/// Overarching network behaviour performing:
/// - network topology discovoery
/// - direct messaging
/// - p2p broadcast
/// - connection management
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "NetworkEvent", poll_method = "poll", event_process = true)]
pub struct NetworkDef {
    /// purpose: broadcasting messages to many peers
    /// NOTE gossipsub works ONLY for sharing messsages right now
    /// in the future it may be able to do peer discovery and routing
    /// <`https://github.com/libp2p/rust-libp2p/issues/2398>
    pub gossipsub: Gossipsub,
    /// purpose: peer routing
    pub kadem: Kademlia<MemoryStore>,
    /// purpose: peer discovery
    pub identify: Identify,
    /// purpose: directly messaging peer
    pub request_response: RequestResponse<DirectMessageCodec>,
    /// if the node has been bootstrapped into the kademlia network
    #[behaviour(ignore)]
    pub bootstrap_in_progress: bool,
    // TODO separate out into ConnectionData struct
    /// set of connected peers
    #[behaviour(ignore)]
    pub connected_peers: HashSet<PeerId>,
    // TODO replace this with a set of queryids
    /// set of currently connecting peers
    #[behaviour(ignore)]
    pub connecting_peers: HashSet<PeerId>,
    /// set of peers that were at one point connected
    #[behaviour(ignore)]
    pub known_peers: HashSet<PeerId>,
    /// set of events to send to UI
    #[behaviour(ignore)]
    pub client_event_queue: Vec<NetworkEvent>,
    /// whether or not to prune nodes
    #[behaviour(ignore)]
    pub pruning_enabled: bool,
    /// ping event. Keep the connection alive!
    pub ping: Ping,
}

impl Debug for NetworkDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetworkDef")
            .field("bootstrap", &self.bootstrap_in_progress)
            .field("connected_peers", &self.connected_peers)
            .field("connecting_peers", &self.connecting_peers)
            .field("known_peers", &self.known_peers)
            .finish()
    }
}

/// metadata about connections
#[derive(Default, Debug, Clone)]
pub struct ConnectionData {
    /// set of currently connecting peers
    pub connected_peers: HashSet<PeerId>,
    /// set of peers that were at one point connected
    pub connecting_peers: HashSet<PeerId>,
    /// set of events to send to client
    pub known_peers: HashSet<PeerId>,
}

impl NetworkDef {
    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<NetworkEvent, <Self as NetworkBehaviour>::ProtocolsHandler>>
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

impl NetworkBehaviourEventProcess<PingEvent> for NetworkDef {
    fn inject_event(&mut self, event: PingEvent) {
        info!(?event, "ping event recv-ed");
    }
}

impl NetworkBehaviourEventProcess<GossipsubEvent> for NetworkDef {
    fn inject_event(&mut self, event: GossipsubEvent) {
        info!(?event, "gossipsub msg recv-ed");
        if let GossipsubEvent::Message { message, .. } = event {
            self.client_event_queue
                .push(NetworkEvent::GossipMsg(message.data));
        }
    }
}

impl NetworkBehaviourEventProcess<KademliaEvent> for NetworkDef {
    fn inject_event(&mut self, event: KademliaEvent) {
        info!(?event, "kadem msg recv-ed");
        match event {
            KademliaEvent::OutboundQueryCompleted { result, .. } => {
                match result {
                    // FIXME rebootstrap or fail in the failed
                    // bootstrap case
                    QueryResult::Bootstrap(r) => {
                        match r {
                            Ok(_bootstrap) => {
                                // we're bootstrapped
                                // don't bootstrap again
                                self.bootstrap_in_progress = false;
                            }
                            Err(_) => {
                                // try again
                                self.bootstrap_in_progress = true;
                            }
                        }
                    }
                    QueryResult::GetClosestPeers(Ok(GetClosestPeersOk { peers, .. })) => {
                        for peer in peers {
                            self.known_peers.insert(peer);
                        }
                        self.client_event_queue
                            .push(NetworkEvent::UpdateKnownPeers(self.known_peers.clone()));
                    }
                    _ => {}
                }
            }
            KademliaEvent::RoutingUpdated { peer, addresses, .. } => {
                for address in addresses.iter() {
                    self.request_response.add_address(&peer, address.clone());
                }
                // TODO try add_address to request_response. It looks like stale addresses are
                // happening...
                // NOTE the other reason this is failing might be because the requester is
                // terminating requests
                self.known_peers.insert(peer);
                self.client_event_queue
                    .push(NetworkEvent::UpdateKnownPeers(self.known_peers.clone()));
            }
            _ => {}
        }
    }
}

impl NetworkBehaviourEventProcess<IdentifyEvent> for NetworkDef {
    fn inject_event(&mut self, event: IdentifyEvent) {
        if let IdentifyEvent::Received { peer_id, info, .. } = event {
            for addr in info.listen_addrs {
                self.kadem.add_address(&peer_id, addr.clone());
                self.request_response.add_address(&peer_id, addr.clone());
            }
            self.known_peers.insert(peer_id);
            self.client_event_queue
                .push(NetworkEvent::UpdateKnownPeers(self.known_peers.clone()));
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
        if let RequestResponseEvent::Message { message, .. } = event {
            match message {
                RequestResponseMessage::Request {
                    request: DirectMessageRequest(msg),
                    channel,
                    ..
                } => {
                    self.client_event_queue
                        .push(NetworkEvent::DirectRequest(msg, channel));
                }
                RequestResponseMessage::Response {
                    response: DirectMessageResponse(msg),
                    ..
                } => {
                    self.client_event_queue
                        .push(NetworkEvent::DirectResponse(msg));
                }
            }
        }
    }
}

/// this is mostly to estimate how many network connections
/// a node should allow
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum NetworkNodeType {
    /// bootstrap node accepts all connections
    Bootstrap,
    /// regular node has a limit to the
    /// number of connections to accept
    Regular,
}

impl Default for NetworkNodeType {
    fn default() -> Self {
        Self::Bootstrap
    }
}

// FIXME split this out into network config + swarm

/// Network definition
pub struct NetworkNode {
    /// pub/private key from with peer_id is derived
    pub identity: Keypair,
    /// peer id of network node
    pub peer_id: PeerId,
    /// the swarm of networkbehaviours
    pub swarm: Swarm<NetworkDef>,
    /// the configuration parameters of the netework
    pub config: NetworkNodeConfig,
}

/// describe the configuration of the network
#[derive(Debug, Clone, Copy, Default, derive_builder::Builder)]
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
    pub node_type: NetworkNodeType,
}

impl Debug for NetworkNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Network")
            .field("Peer id", &self.peer_id)
            .field("Swarm", self.swarm.behaviour())
            .field("Network Config", &self.config)
            .finish()
    }
}

/// Actions to send from the client to the swarm
#[derive(Debug)]
pub enum ClientRequest {
    /// kill the swarm
    Shutdown,
    /// broadcast a serialized message
    GossipMsg(Topic, Vec<u8>, Sender<Result<(), NetworkError>>),
    /// send the peer id
    GetId(Sender<PeerId>),
    /// subscribe to a topic
    Subscribe(String),
    /// unsubscribe from a topic
    Unsubscribe(String),
    /// direct message a serialized message
    DirectRequest(PeerId, Vec<u8>),
    /// direct reply to a message
    DirectResponse(ResponseChannel<DirectMessageResponse>, Vec<u8>),
    /// disable or enable pruning of connections
    Pruning(bool),
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
    DirectRequest(Vec<u8>, ResponseChannel<DirectMessageResponse>),
    /// recv-ed a direct response from a node
    DirectResponse(Vec<u8>),
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
        .multiplex(
            upgrade::SelectUpgrade::new(
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
    pub async fn start(
        &mut self,
        listen_addr: Multiaddr,
        known_peers: &[(PeerId, Multiaddr)],
    ) -> Result<Multiaddr, NetworkError> {
        self.swarm.listen_on(listen_addr).context(TransportSnafu)?;
        let addr = loop {
            if let Some(SwarmEvent::NewListenAddr { address, .. }) = self.swarm.next().await {
                break address;
            }
        };
        info!("peerid {:?} listen addr: {:?}", self.peer_id, addr);
        for (peer_id, addr) in known_peers {
            self.swarm
                .behaviour_mut()
                .kadem
                .add_address(&peer_id, addr.clone());
            self.swarm
                .behaviour_mut()
                .request_response
                .add_address(&peer_id, addr.clone());
            // match self.swarm.dial(addr.clone()) {
            //     Ok(_) => {
            //         warn!("peerid {:?} dialed {:?}", self.peer_id, peer_id);
            //     }
            //     Err(e) => error!(
            //         "peerid {:?} dialed {:?} and failed with error: {:?}",
            //         self.peer_id, peer_id, e
            //     ),
            // };
        }
        Ok(addr)
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
        let identity = Keypair::generate_ed25519();
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
            // Use a jank match because Gossipsubconfigbuilder::build returns a non-static str for
            // some god forsaken reason
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
            //   TODO check into the MemoryStore defaults
            let mut kconfig = KademliaConfig::default();
            kconfig.set_caching(kad::KademliaCaching::Disabled);
            let kadem = Kademlia::with_config(peer_id, MemoryStore::new(peer_id), kconfig);

            // request response for direct messages
            let request_response = RequestResponse::new(
                DirectMessageCodec(),
                iter::once((DirectMessageProtocol(), ProtocolSupport::Full)),
                RequestResponseConfig::default(),
            );

            let ping_config = PingConfig::new().with_keep_alive(true);

            let ping = Ping::new(ping_config);

            let pruning_enabled = config.node_type == NetworkNodeType::Regular;

            let network = NetworkDef {
                gossipsub,
                kadem,
                identify,
                request_response,
                ping,
                connected_peers: HashSet::new(),
                connecting_peers: HashSet::new(),
                known_peers: HashSet::new(),
                client_event_queue: Vec::new(),
                bootstrap_in_progress: false,
                pruning_enabled
            };

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
        if !self.swarm.behaviour().bootstrap_in_progress {
            let random_peer = PeerId::random();
            self.swarm
                .behaviour_mut()
                .kadem
                .get_closest_peers(random_peer);
        }
    }

    /// Keep the number of open connections between threshold specified by
    /// the swarm
    #[instrument(skip(self))]
    fn handle_num_connections(&mut self) {
        let swarm = self.swarm.behaviour();
        // if we're bootstrapped, do nothing
        // otherwise periodically get more peers if needed
        if !swarm.bootstrap_in_progress {
            if swarm.connecting_peers.len() + swarm.connected_peers.len()
                <= self.config.min_num_peers
            {
                // Calcuate the currently connected peers
                let used_peers = swarm
                    .connecting_peers
                    .union(&swarm.connected_peers)
                    .copied()
                    .collect();
                // Calcuate the list of "new" peers, once not currently used for
                // a connection
                let potential_peers: HashSet<PeerId> =
                    swarm.known_peers.difference(&used_peers).copied().collect();
                // Number of peers we want to try connecting to
                let num_to_connect = self.config.min_num_peers + 1
                    - (swarm.connected_peers.len() + swarm.connecting_peers.len());
                // Random(?) subset of the availible peers to try connecting to
                let chosen_peers = potential_peers
                    .iter()
                    .copied()
                    .choose_multiple(&mut thread_rng(), num_to_connect);
                // Try dialing each random (?) peer
                for a_peer in chosen_peers {
                    match self.swarm.dial(a_peer) {
                        Ok(_) => {
                            self.swarm.behaviour_mut().connecting_peers.insert(a_peer);
                        }
                        Err(e) => {
                            info!("Peer {:?} dial {:?} failed: {:?}", self.peer_id, a_peer, e);
                        }
                    };
                }
            }
            else if (swarm.pruning_enabled || self.config.node_type == NetworkNodeType::Bootstrap) && swarm.connected_peers.len() > self.config.max_num_peers
            {
                // If we are connected to too many peers, try disconnecting from
                // a random (?) subset
                let peers_to_rm = swarm.connected_peers.iter().copied().choose_multiple( &mut thread_rng(),
                    swarm.connected_peers.len() - self.config.max_num_peers,
                );
                for a_peer in peers_to_rm {
                    // FIXME the error is () ?
                    let _ = self.swarm.disconnect_peer_id(a_peer);
                }
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
        match msg {
            Ok(msg) => {
                #[allow(clippy::enum_glob_use)]
                use ClientRequest::*;
                match msg {
                    Shutdown => {
                        warn!("Libp2p listener shutting down");
                        return Ok(true);
                    }
                    ClientRequest::GossipMsg(topic, contents, chan) => {
                        let res = self
                            .swarm
                            .behaviour_mut()
                            .gossipsub
                            .publish(topic, contents.clone())
                            .map(|_| ())
                            .context(PublishSnafu);
                        info!("publishing reuslt! {:?}", res);

                        // send result back to ui to confirm this isn't a duplicate message
                        chan.send_async(res)
                            .await
                            .map_err(|_e| NetworkError::StreamClosed)?;
                    }
                    GetId(reply_chan) => {
                        // FIXME proper error handling
                        reply_chan
                            .send_async(self.peer_id)
                            .await
                            .map_err(|_e| NetworkError::StreamClosed)?;
                    }
                    Subscribe(t) => {
                        if self
                            .swarm
                            .behaviour_mut()
                            .gossipsub
                            .subscribe(&Topic::new(t.clone()))
                            .is_err()
                        {
                            error!("error subscribing to topic {}", t);
                        }
                    }
                    Unsubscribe(t) => {
                        if self
                            .swarm
                            .behaviour_mut()
                            .gossipsub
                            .unsubscribe(&Topic::new(t.clone()))
                            .is_err()
                        {
                            error!("error unsubscribing to topic {}", t);
                        }
                    }
                    DirectRequest(pid, msg) => {
                        self.swarm
                            .behaviour_mut()
                            .request_response
                            .send_request(&pid, DirectMessageRequest(msg));
                    }
                    DirectResponse(chan, msg) => {
                        let res = self
                            .swarm
                            .behaviour_mut()
                            .request_response
                            .send_response(chan, DirectMessageResponse(msg));
                        if let Err(e) = res {
                            error!("Error replying to direct message. {:?}", e);
                        }
                    }
                    Pruning(is_enabled) => {
                        self.swarm.behaviour_mut().pruning_enabled = is_enabled;
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
            EitherError<EitherError<
                EitherError<EitherError<GossipsubHandlerError, Error>, Error>, 
                ProtocolsHandlerUpgrErr<Error>,
            >,
                Failure>,
        >,
        send_to_client: &Sender<NetworkEvent>,
    ) -> Result<(), NetworkError> {
        // Make the match cleaner
        #[allow(clippy::enum_glob_use)]
        use SwarmEvent::*;

        warn!("libp2p event {:?}", event);
        match event {
            ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                match endpoint {
                    ConnectedPoint::Dialer { address } => {
                        self.swarm
                            .behaviour_mut()
                            .kadem
                            .add_address(&peer_id, address.clone());
                        self.swarm
                            .behaviour_mut()
                            .request_response
                            .add_address(&peer_id, address);
                    }
                    ConnectedPoint::Listener {
                        local_addr: _,
                        send_back_addr,
                    } => {
                        self.swarm
                            .behaviour_mut()
                            .kadem
                            .add_address(&peer_id, send_back_addr.clone());
                        self.swarm
                            .behaviour_mut()
                            .request_response
                            .add_address(&peer_id, send_back_addr);
                    }
                }
                self.swarm.behaviour_mut().connected_peers.insert(peer_id);
                self.swarm.behaviour_mut().connecting_peers.remove(&peer_id);
                // now we have at least one peer so we can bootstrap
                if !self.swarm.behaviour().bootstrap_in_progress {
                    self.swarm
                        .behaviour_mut()
                        .kadem
                        .bootstrap()
                        .map_err(|_e| NetworkError::NoKnownPeers)?;
                }
                send_to_client
                    .send_async(NetworkEvent::UpdateConnectedPeers(
                        self.swarm.behaviour_mut().connected_peers.clone(),
                    ))
                    .await
                    .map_err(|_e| NetworkError::StreamClosed)?;
            }
            ConnectionClosed { peer_id, endpoint, .. } => {
                let swarm = self.swarm.behaviour_mut();
                swarm.connected_peers.remove(&peer_id);
                // FIXME remove stale address, not *all* addresses
                swarm.kadem.remove_peer(&peer_id);
                // swarm.kadem.remove_address();
                // swarm.request_response.remove_address(peer, address)

                send_to_client
                    .send_async(NetworkEvent::UpdateConnectedPeers(
                        swarm.connected_peers.clone(),
                    ))
                    .await
                    .map_err(|_e| NetworkError::StreamClosed)?;
                send_to_client
                    .send_async(NetworkEvent::UpdateKnownPeers(swarm.known_peers.clone()))
                    .await
                    .map_err(|_e| NetworkError::StreamClosed)?;
            }
            Dialing(_)
            | NewListenAddr { .. }
            | ExpiredListenAddr { .. }
            | ListenerClosed { .. }
            | IncomingConnection { .. }
            | IncomingConnectionError { .. }
            | OutgoingConnectionError { .. }
            | BannedPeer { .. }
            | ListenerError { .. } => {}
            Behaviour(b) => {
                // forward messages directly to Client
                send_to_client
                    .send_async(b)
                    .await
                    .map_err(|_e| NetworkError::StreamClosed)?;
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

        spawn(
            async move {
                loop {
                    // TODO variable on futures times
                    select! {
                        _ = sleep(Duration::from_secs(1)).fuse() => {
                            self.handle_peer_discovery();
                        },
                        _ = sleep(Duration::from_secs(1)).fuse() => {
                            self.handle_num_connections();
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
