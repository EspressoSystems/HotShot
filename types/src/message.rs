//! Network message types
//!
//! This module contains types used to represent the various types of messages that
//! `HotShot` nodes can send among themselves.

use crate::{
    data::{Leaf, LeafHash, QuorumCertificate, ViewNumber, BlockHash},
    traits::{
        signature_key::{EncodedPublicKey, EncodedSignature},
        State,
    },
};
use hex_fmt::HexFmt;
use hotshot_utils::hack::nll_todo;
use serde::{Deserialize, Serialize};
use std::{fmt::Debug, marker::PhantomData};

/// Incoming message
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message<B, T, S, K, const N: usize> {
    /// The sender of this message
    pub sender: K,

    /// The message kind
    pub kind: MessageKind<B, T, S, N>,
}

/// Enum representation of any message type
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MessageKind<B, T, S, const N: usize> {
    /// Messages related to the consensus protocol
    Consensus(ConsensusMessage<B, S, N>),
    /// Messages relating to sharing data between nodes
    Data(DataMessage<B, T, S, N>),
}

impl<'a, B, T, S, const N: usize> From<ConsensusMessage<B, S, N>> for MessageKind<B, T, S, N> {
    fn from(m: ConsensusMessage<B, S, N>) -> Self {
        Self::Consensus(m)
    }
}

impl<'a, B, T, S, const N: usize> From<DataMessage<B, T, S, N>> for MessageKind<B, T, S, N> {
    fn from(m: DataMessage<B, T, S, N>) -> Self {
        Self::Data(m)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, std::hash::Hash, PartialEq, Eq)]
/// Messages related to the consensus protocol
pub enum ConsensusMessage<B, S, const N: usize> {
    /// Leader's proposal
    Proposal(Proposal<B, S, N>),
    /// Replica timed out
    TimedOut(TimedOut<N>),
    /// Replica votes
    Vote(Vote<N>),
    /// Internal ONLY message indicating a NextView interrupt
    /// View number this nextview interrupt was generated for
    /// used so we ignore stale nextview interrupts within a task
    NextViewInterrupt(ViewNumber),
}

impl<B, S, const N: usize> ConsensusMessage<B, S, N> {
    /// The view number of the (leader|replica) when the message was sent
    /// or the view of the timeout
    pub fn view_number(&self) -> ViewNumber {
        match self {
            ConsensusMessage::Proposal(p) => {
                // view of leader in the leaf when proposal
                // this should match replica upon receipt
                p.leaf.view_number
            },
            ConsensusMessage::TimedOut(t) => {
                // view number on which the replica timed out waiting for proposal
                t.current_view
            },
            ConsensusMessage::Vote(v) => {
                // view number on which the replica votes for a proposal for
                // the leaf should have this view number
                v.current_view
            },
            ConsensusMessage::NextViewInterrupt(view_number) => {
                *view_number
            },
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, std::hash::Hash, PartialEq, Eq)]
/// Messages related to sending data between nodes
pub enum DataMessage<B, T, S, const N: usize> {
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

    /// Contains a transaction to be submitted
    SubmitTransaction(T),
}

#[derive(Serialize, Deserialize, Clone, Debug, std::hash::Hash, PartialEq, Eq)]
/// Signals the start of a new view
pub struct TimedOut<const N: usize> {
    /// The current view
    pub current_view: ViewNumber,
    /// The justification qc for this view
    pub justify: QuorumCertificate<N>,
}

#[derive(Serialize, Deserialize, Clone, Debug, std::hash::Hash, PartialEq, Eq)]
/// Prepare qc from the leader
pub struct Proposal<BLOCK, STATE, const N: usize> {
    // NOTE: optimization could include view number to help look up parent leaf
    // could even do 16 bit numbers if we want
    /// The leaf being proposed (see pseudocode)
    pub leaf: Leaf<BLOCK, STATE, N>,
}

/// A nodes vote on the prepare field.
///
/// This should not be used directly. Consider using [`PrepareVote`], [`PreCommitVote`] or [`CommitVote`] instead.
#[derive(Serialize, Deserialize, Clone, custom_debug::Debug, std::hash::Hash, PartialEq, Eq)]
pub struct Vote<const N: usize> {
    /// hash of the block being proposed
    /// TODO delete this when we delete block hash from the QC
    pub block_hash: BlockHash<N>,
    /// TODO we should remove this
    /// this is correct, but highly inefficient
    /// we should check a cache, and if that fails request the qc
    pub justify_qc: QuorumCertificate<N>,
    /// The signature share associated with this vote
    pub signature: (EncodedPublicKey, EncodedSignature),
    /// Hash of the item being voted on
    #[debug(with = "fmt_leaf_hash")]
    pub leaf_hash: LeafHash<N>,
    /// The view this vote was cast for
    pub current_view: ViewNumber,
}

/// Format a [`LeafHash`] with [`HexFmt`]
fn fmt_leaf_hash<const N: usize>(
    n: &LeafHash<N>,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
    write!(f, "{:12}", HexFmt(n))
}
