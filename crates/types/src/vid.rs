//! This module provides:
//! - an opaque constructor [`vid_scheme`] that returns a new instance of a
//!   VID scheme.
//! - type aliases [`VidCommitment`], [`VidCommon`], [`VidShare`]
//!   for [`VidScheme`] assoc types.
//!
//! Purpose: the specific choice of VID scheme is an implementation detail.
//! This crate and all downstream crates should talk to the VID scheme only
//! via the traits exposed here.
use jf_vid::{precomputable::Precomputable, VidScheme};

use crate::{
    data::{VidDisperse as HotShotVidDisperse, VidDisperseShare},
    message::Proposal,
};

/// Production VID scheme is ADVZ
#[cfg(not(feature = "vid-dummy"))]
mod advz;

#[cfg(not(feature = "vid-dummy"))]
pub use advz::*;

#[cfg(feature = "vid-dummy")]
mod dummy;
#[cfg(feature = "vid-dummy")]
pub use dummy::*;

/// VID commitment type
pub type VidCommitment = <VidSchemeType as VidScheme>::Commit;
/// VID common type
pub type VidCommon = <VidSchemeType as VidScheme>::Common;
/// VID share type
pub type VidShare = <VidSchemeType as VidScheme>::Share;
/// VID PrecomputeData type
pub type VidPrecomputeData = <VidSchemeType as Precomputable>::PrecomputeData;
/// VID proposal type
pub type VidProposal<TYPES> = (
    Proposal<TYPES, HotShotVidDisperse<TYPES>>,
    Vec<Proposal<TYPES, VidDisperseShare<TYPES>>>,
);
