use serde::{Deserialize, Serialize};
use threshold_crypto::SignatureShare;

use crate::{BlockHash, BlockRef, QuorumCertificate, Stage};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Message<B, T> {
    SubmitTransaction {
        transaction: T,
    },
    ProposeBlock {
        phase: Stage,
        proposal: Proposal<B>,
        hash: BlockHash,
    },
    Vote {
        phase: Stage,
        hash: BlockHash,
        vote: Vote,
    },
    Commit {
        hash: BlockHash,
        qc: QuorumCertificate,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Proposal<B> {
    block: BlockRef<B>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Vote {
    phase: Stage,
    hash: BlockHash,
    signature: SignatureShare,
}
