//! Network message types
//!
//! This module contains types used to represent the various types of messages that
//! `PhaseLock` nodes can send among themselves.

use crate::data::Stage;
use crate::data::{Leaf, LeafHash, QuorumCertificate};
use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use threshold_crypto::SignatureShare;

#[derive(Serialize, Deserialize, Clone, Debug)]
/// Enum representation of any message type
pub enum Message<B, T, S, const N: usize> {
    /// Messages related to the consensus protocol
    Consensus(ConsensusMessage<B, T, S, N>),
    /// Messages relating to sharing data between nodes
    Data(DataMessage<B, S, N>),
}

impl<B, T, S, const N: usize> From<ConsensusMessage<B, T, S, N>> for Message<B, T, S, N> {
    fn from(m: ConsensusMessage<B, T, S, N>) -> Self {
        Self::Consensus(m)
    }
}

impl<B, T, S, const N: usize> From<DataMessage<B, S, N>> for Message<B, T, S, N> {
    fn from(m: DataMessage<B, S, N>) -> Self {
        Self::Data(m)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
/// Messages related to the consensus protocol
pub enum ConsensusMessage<B, T, S, const N: usize> {
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

#[derive(Serialize, Deserialize, Clone, Debug)]
/// Messages related to sending data between nodes
pub enum DataMessage<B, S, const N: usize> {
    /// The newest entry that a node knows. This is send from existing nodes to a new node when the new node joins the network
    NewestQuorumCertificate {
        /// The newest [`QuorumCertificate`]
        quorum_certificate: QuorumCertificate<N>,
        /// The relevant [`BlockContents`]
        ///
        /// [`BlockContents`]: ../traits/block_contents/trait.BlockContents.html
        block: B,
        /// The relevant [`State`]
        ///
        /// [`State`]: ../traits/state/trait.State.html
        state: S,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
/// Signals the start of a new view
pub struct NewView<const N: usize> {
    /// The current view
    pub current_view: u64,
    /// The justification qc for this view
    pub justify: QuorumCertificate<N>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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

#[derive(Serialize, Deserialize, Clone, custom_debug::Debug)]
/// A nodes vote on the prepare field
pub struct Vote<const N: usize> {
    /// The signature share associated with this vote
    pub signature: SignatureShare,
    /// Id of the voting nodes
    pub id: u64,
    /// Hash of the item being voted on
    #[debug(with = "fmt_leaf_hash")]
    pub leaf_hash: LeafHash<N>,
    /// The view this vote was cast for
    pub current_view: u64,
    /// The current stage
    #[debug(skip)]
    pub stage: Stage,
}

#[derive(Serialize, Deserialize, Clone, custom_debug::Debug)]
/// Pre-commit qc from the leader
pub struct PreCommit<const N: usize> {
    /// Hash of the item being worked on
    #[debug(with = "fmt_leaf_hash")]
    pub leaf_hash: LeafHash<N>,
    /// The pre commit qc
    pub qc: QuorumCertificate<N>,
    /// The current view
    pub current_view: u64,
}

#[derive(Serialize, Deserialize, Clone, custom_debug::Debug)]
/// `Commit` qc from the leader
pub struct Commit<const N: usize> {
    /// Hash of the thing being worked on
    #[debug(with = "fmt_leaf_hash")]
    pub leaf_hash: LeafHash<N>,
    /// The `Commit` qc
    pub qc: QuorumCertificate<N>,
    /// The current view
    pub current_view: u64,
}

#[derive(Serialize, Deserialize, Clone, custom_debug::Debug)]
/// Final decision
pub struct Decide<const N: usize> {
    /// Hash of the thing we just decided on
    #[debug(with = "fmt_leaf_hash")]
    pub leaf_hash: LeafHash<N>,
    /// final qc for the round
    pub qc: QuorumCertificate<N>,
    /// the current view
    pub current_view: u64,
}

/// Format a [`LeafHash`] with [`HexFmt`]
fn fmt_leaf_hash<const N: usize>(
    n: &LeafHash<N>,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
    write!(f, "{:12}", HexFmt(n))
}
