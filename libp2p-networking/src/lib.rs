#![warn(
    clippy::all,
    clippy::pedantic,
    rust_2018_idioms,
    // missing_docs,
    // clippy::missing_docs_in_private_items,
    clippy::panic
)]
#![allow(
    clippy::option_if_let_else,
    clippy::must_use_candidate,
    clippy::module_name_repetitions,
    clippy::similar_names,
    clippy::unused_self
)]

// FIXME instrument all functions
// FIXME debug impl for Network, NetworkDef

pub mod direct_message;
pub mod tracing_setup;

use async_std::task::{sleep, spawn};
use direct_message::{DirectMessageCodec, DirectMessageRequest, DirectMessageResponse};
use rand::{seq::IteratorRandom, thread_rng};
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

pub mod message;
pub mod ui;

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "SwarmResult", poll_method = "poll", event_process = true)]
pub struct NetworkDef {
    /// purpose: broadcasting messages to many peers
    /// NOTE gossipsub works ONLY for sharing messsages right now
    /// in the future it may be able to do peer discovery and routing
    /// https://github.com/libp2p/rust-libp2p/issues/2398
    pub gossipsub: Gossipsub,
    /// purpose: peer routing
    pub kadem: Kademlia<MemoryStore>,
    /// purpose: peer discovery
    pub identify: Identify,
    /// purpose: directly messaging peer
    pub request_response: RequestResponse<DirectMessageCodec>,
    #[behaviour(ignore)]
    pub bootstrap: bool,
    #[behaviour(ignore)]
    pub connected_peers: HashSet<PeerId>,
    // TODO replace this with a set of queryids
    #[behaviour(ignore)]
    pub connecting_peers: HashSet<PeerId>,
    #[behaviour(ignore)]
    pub known_peers: HashSet<PeerId>,
    #[behaviour(ignore)]
    pub ui_events: Vec<SwarmResult>,
}

impl NetworkDef {
    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<SwarmResult, <Self as NetworkBehaviour>::ProtocolsHandler>>
    {
        // push events that must be relayed back to UI onto queue
        // to be consumed by UI event handler
        if !self.ui_events.is_empty() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                self.ui_events.remove(0),
            ));
        }

        Poll::Pending
    }
}

impl NetworkBehaviourEventProcess<GossipsubEvent> for NetworkDef {
    fn inject_event(&mut self, event: GossipsubEvent) {
        error!(?event, "gossipsub msg recv-ed");
        if let GossipsubEvent::Message { message, .. } = event {
            error!("correct message form");
            self.ui_events.push(SwarmResult::GossipMsg(message.data));
        }
    }
}

impl NetworkBehaviourEventProcess<KademliaEvent> for NetworkDef {
    fn inject_event(&mut self, event: KademliaEvent) {
        error!(?event, "kadem msg recv-ed");
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
                                self.bootstrap = false;
                            }
                            Err(_) => {
                                // try again
                                self.bootstrap = true;
                            }
                        }
                    }
                    QueryResult::GetClosestPeers(Ok(GetClosestPeersOk { peers, .. })) => {
                        for peer in peers {
                            self.known_peers.insert(peer);
                        }
                        self.ui_events
                            .push(SwarmResult::UpdateKnownPeers(self.known_peers.clone()));
                    }
                    _ => {}
                }
            }
            KademliaEvent::RoutingUpdated {
                peer,
                is_new_peer: _is_new_peer,
                addresses: _addresses,
                ..
            } => {
                self.known_peers.insert(peer);
                self.ui_events
                    .push(SwarmResult::UpdateKnownPeers(self.known_peers.clone()));
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
            }
            self.known_peers.insert(peer_id);
            self.ui_events
                .push(SwarmResult::UpdateKnownPeers(self.known_peers.clone()));
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
                    self.ui_events
                        .push(SwarmResult::DirectRequest(msg, channel));
                }
                RequestResponseMessage::Response {
                    response: DirectMessageResponse(msg),
                    ..
                } => {
                    self.ui_events.push(SwarmResult::DirectResponse(msg));
                }
            }
        }
    }
}

pub struct Network {
    pub identity: Keypair,
    pub peer_id: PeerId,
    pub broadcast_topic: Topic,
    pub swarm: Swarm<NetworkDef>,
    pub max_num_peers: usize,
    pub min_num_peers: usize,
}

pub enum SwarmAction {
    Shutdown,
    GossipMsg(Topic, Vec<u8>, Sender<Result<(), NetworkError>>),
    GetId(Sender<PeerId>),
    Subscribe(String),
    Unsubscribe(String),
    DirectRequest(PeerId, Vec<u8>),
    DirectResponse(ResponseChannel<DirectMessageResponse>, Vec<u8>),
}

