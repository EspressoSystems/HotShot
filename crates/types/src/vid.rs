//! This crate and all downstream crates should talk to the VID scheme only
//! via the traits exposed here.

/// Production VID scheme is ADVZ
#[cfg(not(feature = "vid-dummy"))]
mod advz;

#[cfg(not(feature = "vid-dummy"))]
pub use advz::*;

#[cfg(feature = "vid-dummy")]
mod dummy;
#[cfg(feature = "vid-dummy")]
pub use dummy::*;
