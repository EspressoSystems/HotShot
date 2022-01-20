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

use async_std::task::{sleep, spawn};
use message::{DirectMessageCodec, DirectMessageRequest, DirectMessageResponse};
use rand::{seq::IteratorRandom, thread_rng};
use std::{collections::HashSet, marker::PhantomData, time::Duration, iter};

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
    kad::{
        self, store::MemoryStore, GetClosestPeersOk, Kademlia, KademliaConfig, KademliaEvent,
        QueryResult,
    },
    request_response::{
        RequestResponse, RequestResponseConfig, RequestResponseEvent, RequestResponseMessage, ProtocolSupport,
    },
    swarm::{SwarmEvent},
    Multiaddr, NetworkBehaviour, PeerId, Swarm, TransportError,
};
use serde::{de::DeserializeOwned, Serialize};
use snafu::{ResultExt, Snafu};
use tracing::{debug, error, info, info_span, instrument, trace, warn, Instrument};

use crate::message::DirectMessageProtocol;

pub mod ui;
pub mod message;

/// `event_process` is false because
/// injecting events does not play well
/// with asyncrony
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "NetworkEvent<T>")]
#[behaviour(event_process = false)]
pub struct NetworkDef<
    T: std::fmt::Debug + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
> {
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
    pub request_response: RequestResponse<DirectMessageCodec<T>>,
}

#[derive(Debug)]
pub enum NetworkEvent<M: Send + Sync + std::fmt::Debug + Clone> {
    Gossip(GossipsubEvent),
    Kadem(KademliaEvent),
    Ident(IdentifyEvent),
    RequestResponse(RequestResponseEvent<DirectMessageRequest<M>, DirectMessageResponse>),
}

#[derive(Debug)]
pub enum SendDirectMsgEvent<T> {
    NotifyPeer(T),
}

impl<T: Send + Sync + Clone + 'static + std::fmt::Debug + Serialize + DeserializeOwned>
    From<IdentifyEvent> for NetworkEvent<T>
{
    fn from(source: IdentifyEvent) -> Self {
        NetworkEvent::Ident(source)
    }
}

impl<T: Send + Sync + Clone + 'static + std::fmt::Debug + Serialize + DeserializeOwned>
    From<KademliaEvent> for NetworkEvent<T>
{
    fn from(source: KademliaEvent) -> Self {
        NetworkEvent::Kadem(source)
    }
}

impl<T: Send + Sync + Clone + 'static + std::fmt::Debug + Serialize + DeserializeOwned>
    From<GossipsubEvent> for NetworkEvent<T>
{
    fn from(source: GossipsubEvent) -> Self {
        NetworkEvent::Gossip(source)
    }
}

impl<M: Send + Sync + Clone + 'static + std::fmt::Debug + Clone + Serialize + DeserializeOwned>
    From<RequestResponseEvent<DirectMessageRequest<M>, DirectMessageResponse>> for NetworkEvent<M>
{
    fn from(source: RequestResponseEvent<DirectMessageRequest<M>, DirectMessageResponse>) -> Self {
        NetworkEvent::RequestResponse(source)
    }
}

pub struct Network<
    T: Send + Sync + Clone + 'static + std::fmt::Debug + Serialize + DeserializeOwned,
> {
    pub connected_peers: HashSet<PeerId>,
    // TODO replace this with a set of queryids
    pub connecting_peers: HashSet<PeerId>,
    pub known_peers: HashSet<PeerId>,
    pub identity: Keypair,
    pub peer_id: PeerId,
    pub broadcast_topic: Topic,
    pub swarm: Swarm<NetworkDef<T>>,
    pub max_num_peers: usize,
    pub min_num_peers: usize,
}

/// holds requests to the swarm
pub enum SwarmAction<N: Send> {
    Shutdown,
    GossipMsg(N, Sender<Result<(), NetworkError>>), // topic, message
    GetId(Sender<PeerId>),
    Subscribe(String),
    Unsubscribe(String),
    DirectMessage(PeerId, N),
}

