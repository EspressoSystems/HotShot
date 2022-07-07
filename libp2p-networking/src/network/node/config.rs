use crate::network::NetworkNodeType;
use hotshot_types::constants::DEFAULT_REPLICATION_FACTOR;
use libp2p::{identity::Keypair, Multiaddr, PeerId};
use std::{collections::HashSet, num::NonZeroUsize};

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
    /// replication factor for entries in the DHT
    /// default is [`libp2p::kad::K_VALUE`] which is 20
    #[builder(setter(into, strip_option), default = "DEFAULT_REPLICATION_FACTOR")]
    pub replication_factor: Option<NonZeroUsize>,
}
