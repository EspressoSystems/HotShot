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

pub mod tracing_setup;

use async_std::task::{spawn, sleep};
use std::{collections::{HashSet, HashMap}, marker::PhantomData, time::Duration};

use flume::{unbounded, Receiver, Sender};
use futures::{select, FutureExt, StreamExt};
use libp2p::{
    build_multiaddr,
    core::{muxing::StreamMuxerBox, transport::Boxed, ConnectedPoint},
    gossipsub::{
        error::PublishError, Gossipsub, GossipsubConfigBuilder, GossipsubEvent, GossipsubMessage,
        IdentTopic as Topic, MessageAuthenticity, MessageId, ValidationMode,
    },
    identify::{Identify, IdentifyConfig, IdentifyEvent},
    identity::Keypair,
    kad::{store::MemoryStore, Kademlia, KademliaEvent, self, GetClosestPeersOk, KademliaConfig},
    swarm::{NetworkBehaviour, SwarmEvent},
    Multiaddr, NetworkBehaviour, PeerId, Swarm, TransportError,
};
use serde::{de::DeserializeOwned, Serialize};
use snafu::{ResultExt, Snafu};
use tracing::{debug, error, info, info_span, instrument, trace, warn, Instrument};
pub mod ui;

/// event_process is false because
/// injecting events does not play well
/// with asyncrony
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "NetworkEvent")]
#[behaviour(event_process = false)]
pub struct NetworkDef {
    /// NOTE gossipsub works ONLY for sharing messsages right now
    /// in the future it may be able to do peer discovery and routing
    /// at which point we can scrap kadem
    pub gossipsub: Gossipsub,
    /// used for passive peer discovery and peer routing
    pub kadem: Kademlia<MemoryStore>,
    pub identify: Identify,
}

#[derive(Debug)]
pub enum NetworkEvent {
    Gossip(GossipsubEvent),
    Kadem(KademliaEvent),
    Ident(IdentifyEvent),
}

impl From<IdentifyEvent> for NetworkEvent {
    fn from(source: IdentifyEvent) -> Self {
        NetworkEvent::Ident(source)
    }
}

impl From<KademliaEvent> for NetworkEvent {
    fn from(source: KademliaEvent) -> Self {
        NetworkEvent::Kadem(source)
    }
}

impl From<GossipsubEvent> for NetworkEvent {
    fn from(source: GossipsubEvent) -> Self {
        NetworkEvent::Gossip(source)
    }
}

pub struct Network<N, M: NetworkBehaviour> {
    pub connected_peers: HashSet<PeerId>,
    pub connecting_peers: HashSet<PeerId>,
    pub known_peers: HashSet<PeerId>,
    pub identity: Keypair,
    pub peer_id: PeerId,
    pub broadcast_topic: Topic,
    pub swarm: Swarm<M>,
    pub max_num_peers: usize,
    _phantom: PhantomData<N>,
}

/// holds requests to the swarm
pub enum SwarmAction<N: Send> {
    Shutdown,
    GossipMsg(N, Sender<Result<(), NetworkError>>), // topic, message
    GetId(Sender<PeerId>),
    Subscribe(String),
    Unsubscribe(String),
}

/// holds events of the swarm to be relayed to the cli event loop
/// out
pub enum SwarmResult<N: Send> {
    UpdateConnectedPeers(HashSet<PeerId>),
    GossipMsg(N),
}

/// trait to get out the topic and contents of a message
/// such that it may be "gossipped" to other people
pub trait GossipMsg: Send {
    fn topic(&self) -> Topic;
    fn data(&self) -> Vec<u8>;
}

impl<N, M: NetworkBehaviour> std::fmt::Debug for Network<N, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "identity public key: {:?}, peer id: {:?}, topic: {:?}",
            self.identity.public(),
            self.peer_id,
            self.broadcast_topic
        )
    }
}

impl<N: DeserializeOwned + Serialize + std::fmt::Debug, M: NetworkBehaviour> Network<N, M> {
    /// starts the swarm listening on `listen_addr`
    /// and optionally dials into peer `known_peer`
    #[instrument]
    pub fn start(
        &mut self,
        listen_addr: Multiaddr,
        known_peer: Option<Multiaddr>,
    ) -> Result<(), NetworkError> {
        self.swarm.listen_on(listen_addr).context(TransportSnafu)?;
        if let Some(known_peer) = known_peer {
            let dialing = known_peer.clone();
            match self.swarm.dial(known_peer) {
                Ok(_) => {
                    info!("Dialed {:?}", dialing);
                }
                Err(e) => error!("Dial {:?} failed: {:?}", dialing, e),
            };
        }
        Ok(())
    }
}

/// bind all interfaces on port `port`
/// TODO something more general
pub fn gen_multiaddr(port: u16) -> Multiaddr {
    build_multiaddr!(Ip4([0, 0, 0, 0]), Tcp(port))
}