/// holds events of the swarm to be relayed to the cli event loop
/// out
pub enum SwarmResult<N: Send> {
    UpdateConnectedPeers(HashSet<PeerId>),
    UpdateKnownPeers(HashSet<PeerId>),
    GossipMsg(N),
    DirectMessage(N),
}

/// trait to get out the topic and contents of a message
/// such that it may be "gossipped" to other people
pub trait GossipMsg: Send {
    fn topic(&self) -> Topic;
    fn data(&self) -> Vec<u8>;
}

/// bind all interfaces on port `port`
/// TODO something more general
pub fn gen_multiaddr(port: u16) -> Multiaddr {
    build_multiaddr!(Ip4([0, 0, 0, 0]), Tcp(port))
}

impl<M> Network<M>
where
    M: std::fmt::Debug
        + Send
        + Sync
        + Clone
        + 'static
        + Serialize
        + DeserializeOwned
        + GossipMsg
        + From<GossipsubMessage>,
{
    /// starts the swarm listening on `listen_addr`
    /// and optionally dials into peer `known_peer`
    #[instrument(skip(self))]
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
        // TODO: Maybe not use a development only networking backend
        let transport: Boxed<(PeerId, StreamMuxerBox)> =
            libp2p::development_transport(identity.clone())
                .await
                .context(TransportLaunchSnafu)?;
        trace!("Launched network transport");
        let broadcast_topic = Topic::new("broadcast");
        // Generate the swarm
        let swarm: Swarm<NetworkDef<M>> = {
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
                DirectMessageCodec(PhantomData::<M>),
                iter::once((DirectMessageProtocol(), ProtocolSupport::Full)),
                RequestResponseConfig::default(),
            );

            let network = NetworkDef {
                gossipsub,
                kadem,
                identify,
                request_response,
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
            max_num_peers: 6,
            min_num_peers: 5,
            swarm,
        })
    }

    /// Spawn a task to listen for requests on the returned channel
    /// as well as any events produced by libp2p
    /// `mut_mut` is disabled b/c must consume `self`
    /// TODO why does clippy not like `panic` with select?
    #[allow(
        clippy::mut_mut,
        clippy::panic,
        clippy::single_match,
        clippy::collapsible_match
    )]
    #[instrument(skip(self))]
    pub async fn spawn_listeners(
        mut self,
    ) -> Result<(Sender<SwarmAction<M>>, Receiver<SwarmResult<M>>), NetworkError> {
        let (s_input, s_output) = unbounded::<SwarmAction<M>>();
        let (r_input, r_output) = unbounded::<SwarmResult<M>>();

        let mut bootstrapped = false;
        spawn(async move {
            loop {
                select! {
                    _ = sleep(Duration::from_secs(1)).fuse() => {
                        // if we're bootstrapped, do nothing
                        // otherwise periodically get more peers if needed
                        if !bootstrapped {
                            if self.connecting_peers.len() + self.connected_peers.len() <= self.min_num_peers {
                                let used_peers = self.connecting_peers.union(&self.connected_peers).copied().collect();
                                let potential_peers : HashSet<PeerId> = self.known_peers.difference(&used_peers).copied().collect();
                                for a_peer in potential_peers {
                                    match self.swarm.dial(a_peer) {
                                        Ok(_) => {
                                            self.connecting_peers.insert(a_peer);

                                        }
                                        Err(e) => error!("Dial {:?} failed: {:?}", a_peer, e),
                                    };
                                }
                            } else if self.connected_peers.len() > self.max_num_peers {
                                let peers_to_rm = self.connected_peers.iter().copied().choose_multiple(&mut thread_rng(), self.connected_peers.len() - self.max_num_peers);
                                for a_peer in peers_to_rm {
                                    // FIXME the error is () ?
                                    let _ = self.swarm.disconnect_peer_id(a_peer);
                                }

                            }
                        }
                    }
                    event = self.swarm.select_next_some() => {
                        // TODO re enable this
                        warn!("libp2p event {:?}", event);
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
                                    NetworkEvent::RequestResponse(msg) => {
                                        match msg {
                                            RequestResponseEvent::Message { message: RequestResponseMessage::Request{ request: DirectMessageRequest(m), ..}, .. } => {
                                                r_input.send_async(SwarmResult::DirectMessage(m)).await.map_err(|_e| NetworkError::StreamClosed)?;
                                            },
                                            _ => {}
                                        }
                                    }
                                    NetworkEvent::Gossip(g) => {
                                        match g {
                                            GossipsubEvent::Message { message, .. } => {
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
                                                    QueryResult::Bootstrap(r) => {
                                                        match r {
                                                             Ok(_bootstrap) => {
                                                                // we're bootstrapped
                                                                // don't bootstrap again
                                                                bootstrapped = false;
                                                             },
                                                             Err(_) => {
                                                                 // try again
                                                                bootstrapped = true;
                                                             }

                                                        }
                                                    },
                                                    QueryResult::GetClosestPeers(result) => {
                                                        match result {
                                                            Ok(GetClosestPeersOk { key: _key, peers }) => {
                                                                for peer in peers {
                                                                    self.known_peers.insert(peer);
                                                                }
                                                                r_input.send_async(SwarmResult::UpdateKnownPeers(self.known_peers.clone())).await.map_err(|_e| NetworkError::StreamClosed)?;
                                                            },
                                                            _ => {}
                                                        }
                                                    },
                                                    _ => {}
                                                }
                                            },
                                            KademliaEvent::RoutingUpdated { peer, is_new_peer: _is_new_peer, addresses: _addresses, .. } => {
                                                self.known_peers.insert(peer);
                                                r_input.send_async(SwarmResult::UpdateKnownPeers(self.known_peers.clone())).await.map_err(|_e| NetworkError::StreamClosed)?;
                                            },
                                            _ => {}
                                        }
                                    },
                                    NetworkEvent::Ident(i) => {
                                        match i {
                                            IdentifyEvent::Received { peer_id, info, .. } => {
                                                for addr in info.listen_addrs {
                                                    self.swarm.behaviour_mut().kadem
                                                        .add_address(&peer_id, addr.clone());
                                                }
                                                self.known_peers.insert(peer_id);
                                                r_input.send_async(SwarmResult::UpdateKnownPeers(self.known_peers.clone())).await.map_err(|_e| NetworkError::StreamClosed)?;
                                            },
                                            _ => {}
                                        }
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
                                    self.swarm.behaviour_mut().kadem.bootstrap().map_err(|_e| NetworkError::NoKnownPeers)?;
                                }
                                r_input.send_async(SwarmResult::UpdateConnectedPeers(self.connected_peers.clone())).await.map_err(|_e| NetworkError::StreamClosed)?;
                            }
                            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                                self.connected_peers.remove(&peer_id);
                                // FIXME remove stale address, not *all* addresses
                                self.swarm.behaviour_mut().kadem.remove_peer(&peer_id);

                                r_input.send_async(SwarmResult::UpdateConnectedPeers(self.connected_peers.clone())).await.map_err(|_e| NetworkError::StreamClosed)?;
                                r_input.send_async(SwarmResult::UpdateKnownPeers(self.known_peers.clone())).await.map_err(|_e| NetworkError::StreamClosed)?;
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
                                        let topic = <M as GossipMsg>::topic(&msg);
                                        let contents = <M as GossipMsg>::data(&msg);
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
                                    SwarmAction::DirectMessage(pid, msg) => {
                                        self.swarm.behaviour_mut().request_response.send_request(&pid, DirectMessageRequest(msg));
                                    }
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
    PublishError {
        source: PublishError,
    },
    /// Error when there are no known peers to bootstrap off
    NoKnownPeers,
}
