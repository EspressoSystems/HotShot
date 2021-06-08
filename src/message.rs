use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};
use threshold_crypto::SignatureShare;

use std::fmt::Debug;

use crate::{data::Leaf, BlockHash, QuorumCertificate};

#[derive(Serialize, Deserialize, Clone, Debug)]
/// Represents the messages `HotStuff` nodes send to each other
pub enum Message<B, T> {
    /// Signals start of a new view
    NewView(NewView),
    /// Contains the prepare qc from the leader
    Prepare(Prepare<B>),
    /// A nodes vote on the prepare stage
    PrepareVote(PrepareVote),
    /// Contains the precommit qc from the leader
    PreCommit(PreCommit),
    /// A node's vote on the precommit stage
    PreCommitVote(PreCommitVote),
    /// Contains the commit qc from the leader
    Commit(Commit),
    /// A node's vote on the commit stage
    CommitVote(CommitVote),
    /// Contains the decide qc from the leader
    Decide(Decide),
    /// Contains a transaction to be submitted
    SubmitTransaction(T),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
/// Signals the start of a new view
pub struct NewView {
    /// The current view
    pub current_view: u64,
    /// The justification qc for this view
    pub justify: super::QuorumCertificate,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
/// Prepare qc from the leader
pub struct Prepare<T> {
    /// The current view
    pub current_view: u64,
    /// The item being proposed
    pub leaf: Leaf<T>,
    /// The current high qc
    pub high_qc: QuorumCertificate,
}

#[derive(Serialize, Deserialize, Clone)]
/// A nodes vote on the prepare field
pub struct PrepareVote {
    /// The signature share associated with this vote
    pub signature: SignatureShare,
    /// Id of the voting nodes
    pub id: u64,
    /// Hash of the item being voted on
    pub leaf_hash: BlockHash,
}

impl Debug for PrepareVote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrepareVote")
            .field("signature", &self.signature)
            .field("id", &self.id)
            .field("leaf_hash", &format!("{:12}", HexFmt(&self.leaf_hash)))
            .finish()
    }
}

#[derive(Serialize, Deserialize, Clone)]
/// Pre-commit qc from the leader
pub struct PreCommit {
    /// Hash of the item being worked on
    pub leaf_hash: BlockHash,
    /// The pre commit qc
    pub qc: QuorumCertificate,
    /// The current view
    pub current_view: u64,
}

impl Debug for PreCommit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreCommit")
            .field("current_view", &self.current_view)
            .field("qc", &self.qc)
            .field("leaf_hash", &format!("{:12}", HexFmt(&self.leaf_hash)))
            .finish()
    }
}

#[derive(Serialize, Deserialize, Clone)]
/// A nodes vote on the precommit stage
pub struct PreCommitVote {
    /// Hash of the thing being voted on
    pub leaf_hash: BlockHash,
    /// The signature share for this vote
    pub signature: SignatureShare,
    /// The id of the voting node
    pub id: u64,
}

impl Debug for PreCommitVote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreCommitVote")
            .field("signature", &self.signature)
            .field("id", &self.id)
            .field("leaf_hash", &format!("{:12}", HexFmt(&self.leaf_hash)))
            .finish()
    }
}

#[derive(Serialize, Deserialize, Clone)]
/// `Commit` qc from the leader
pub struct Commit {
    /// Hash of the thing being worked on
    pub leaf_hash: BlockHash,
    /// The `Commit` qc
    pub qc: QuorumCertificate,
    /// The current view
    pub current_view: u64,
}

impl Debug for Commit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Commit")
            .field("current_view", &self.current_view)
            .field("qc", &self.qc)
            .field("leaf_hash", &format!("{:12}", HexFmt(&self.leaf_hash)))
            .finish()
    }
}

#[derive(Serialize, Deserialize, Clone)]
/// A nodes vote on the `Commit` stage
pub struct CommitVote {
    /// Hash of the thing being voted on
    pub leaf_hash: BlockHash,
    /// signature share for this vote
    pub signature: SignatureShare,
    /// the id of this voting node
    pub id: u64,
}

impl Debug for CommitVote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommitVote")
            .field("signature", &self.signature)
            .field("id", &self.id)
            .field("leaf_hash", &format!("{:12}", HexFmt(&self.leaf_hash)))
            .finish()
    }
}

#[derive(Serialize, Deserialize, Clone)]
/// Final decision
pub struct Decide {
    /// Hash of the thing we just decided on
    pub leaf_hash: BlockHash,
    /// final qc for the round
    pub qc: QuorumCertificate,
    /// the current view
    pub current_view: u64,
}

impl Debug for Decide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Decide")
            .field("current_view", &self.current_view)
            .field("qc", &self.qc)
            .field("leaf_hash", &format!("{:12}", HexFmt(&self.leaf_hash)))
            .finish()
    }
}
