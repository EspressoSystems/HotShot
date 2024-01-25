//! Traits associated to versioning for the network protocol.

use crate::Version;

/// Trait for types that have a version
pub trait Versioned {
    /// Get version
    fn version(&self) -> Version;
}
