use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};
use threshold_crypto::SignatureShare;

use std::fmt::Debug;

use crate::{data::Leaf, BlockHash, QuorumCertificate};

#[derive(Serialize, Deserialize, Clone, Debug)]
/// Represents the messages `HotStuff` nodes send to each other
pub enum Message<B, T, const N: usize> {
    /// Signals start of a new view
    NewView(NewView<N>),
    /// Contains the prepare qc from the leader
    Prepare(Prepare<B, N>),
    /// A nodes vote on the prepare stage
    PrepareVote(PrepareVote<N>),
    /// Contains the precommit qc from the leader
    PreCommit(PreCommit<N>),
    /// A node's vote on the precommit stage
    PreCommitVote(PreCommitVote<N>),
    /// Contains the commit qc from the leader
    Commit(Commit<N>),
    /// A node's vote on the commit stage
    CommitVote(CommitVote<N>),
    /// Contains the decide qc from the leader
    Decide(Decide<N>),
    /// Contains a transaction to be submitted
    SubmitTransaction(T),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
/// Signals the start of a new view
pub struct NewView<const N: usize> {
    /// The current view
    pub current_view: u64,
    /// The justification qc for this view
    pub justify: super::QuorumCertificate<N>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
/// Prepare qc from the leader
pub struct Prepare<T, const N: usize> {
    /// The current view
    pub current_view: u64,
    /// The item being proposed
    pub leaf: Leaf<T, N>,
    /// The current high qc
    pub high_qc: QuorumCertificate<N>,
}

#[derive(Serialize, Deserialize, Clone)]
/// A nodes vote on the prepare field
pub struct PrepareVote<const N: usize> {
    /// The signature share associated with this vote
    pub signature: SignatureShare,
    /// Id of the voting nodes
    pub id: u64,
    /// Hash of the item being voted on
    pub leaf_hash: BlockHash<N>,
}

impl<const N: usize> Debug for PrepareVote<N> {
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
pub struct PreCommit<const N: usize> {
    /// Hash of the item being worked on
    pub leaf_hash: BlockHash<N>,
    /// The pre commit qc
    pub qc: QuorumCertificate<N>,
    /// The current view
    pub current_view: u64,
}

impl<const N: usize> Debug for PreCommit<N> {
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
pub struct PreCommitVote<const N: usize> {
    /// Hash of the thing being voted on
    pub leaf_hash: BlockHash<N>,
    /// The signature share for this vote
    pub signature: SignatureShare,
    /// The id of the voting node
    pub id: u64,
}

impl<const N: usize> Debug for PreCommitVote<N> {
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
pub struct Commit<const N: usize> {
    /// Hash of the thing being worked on
    pub leaf_hash: BlockHash<N>,
    /// The `Commit` qc
    pub qc: QuorumCertificate<N>,
    /// The current view
    pub current_view: u64,
}

impl<const N: usize> Debug for Commit<N> {
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
pub struct CommitVote<const N: usize> {
    /// Hash of the thing being voted on
    pub leaf_hash: BlockHash<N>,
    /// signature share for this vote
    pub signature: SignatureShare,
    /// the id of this voting node
    pub id: u64,
}

impl<const N: usize> Debug for CommitVote<N> {
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
pub struct Decide<const N: usize> {
    /// Hash of the thing we just decided on
    pub leaf_hash: BlockHash<N>,
    /// final qc for the round
    pub qc: QuorumCertificate<N>,
    /// the current view
    pub current_view: u64,
}

impl<const N: usize> Debug for Decide<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Decide")
            .field("current_view", &self.current_view)
            .field("qc", &self.qc)
            .field("leaf_hash", &format!("{:12}", HexFmt(&self.leaf_hash)))
            .finish()
    }
}
