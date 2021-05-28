use serde::{Deserialize, Serialize};
use threshold_crypto::SignatureShare;

use crate::{data::Leaf, BlockHash, QuorumCertificate};

#[derive(Serialize, Deserialize, Clone)]
pub enum Message<T> {
    NewView(NewView),
    Prepare(Prepare<T>),
    PrepareVote(PrepareVote),
    PreCommit(PreCommit),
    PreCommitVote(PreCommitVote),
    Commit(Commit),
    CommitVote(CommitVote),
    Decide(Decide),
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

#[derive(Serialize, Deserialize, Clone)]
pub struct PreCommit {
    pub leaf_hash: BlockHash,
    pub qc: QuorumCertificate,
    pub current_view: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PreCommitVote {
    pub leaf_hash: BlockHash,
    pub signature: SignatureShare,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Commit {
    pub leaf_hash: BlockHash,
    pub qc: QuorumCertificate,
    pub current_view: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CommitVote {
    pub leaf_hash: BlockHash,
    pub signature: SignatureShare,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Decide {
    pub leaf_hash: BlockHash,
    pub qc: QuorumCertificate,
    pub current_view: u64,
}
