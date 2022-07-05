use crate::network::NetworkNodeType;
use libp2p::{identity::Keypair, Multiaddr, PeerId};
use phaselock_types::constants::DEFAULT_REPLICATION_FACTOR;
use std::{collections::HashSet, num::NonZeroUsize};

/// describe the configuration of the network
#[derive(Clone, Default, derive_builder::Builder, custom_debug::Debug)]
pub struct NetworkNodeConfig {
    #[builder(default)]
    /// The type of node (bootstrap etc)
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
    #[builder(default)]
    pub bound_addr: Option<Multiaddr>,
    /// replication factor for entries in the DHT
    /// default is [`libp2p::kad::K_VALUE`] which is 20
    #[builder(setter(into, strip_option), default = "DEFAULT_REPLICATION_FACTOR")]
    pub replication_factor: Option<NonZeroUsize>,

    /// list of addresses to connect to at initialization
    pub to_connect_addrs: HashSet<(Option<PeerId>, Multiaddr)>,
}
