use serde::{Deserialize, Serialize};
use threshold_crypto::SignatureShare;

use crate::{data::Leaf, BlockHash, QuorumCertificate};

#[derive(Serialize, Deserialize, Clone)]
pub enum Message<T> {
    NewView(NewView),
    Prepare(Prepare<T>),
    PrepareVote(PrepareVote),
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NewView {
    pub current_view: u64,
    pub justify: super::QuorumCertificate,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Prepare<T> {
    pub current_view: u64,
    pub leaf: Leaf<T>,
    pub high_qc: QuorumCertificate,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PrepareVote {
    pub signature: SignatureShare,
    pub leaf_hash: BlockHash,
}
