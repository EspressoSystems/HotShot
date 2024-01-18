//! configurable constants for hotshot

use serde::{Deserialize, Serialize};

/// the number of views to gather information for ahead of time
pub const LOOK_AHEAD: u64 = 5;

/// the default kademlia record republication interval (in seconds)
pub const KAD_DEFAULT_REPUB_INTERVAL_SEC: u64 = 28800;

/// the number of messages to cache in the combined network
pub const COMBINED_NETWORK_CACHE_SIZE: usize = 1000;

/// the number of messages to attempt to send over the primary network before switching to prefer the secondary network
pub const COMBINED_NETWORK_MIN_PRIMARY_FAILURES: u64 = 5;

/// the number of messages to send over the secondary network before re-attempting the (presumed down) primary network
pub const COMBINED_NETWORK_PRIMARY_CHECK_INTERVAL: u64 = 5;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Type for protocol version number
pub struct Version {
    pub major: u16,
    pub minor: u16,
}

/// Constants for the current version number used by the program
pub const PROGRAM_PROTOCOL_VERSION: Version = Version { major: 0, minor: 1 };
