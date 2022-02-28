//! Network message types
//!
//! This module contains types used to represent the various types of messages that
//! `PhaseLock` nodes can send among themselves.

use crate::data::Stage;
use crate::data::{Leaf, LeafHash, QuorumCertificate};
use hex_fmt::HexFmt;
use std::fmt::Debug;
use threshold_crypto::SignatureShare;

#[derive(bincode::Encode, bincode::Decode, Clone, Debug)]
/// Enum representation of any message type
pub enum Message<B, T, S, const N: usize> {
    /// Signals start of a new view
    NewView(NewView<N>),
    /// Contains the prepare qc from the leader
    Prepare(Prepare<B, S, N>),
    /// A nodes vote on the prepare stage
    PrepareVote(Vote<N>),
    /// Contains the precommit qc from the leader
    PreCommit(PreCommit<N>),
    /// A node's vote on the precommit stage
    PreCommitVote(Vote<N>),
    /// Contains the commit qc from the leader
    Commit(Commit<N>),
    /// A node's vote on the commit stage
    CommitVote(Vote<N>),
    /// Contains the decide qc from the leader
    Decide(Decide<N>),
    /// Contains a transaction to be submitted
    SubmitTransaction(T),
}

#[derive(Debug, bincode::Encode, bincode::Decode, Clone)]
/// Signals the start of a new view
pub struct NewView<const N: usize> {
    /// The current view
    pub current_view: u64,
    /// The justification qc for this view
    pub justify: QuorumCertificate<N>,
}

#[derive(Debug, bincode::Encode, bincode::Decode, Clone)]
/// Prepare qc from the leader
pub struct Prepare<T, S, const N: usize> {
    /// The current view
    pub current_view: u64,
    /// The item being proposed
    pub leaf: Leaf<T, N>,
    /// The state this proposal results in
    pub state: S,
    /// The current high qc
    pub high_qc: QuorumCertificate<N>,
}

#[derive(bincode::Encode, bincode::Decode, Clone)]
/// A nodes vote on the prepare field
pub struct Vote<const N: usize> {
    /// The signature share associated with this vote
    #[bincode(with_serde)]
    pub signature: SignatureShare,
    /// Id of the voting nodes
    pub id: u64,
    /// Hash of the item being voted on
    pub leaf_hash: LeafHash<N>,
    /// The view this vote was cast for
    pub current_view: u64,
    /// The current stage
    pub stage: Stage,
}

impl<const N: usize> Debug for Vote<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrepareVote")
            .field("current_view", &self.current_view)
            .field("signature", &self.signature)
            .field("id", &self.id)
            .field("leaf_hash", &format!("{:12}", HexFmt(&self.leaf_hash)))
            .finish()
    }
}

#[derive(bincode::Encode, bincode::Decode, Clone)]
/// Pre-commit qc from the leader
pub struct PreCommit<const N: usize> {
    /// Hash of the item being worked on
    pub leaf_hash: LeafHash<N>,
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

#[derive(bincode::Encode, bincode::Decode, Clone)]
/// `Commit` qc from the leader
pub struct Commit<const N: usize> {
    /// Hash of the thing being worked on
    pub leaf_hash: LeafHash<N>,
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

#[derive(bincode::Encode, bincode::Decode, Clone)]
/// Final decision
pub struct Decide<const N: usize> {
    /// Hash of the thing we just decided on
    pub leaf_hash: LeafHash<N>,
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
