//! Provides types useful for representing [`PhaseLock`](crate::PhaseLock)'s data structures
//!
//! This module provides types for representing consensus internal state, such as the [`Leaf`],
//! [`PhaseLock`](crate::PhaseLock)'s version of a block, and the [`QuorumCertificate`],
//! representing the threshold signatures fundamental to consensus.

pub use phaselock_types::data::{
    create_verify_hash, BlockHash, Leaf, LeafHash, QuorumCertificate, Stage, StateHash,
    TransactionHash, VecQuorumCertificate, VerifyHash,
};
