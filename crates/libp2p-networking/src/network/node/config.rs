// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::{sync::Arc, time::Duration};

use async_lock::RwLock;
use hotshot_types::traits::node_implementation::NodeType;
use libp2p::Multiaddr;
use libp2p_identity::Keypair;

use crate::network::behaviours::dht::record::RecordValue;

use super::MAX_GOSSIP_MSG_SIZE;

/// Configuration for Libp2p's Gossipsub
#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct GossipConfig {
    /// The heartbeat interval
    pub heartbeat_interval: Duration,

    /// The number of past heartbeats to gossip about
    pub history_gossip: usize,
    /// The number of past heartbeats to remember the full messages for
    pub history_length: usize,

    /// The target number of peers in the mesh
    pub mesh_n: usize,
    /// The maximum number of peers in the mesh
    pub mesh_n_high: usize,
    /// The minimum number of peers in the mesh
    pub mesh_n_low: usize,
    /// The minimum number of mesh peers that must be outbound
    pub mesh_outbound_min: usize,

    /// The maximum gossip message size
    pub max_transmit_size: usize,

    /// The maximum number of messages in an IHAVE message
    pub max_ihave_length: usize,

    /// Maximum number of IHAVE messages to accept from a peer within a heartbeat
    pub max_ihave_messages: usize,

    /// Cache duration for published message IDs
    pub published_message_ids_cache_time: Duration,

    /// Time to wait for a message requested through IWANT following an IHAVE advertisement
    pub iwant_followup_time: Duration,

    /// The maximum number of messages we will process in a given RPC
    pub max_messages_per_rpc: Option<usize>,

    /// Controls how many times we will allow a peer to request the same message id through IWANT gossip before we start ignoring them.
    pub gossip_retransmission: u32,

    /// If enabled newly created messages will always be sent to all peers that are subscribed to the topic and have a good enough score.
    pub flood_publish: bool,

    /// The time period that messages are stored in the cache
    pub duplicate_cache_time: Duration,

    /// Time to live for fanout peers
    pub fanout_ttl: Duration,

    /// Initial delay in each heartbeat
    pub heartbeat_initial_delay: Duration,

    /// Affects how many peers we will emit gossip to at each heartbeat
    pub gossip_factor: f64,

    /// Minimum number of peers to emit gossip to during a heartbeat
    pub gossip_lazy: usize,
}

impl Default for GossipConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(1), // Default of Libp2p

            // The following are slightly modified defaults of Libp2p
            history_gossip: 6, // The number of past heartbeats to gossip about
            history_length: 8, // The number of past heartbeats to remember the full messages for

            // The mesh parameters are borrowed from Ethereum:
            // https://github.com/ethereum/consensus-specs/blob/dev/specs/phase0/p2p-interface.md#the-gossip-domain-gossipsub
            mesh_n: 8,            // The target number of peers in the mesh
            mesh_n_high: 12,      // The maximum number of peers in the mesh
            mesh_n_low: 6,        // The minimum number of peers in the mesh
            mesh_outbound_min: 2, // The minimum number of mesh peers that must be outbound

            max_ihave_length: 5000,
            max_ihave_messages: 10,
            published_message_ids_cache_time: Duration::from_secs(60 * 20), // 20 minutes
            iwant_followup_time: Duration::from_secs(3),
            max_messages_per_rpc: None,
            gossip_retransmission: 3,
            flood_publish: true,
            duplicate_cache_time: Duration::from_secs(60),
            fanout_ttl: Duration::from_secs(60),
            heartbeat_initial_delay: Duration::from_secs(5),
            gossip_factor: 0.25,
            gossip_lazy: 6,

            max_transmit_size: MAX_GOSSIP_MSG_SIZE, // The maximum gossip message size
        }
    }
}

/// Configuration for Libp2p's request-response
#[derive(Clone, Debug)]
pub struct RequestResponseConfig {
    /// The maximum request size in bytes
    pub request_size_maximum: u64,
    /// The maximum response size in bytes
    pub response_size_maximum: u64,
}

impl Default for RequestResponseConfig {
    fn default() -> Self {
        Self {
            request_size_maximum: 20 * 1024 * 1024,
            response_size_maximum: 20 * 1024 * 1024,
        }
    }
}

/// Configuration for Libp2p's Kademlia
#[derive(Clone, Debug)]
pub struct KademliaConfig<T: NodeType> {
    /// The replication factor
    pub replication_factor: usize,
    /// The record ttl
    pub record_ttl: Option<Duration>,
    /// The publication interval
    pub publication_interval: Option<Duration>,
    /// The file path for the [file-backed] record store
    pub file_path: String,
    /// The lookup record value (a signed peer ID so it can be verified by any node)
    pub lookup_record_value: RecordValue<T::SignatureKey>,
}

/// Configuration for Libp2p
#[derive(Clone)]
pub struct Libp2pConfig<T: NodeType> {
    /// The Libp2p keypair
    pub keypair: Keypair,
    /// The address to bind Libp2p to
    pub bind_address: Multiaddr,
    /// Addresses of known peers to add to Libp2p on startup
    pub known_peers: Vec<Multiaddr>,

    /// The quorum membership
    pub quorum_membership: Option<Arc<RwLock<T::Membership>>>,
    /// The (signed) authentication message
    pub auth_message: Option<Vec<u8>>,

    /// The gossip config
    pub gossip_config: GossipConfig,
    /// The request response config
    pub request_response_config: RequestResponseConfig,
    /// The kademlia config
    pub kademlia_config: KademliaConfig<T>,
}
