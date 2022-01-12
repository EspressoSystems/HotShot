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
    core::{muxing::StreamMuxerBox, transport::Boxed},
    gossipsub::{
        Gossipsub, GossipsubConfig, GossipsubConfigBuilder, GossipsubEvent, GossipsubMessage,
        IdentTopic as Topic, MessageAuthenticity, MessageId, ValidationMode,
    },
    identify::{Identify, IdentifyEvent},
    identity::Keypair,
    kad::{store::MemoryStore, Kademlia, KademliaEvent},
    NetworkBehaviour, PeerId, Swarm,
};
use serde::{de::DeserializeOwned, Serialize};
use snafu::{ResultExt, Snafu, Whatever};
use tracing::{debug, instrument, trace};

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "NetworkEvent")]
#[behaviour(event_process = false)]
pub struct NetworkDef {
    pub gossipsub: Gossipsub,
    pub kadem: Kademlia<MemoryStore>,
    pub identity: Identify,
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

pub struct Network<M> {
    pub identity: Keypair,
    pub peer_id: PeerId,
    pub transport: Boxed<(PeerId, StreamMuxerBox)>,
    pub broadcast_topic: Topic,
    _phantom: PhantomData<M>,
}

impl<M: DeserializeOwned + Serialize> Network<M> {
    /// Creates a new `Network` with the given settings.
    ///
    /// Currently:
    ///   * Generates a random key pair and associated [`PeerId`]
    ///   * Launches a development-only type of transport backend
    ///   * Generates a connection to the "broadcast" topic
    ///   * Creates a swarm to manage peers and events
    #[instrument]
    pub async fn new(_: PhantomData<M>) -> Result<Self, NetworkError> {
        // Generate a random PeerId
        let identity = Keypair::generate_ed25519();
        let peer_id = PeerId::from(identity.public());
        debug!(?peer_id);
        // TODO: Maybe not use a development only networking backend
        let transport = libp2p::development_transport(identity.clone())
            .await
            .context(TransportLaunchSnafu)?;
        trace!("Launched network transport");
        let broadcast_topic = Topic::new("broadcast");
        // Generate the swarm
        let mut swarm: Swarm<NetworkDef> = {
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
            let gossipsub_config = match GossipsubConfigBuilder::default()
                // Use a reasonable 10 second heartbeat interval by default
                .heartbeat_interval(Duration::from_secs(10))
                // Force all messages to have valid signatures
                .validation_mode(ValidationMode::Strict)
                // Use the (blake3) hash of a message as its ID
                .message_id_fn(message_id_fn)
                .build()
            {
                Ok(x) => x,
                Err(string) => {
                    return Err(GossipsubConfigSnafu {
                        message: string.to_string(),
                    }
                    .build())
                }
            };
            // - Build a gossipsub network behavior
            let mut gossipsub: Gossipsub = Gossipsub::new(
                MessageAuthenticity::Signed(identity.clone()),
                gossipsub_config,
            )
            .map_err(|s| {
                GossipsubBuildSnafu {
                    message: s.to_string(),
                }
                .build()
            })?;
            todo!("I am supposed to panic here")
        };
        Ok(Self {
            identity,
            peer_id,
            transport,
            broadcast_topic,
            _phantom: PhantomData,
        })
    }
}

#[derive(Debug, Snafu)]
pub enum NetworkError {
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
