use serde::{Deserialize, Serialize};
use threshold_crypto::SignatureShare;

use crate::{Block, BlockHash, ReplicaId};

/// Abstraction for Proposal messages
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Proposal<B> {
    /// The node proposing the block
    proposer: ReplicaId,
    /// The block being proposed
    block: Block<B>,
}

/// Abstraction for Vote messages
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Vote {
    /// The node casting this vote
    voter: ReplicaId,
    /// Hash of the block being voted for
    block_hash: BlockHash,
    /// The signature share for this vote
    ///
    /// Computed as a signature of the hash of the block
    cert: SignatureShare,
}
