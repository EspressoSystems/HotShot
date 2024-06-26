//! This module provides:
//! - an opaque constructor [`vid_scheme`] that returns a new instance of a
//!   VID scheme.
//! - type aliases [`VidCommitment`], [`VidCommon`], [`VidShare`]
//!   for [`VidScheme`] assoc types.
//!
//! Purpose: the specific choice of VID scheme is an implementation detail.
//! This crate and all downstream crates should talk to the VID scheme only
//! via the traits exposed here.

/// ADVZ module
#[cfg(not(feature = "novid"))]
mod advz;
/// re-export inner module
#[cfg(not(feature = "novid"))]
pub use advz::*;

/// Dummy VID module
#[cfg(feature = "novid")]
mod dummy;
#[cfg(feature = "novid")]
pub use dummy::*;
