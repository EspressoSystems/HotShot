//! The [`ConsensusType`] trait allows consensus-specific customization points.

pub mod sequencing_consensus;
pub mod validating_consensus;

/// [`ConsensusType`] generalized trait
pub trait ConsensusType: Clone + Send + Sync {}
