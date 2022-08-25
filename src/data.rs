//! Provides types useful for representing [`HotShot`](crate::HotShot)'s data structures
//!
//! This module provides types for representing consensus internal state, such as the [`Leaf`],
//! [`HotShot`](crate::HotShot)'s version of a block, and the [`QuorumCertificate`],
//! representing the threshold signatures fundamental to consensus.

pub use hotshot_types::data::{
    BlockHash, Leaf, LeafHash, QuorumCertificate, StateHash, TransactionHash, VecQuorumCertificate,
    VerifyHash,
};
