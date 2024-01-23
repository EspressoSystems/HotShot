//! Traits associated to versioning for the HotShot protocol.

use crate::Version;

pub trait Versioned {
    fn version(&self) -> Version;
}
