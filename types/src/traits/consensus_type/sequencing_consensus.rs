//! The [`SequencingConsensusType`] trait allows consensus-specific customization points.

use crate::traits::consensus_type::ConsensusType;

/// Marker trait for consensus which provides availability and ordering but not execution.
pub trait SequencingConsensusType
where
    Self: ConsensusType,
{
}

/// Consensus which provides availability and ordering but not execution.
#[derive(Clone)]
pub struct SequencingConsensus;
impl SequencingConsensusType for SequencingConsensus {}
impl ConsensusType for SequencingConsensus {}