/// events generated by the swarm that we wish
/// to relay to UI
#[derive(Debug)]
pub enum SwarmResult {
    UpdateConnectedPeers(HashSet<PeerId>),
    UpdateKnownPeers(HashSet<PeerId>),
    GossipMsg(Vec<u8>),
    DirectRequest(Vec<u8>, ResponseChannel<DirectMessageResponse>),
    DirectResponse(Vec<u8>),
}

/// bind all interfaces on port `port`
/// TODO something more general
pub fn gen_multiaddr(port: u16) -> Multiaddr {
    build_multiaddr!(Ip4([0, 0, 0, 0]), Tcp(port))
}

/// generate authenticated transport, copied from `development_transport`
/// <http://noiseprotocol.org/noise.html#payload-security-properties> for definition of XX
/// # Errors
/// could not sign the noise key with `identity`
pub async fn gen_transport(identity: Keypair) -> std::io::Result<Boxed<(PeerId, StreamMuxerBox)>> {
    let transport = {
        let tcp = tcp::TcpConfig::new().nodelay(true);
        let dns_tcp = dns::DnsConfig::system(tcp).await?;
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

impl Network {
    /// starts the swarm listening on `listen_addr`
    /// and optionally dials into peer `known_peer`
    /// returns the address the swarm is listening upon
    #[instrument(skip(self))]
    pub async fn start(
        &mut self,
        listen_addr: Multiaddr,
        known_peer: Option<Multiaddr>,
    ) -> Result<Multiaddr, NetworkError> {
        let listen_id = self.swarm.listen_on(listen_addr).context(TransportSnafu)?;
        let addr = loop {
            match self.swarm.next().await {
                Some(SwarmEvent::NewListenAddr { address, .. }) => break address,
                _ => continue,
            };
        };
        error!("listen addr: {:?}", listen_id);
        if let Some(known_peer) = known_peer {
            let dialing = known_peer.clone();
            match self.swarm.dial(known_peer) {
                Ok(_) => {
                    info!("Dialed {:?}", dialing);
                }
                Err(e) => error!("Dial {:?} failed: {:?}", dialing, e),
            };
        }
        Ok(addr)
    }

    /// Creates a new `Network` with the given settings.
    ///
    /// Currently:
    ///   * Generates a random key pair and associated [`PeerId`]
    ///   * Launches a development-only type of transport backend
    ///   * Generates a connection to the "broadcast" topic
    ///   * Creates a swarm to manage peers and events
    #[instrument]
    pub async fn new() -> Result<Self, NetworkError> {
        // Generate a random PeerId
        let identity = Keypair::generate_ed25519();
        let peer_id = PeerId::from(identity.public());
        debug!(?peer_id);
        let transport: Boxed<(PeerId, StreamMuxerBox)> =
            libp2p::development_transport(identity.clone())
                .await
                .context(TransportLaunchSnafu)?;
        trace!("Launched network transport");
        let broadcast_topic = Topic::new("broadcast");
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

            let network = NetworkDef {
                gossipsub,
                kadem,
                identify,
                request_response,
                connected_peers: HashSet::new(),
                connecting_peers: HashSet::new(),
                known_peers: HashSet::new(),
                ui_events: Vec::new(),
                bootstrap: false,
            };

            Swarm::new(transport, network, peer_id)
        };

        Ok(Self {
            identity,
            peer_id,
            broadcast_topic,
            max_num_peers: 6,
            min_num_peers: 5,
            swarm,
        })
    }

    /// peer discovery mechanism
    /// looks up a random peer
    #[inline]
    fn handle_peer_discovery(&mut self) {
        if !self.swarm.behaviour().bootstrap {
            let random_peer = PeerId::random();
            self.swarm
                .behaviour_mut()
                .kadem
                .get_closest_peers(random_peer);
        }
    }

    /// Keep the number of open connections between threshold specified by
    /// the swarm
    #[inline]
    fn handle_num_connections(&mut self) {
        let swarm = self.swarm.behaviour();
        // if we're bootstrapped, do nothing
        // otherwise periodically get more peers if needed
        if !swarm.bootstrap {
            if swarm.connecting_peers.len() + swarm.connected_peers.len() <= self.min_num_peers {
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
                let num_to_connect = self.min_num_peers + 1
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
                        Err(e) => error!("Dial {:?} failed: {:?}", a_peer, e),
                    };
                }
            } else if swarm.connected_peers.len() > self.max_num_peers {
                // If we are connected to too many peers, try disconnecting from
                // a random (?) subset
                let peers_to_rm = swarm.connected_peers.iter().copied().choose_multiple(
                    &mut thread_rng(),
                    swarm.connected_peers.len() - self.max_num_peers,
                );
                for a_peer in peers_to_rm {
                    // FIXME the error is () ?
                    let _ = self.swarm.disconnect_peer_id(a_peer);
                }
            }
        }
    }

    /// event handler for UI.
    /// currectly supported actions include
    /// - shutting down the swarm
    /// - gossipping a message to known peers on the `global` topic
    /// - returning the id of the current peer
    /// - subscribing to a topic
    /// - unsubscribing from a toipc
    /// - direct messaging a peer
    async fn handle_ui_events(
        &mut self,
        msg: Result<SwarmAction, flume::RecvError>,
    ) -> Result<bool, NetworkError> {
        match msg {
            Ok(msg) => {
                #[allow(clippy::enum_glob_use)]
                use SwarmAction::*;
                match msg {
                    Shutdown => {
                        warn!("Libp2p listener shutting down");
                        return Ok(true);
                    }
                    SwarmAction::GossipMsg(topic, contents, chan) => {
                        let res = self
                            .swarm
                            .behaviour_mut()
                            .gossipsub
                            .publish(topic, contents.clone())
                            .map(|_| ())
                            .context(PublishSnafu);
                        error!("publishing reuslt! {:?}", res);

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
                            error!(?e, "problem with replying");
                        }
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
    async fn handle_swarm_events(
        &mut self,
        event: SwarmEvent<
            SwarmResult,
            EitherError<
                EitherError<EitherError<GossipsubHandlerError, Error>, Error>,
                ProtocolsHandlerUpgrErr<Error>,
            >,
        >,
        send_to_ui: &Sender<SwarmResult>,
    ) -> Result<(), NetworkError> {
        // Make the match cleaner
        #[allow(clippy::enum_glob_use)]
        use SwarmEvent::*;

        info!("libp2p event {:?}", event);
        match event {
            ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                match endpoint {
                    ConnectedPoint::Dialer { address } => {
                        self.swarm
                            .behaviour_mut()
                            .kadem
                            .add_address(&peer_id, address);
                    }
                    ConnectedPoint::Listener {
                        local_addr: _,
                        send_back_addr,
                    } => {
                        self.swarm
                            .behaviour_mut()
                            .kadem
                            .add_address(&peer_id, send_back_addr);
                    }
                }
                self.swarm.behaviour_mut().connected_peers.insert(peer_id);
                self.swarm.behaviour_mut().connecting_peers.remove(&peer_id);
                // now we have at least one peer so we can bootstrap
                if !self.swarm.behaviour().bootstrap {
                    self.swarm
                        .behaviour_mut()
                        .kadem
                        .bootstrap()
                        .map_err(|_e| NetworkError::NoKnownPeers)?;
                }
                send_to_ui
                    .send_async(SwarmResult::UpdateConnectedPeers(
                        self.swarm.behaviour_mut().connected_peers.clone(),
                    ))
                    .await
                    .map_err(|_e| NetworkError::StreamClosed)?;
            }
            ConnectionClosed { peer_id, .. } => {
                let swarm = self.swarm.behaviour_mut();
                swarm.connected_peers.remove(&peer_id);
                // FIXME remove stale address, not *all* addresses
                swarm.kadem.remove_peer(&peer_id);

                send_to_ui
                    .send_async(SwarmResult::UpdateConnectedPeers(
                        swarm.connected_peers.clone(),
                    ))
                    .await
                    .map_err(|_e| NetworkError::StreamClosed)?;
                send_to_ui
                    .send_async(SwarmResult::UpdateKnownPeers(swarm.known_peers.clone()))
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
                // forward messages directly to UI
                send_to_ui
                    .send_async(b)
                    .await
                    .map_err(|_e| NetworkError::StreamClosed)?;
            }
        }
        Ok(())
    }

    /// Spawn a task to listen for requests on the returned channel
    /// as well as any events produced by libp2p
    /// `mut_mut` is disabled b/c must consume `self`
    #[allow(
        clippy::mut_mut,
        clippy::panic,
        clippy::single_match,
        clippy::collapsible_match
    )]
    #[instrument(skip(self))]
    pub async fn spawn_listeners(
        mut self,
    ) -> Result<(Sender<SwarmAction>, Receiver<SwarmResult>), NetworkError> {
        let (s_input, s_output) = unbounded::<SwarmAction>();
        let (r_input, r_output) = unbounded::<SwarmResult>();

        spawn(
            async move {
                loop {
                    select! {
                        _ = sleep(Duration::from_secs(30)).fuse() => {
                            self.handle_peer_discovery();
                        },
                        _ = sleep(Duration::from_secs(1)).fuse() => {
                            self.handle_num_connections();
                        }
                        event = self.swarm.next() => {
                            if let Some(event) = event {
                                error!("handling event {:?}", event);
                                self.handle_swarm_events(event, &r_input).await?;
                            }
                        },
                        msg = s_output.recv_async() => {
                            let shutdown = self.handle_ui_events(msg).await?;
                            if shutdown {
                                break
                            }
                        }
                    }
                }
                Ok::<(), NetworkError>(())
            }
            .instrument(info_span!("Libp2p Event Handler")),
        );
        Ok((s_input, r_output))
    }
}

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
    PublishError { source: PublishError },
    /// Error when there are no known peers to bootstrap off
    NoKnownPeers,
}