impl<N> Network<N, NetworkDef>
where
    N: DeserializeOwned
        + Serialize
        + From<GossipsubMessage>
        + GossipMsg
        + std::fmt::Debug
        + Send
        + 'static,
{
    /// Creates a new `Network` with the given settings.
    ///
    /// Currently:
    ///   * Generates a random key pair and associated [`PeerId`]
    ///   * Launches a development-only type of transport backend
    ///   * Generates a connection to the "broadcast" topic
    ///   * Creates a swarm to manage peers and events
    #[instrument]
    pub async fn new(_: PhantomData<N>) -> Result<Self, NetworkError> {
        // Generate a random PeerId
        let identity = Keypair::generate_ed25519();
        let peer_id = PeerId::from(identity.public());
        debug!(?peer_id);
        // TODO: Maybe not use a development only networking backend
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
                .mesh_n_low(1)
                .gossip_factor(1.0f64)
                .mesh_outbound_min(0)
                .opportunistic_graft_ticks(1)

                // defaults to true but we want this
                .do_px()
                .build()
                .map_err(|s| GossipsubConfigSnafu { message: s }.build())?;
            // - Build a gossipsub network behavior
            let gossipsub: Gossipsub = Gossipsub::new(
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

            let network = NetworkDef {
                gossipsub,
                kadem,
                identify,
            };

            Swarm::new(transport, network, peer_id)
        };

        Ok(Self {
            identity,
            peer_id,
            connected_peers: HashSet::new(),
            connecting_peers: HashSet::new(),
            known_peers: HashSet::new(),
            broadcast_topic,
            max_num_peers: 5,
            swarm,
            _phantom: PhantomData,
        })
    }

    /// spawn a task to listen for requests on the returned channel
    /// as well as any events produced by libp2p
    /// `mut_mut` is disabled b/c must consume `self`
    /// TODO why does clippy not like `panic` with select?
    #[allow(clippy::mut_mut, clippy::panic)]
    #[instrument]
    pub async fn spawn_listeners(mut self) -> (Sender<SwarmAction<N>>, Receiver<SwarmResult<N>>) {
        let (s_input, s_output) = unbounded::<SwarmAction<N>>();
        let (r_input, r_output) = unbounded::<SwarmResult<N>>();
        // TODO fix this error handling
        let _ = r_input
            .send_async(SwarmResult::UpdateConnectedPeers(
                self.connected_peers.clone(),
            ))
            .await
            .map_err(|_e| NetworkError::StreamClosed);

        let mut bootstrapped = false;
        spawn(async move {
            loop {
                select! {
                    _ = sleep(Duration::from_secs(10)).fuse() => {
                        if !bootstrapped {
                            if self.connecting_peers.len() + self.connected_peers.len() <= self.max_num_peers {
                                let potential_peers : HashSet<PeerId> = self.known_peers.difference(&self.connecting_peers.union(&self.connected_peers).cloned().collect()).cloned().collect();
                                for a_peer in potential_peers {
                                    match self.swarm.dial(a_peer) {
                                        Ok(_) => {
                                            self.connecting_peers.insert(a_peer);

                                        }
                                        Err(e) => error!("Dial {:?} failed: {:?}", a_peer, e),
                                    };
                                }
                            }
                            for a_peer in self.connected_peers.clone() {
                                let _ = self.swarm.behaviour_mut().kadem.get_closest_peers(a_peer);
                            }
                        }
                    }
                    event = self.swarm.select_next_some() => {
                        error!("libp2p event {:?}", event);
                        match event {
                            SwarmEvent::Dialing(_)
                            | SwarmEvent::NewListenAddr {..}
                            | SwarmEvent::ExpiredListenAddr {..}
                            | SwarmEvent::ListenerClosed {..}
                            | SwarmEvent::IncomingConnection {..}
                            | SwarmEvent::IncomingConnectionError {..}
                            | SwarmEvent::OutgoingConnectionError {..}
                            | SwarmEvent::BannedPeer {..}
                            | SwarmEvent::ListenerError {..} => {
                            },
                            SwarmEvent::Behaviour(b) => {
                                match b {
                                    NetworkEvent::Gossip(g) => {
                                        match g {
                                            GossipsubEvent::Message { message, .. } => {
                                                // FIXME proper error handling
                                                r_input.send_async(SwarmResult::GossipMsg(message.into())).await.map_err(|_e| NetworkError::StreamClosed)?;
                                            },
                                            _ => {
                                                info!(?g);
                                            }
                                        }
                                    },
                                    NetworkEvent::Kadem(event) => {
                                        match event {
                                            KademliaEvent::OutboundQueryCompleted { result, ..} => {
                                                match result {
                                                    // FIXME rebootstrap or fail in the failed
                                                    // bootstrap case
                                                    kad::QueryResult::Bootstrap(Ok(_bootstrap)) => {
                                                        // when bootstrap succeeds,
                                                        // get more peers
                                                        // let _ = self.swarm.behaviour_mut().kadem.get_closest_peers(self.peer_id);
                                                        bootstrapped = false;
                                                    },
                                                    kad::QueryResult::GetClosestPeers(result) => {
                                                        match result {
                                                            Ok(GetClosestPeersOk { key: _key, peers }) => {
                                                                for peer in peers {
                                                                    self.known_peers.insert(peer);
                                                                }
                                                            },
                                                            _ => {}
                                                        }
                                                    },
                                                    _ => {}
                                                }
                                            },
                                            KademliaEvent::RoutingUpdated { peer, is_new_peer: _is_new_peer, addresses: _addresses, .. } => {
                                                self.known_peers.insert(peer);
                                            },
                                            _ => {}
                                        }
                                    },
                                    NetworkEvent::Ident(i) => {
                                        // TODO
                                    },

                                }
                            },
                            SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                                match endpoint {
                                    ConnectedPoint::Dialer { address } => {
                                        self.swarm.behaviour_mut().kadem.add_address(&peer_id, address);
                                    },
                                    ConnectedPoint::Listener { local_addr: _, send_back_addr } => {
                                        self.swarm.behaviour_mut().kadem.add_address(&peer_id, send_back_addr);
                                    },
                                }
                                self.connected_peers.insert(peer_id);
                                self.connecting_peers.remove(&peer_id);
                                // now we have at least one peer so we can bootstrap
                                if !bootstrapped {
                                    // TODO error handling
                                    let _ = self.swarm.behaviour_mut().kadem.bootstrap();
                                }
                                r_input.send_async(SwarmResult::UpdateConnectedPeers(self.connected_peers.clone())).await.map_err(|_e| NetworkError::StreamClosed)?;
                            }
                            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                                self.connected_peers.remove(&peer_id);
                                self.known_peers.remove(&peer_id);
                                let _ = self.swarm.disconnect_peer_id(peer_id);
                                // self.swarm.behaviour_mut().kadem.disconnect_peer_id();
                                // if self.connected_peers.len() <= self.max_num_peers {
                                //     // search for more peers
                                //     // TODO error handling
                                //     let _ = self.swarm.behaviour_mut().kadem.get_closest_peers(self.peer_id);
                                // }

                                r_input.send_async(SwarmResult::UpdateConnectedPeers(self.connected_peers.clone())).await.map_err(|_e| NetworkError::StreamClosed)?;
                            }
                        }
                    },
                    msg = s_output.recv_async() => {
                        match msg {
                            Ok(msg) => {
                                match msg {
                                    SwarmAction::Shutdown => {
                                        warn!("Libp2p listener shutting down");
                                        break
                                    },
                                    SwarmAction::GossipMsg(msg, chan) => {
                                        info!("broadcasting message {:?}", msg);
                                        let topic = <N as GossipMsg>::topic(&msg);
                                        let contents = <N as GossipMsg>::data(&msg);
                                        let res = self.swarm
                                            .behaviour_mut().gossipsub.publish(topic.clone(), contents.clone()).map(|_| ()).context(PublishSnafu);
                                        chan.send_async(res).await.map_err(|_e| NetworkError::StreamClosed)?;
                                    },
                                    SwarmAction::GetId(reply_chan) => {
                                        // FIXME proper error handling
                                        reply_chan.send_async(self.peer_id).await.map_err(|_e| NetworkError::StreamClosed)?;
                                    },
                                    SwarmAction::Subscribe(t) => {
                                        match self.swarm.behaviour_mut().gossipsub.subscribe(&Topic::new(t.clone())) {
                                            Ok(_) => (),
                                            Err(_) => {
                                                error!("error subscribing to topic {}", t);
                                            }
                                        }
                                    }
                                    SwarmAction::Unsubscribe(t) => {
                                        match self.swarm.behaviour_mut().gossipsub.unsubscribe(&Topic::new(t.clone())) {
                                            Ok(_) => (),
                                            Err(_) => {
                                                error!("error unsubscribing to topic {}", t);
                                            }
                                        }
                                    },
                                }
                            },
                            Err(e) => {
                                error!("Error receiving msg: {:?}", e);
                            }
                        }
                    }
                }
            }
        Ok::<(), NetworkError>(())
        }.instrument(info_span!( "Libp2p Event Handler")));
        (s_input, r_output)
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
    // FIXME ideally include more information
    // run into lifetime errors when making NetworkError generic over
    // the type of message
    // occurs if one of the channels to or from the swarm is closed
    StreamClosed,
    PublishError {
        source: PublishError,
    },
}
