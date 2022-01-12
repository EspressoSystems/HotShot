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

use std::marker::PhantomData;

use libp2p::{
    core::{muxing::StreamMuxerBox, transport::Boxed},
    gossipsub::{Gossipsub, GossipsubEvent},
    identify::{Identify, IdentifyEvent},
    identity::Keypair,
    kad::{store::MemoryStore, Kademlia, KademliaEvent},
    NetworkBehaviour, PeerId,
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
    _phantom: PhantomData<M>,
}

impl<M: DeserializeOwned + Serialize> Network<M> {
    /// Creates a new `Network` with the given settings.
    ///
    /// Currently:
    ///   * Generates a random key pair and associated [`PeerId`]
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
        Ok(Self {
            identity,
            peer_id,
            transport,
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
}
