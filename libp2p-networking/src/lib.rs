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

use std::{marker::PhantomData, time::Duration};

use libp2p::{
    build_multiaddr,
    core::{muxing::StreamMuxerBox, transport::Boxed},
    gossipsub::{
        Gossipsub, GossipsubConfigBuilder, GossipsubEvent, GossipsubMessage, IdentTopic as Topic,
        MessageAuthenticity, MessageId, ValidationMode,
    },
    identify::{Identify, IdentifyConfig, IdentifyEvent},
    identity::Keypair,
    kad::{store::MemoryStore, Kademlia, KademliaEvent},
    swarm::NetworkBehaviour,
    Multiaddr, NetworkBehaviour, PeerId, Swarm, TransportError,
};
use serde::{de::DeserializeOwned, Serialize};
use snafu::{ResultExt, Snafu};
use tracing::{debug, instrument, trace};

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "NetworkEvent")]
#[behaviour(event_process = false)]
pub struct NetworkDef {
    pub gossipsub: Gossipsub,
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
    pub identity: Keypair,
    pub peer_id: PeerId,
    pub broadcast_topic: Topic,
    pub swarm: Swarm<M>,
    _phantom: PhantomData<N>,
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

impl<N: DeserializeOwned + Serialize, M: NetworkBehaviour> Network<N, M> {
    /// starts the swarm listening on `listen_addr`
    #[instrument]
    pub fn start(&mut self, listen_addr: Multiaddr) -> Result<(), NetworkError> {
        self.swarm.listen_on(listen_addr).context(TransportSnafu)?;
        Ok(())
    }
}

/// bind all interfaces on port `port`
/// TODO something more general
pub fn gen_multiaddr(port: u16) -> Multiaddr {
    build_multiaddr!(Ip4([0, 0, 0, 0]), Tcp(port))
}

impl<N: DeserializeOwned + Serialize> Network<N, NetworkDef> {
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
                // Use a reasonable 10 second heartbeat interval by default
                .heartbeat_interval(Duration::from_secs(10))
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
            let kadem = Kademlia::new(peer_id, MemoryStore::new(peer_id));

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
            broadcast_topic,
            swarm,
            _phantom: PhantomData,
        })
    }

    pub async fn spawn_listeners(&mut self){
        todo!()
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
    #[snafu(display("Error building the gossipsub configuration: {}", message))]
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
}
