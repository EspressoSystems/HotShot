//! Network message types
//!
//! This module contains types used to represent the various types of messages that
//! `HotShot` nodes can send among themselves.

use crate::{
    data::{BlockHash, Leaf, LeafHash, QuorumCertificate, ViewNumber},
    traits::{signature_key::{EncodedPublicKey, EncodedSignature}, BlockContents, StateContents},
};
use commit::{Commitment, Committable};
use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// Incoming message
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message<'b, S: StateContents<'b>, K> {
    /// The sender of this message
    pub sender: K,

    /// The message kind
    pub kind: MessageKind<'b, S>,
}

/// Enum representation of any message type
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MessageKind<'b, STATE: StateContents<'b>> {
    /// Messages related to the consensus protocol
    Consensus(ConsensusMessage<'b, STATE>),
    /// Messages relating to sharing data between nodes
    Data(DataMessage<'b, STATE>),
}

impl<'b, S: StateContents<'b>> From<ConsensusMessage<'b, S>> for MessageKind<'b, S> {
    fn from(m: ConsensusMessage<'b, S>) -> Self {
        Self::Consensus(m)
    }
}

impl<'b, S: StateContents<'b>> From<DataMessage<'b, S>> for MessageKind<'b, S> {
    fn from(m: DataMessage<'b, S>) -> Self {
        Self::Data(m)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, std::hash::Hash, PartialEq, Eq)]
/// Messages related to the consensus protocol
pub enum ConsensusMessage<'b, STATE: StateContents<'b>> {
    /// Leader's proposal
    Proposal(Proposal<'b, STATE>),
    /// Replica timed out
    TimedOut(TimedOut<'b, STATE>),
    /// Replica votes
    Vote(Vote<'b, STATE>),
    /// Internal ONLY message indicating a NextView interrupt
    /// View number this nextview interrupt was generated for
    /// used so we ignore stale nextview interrupts within a task
    #[serde(skip)]
    NextViewInterrupt(ViewNumber),
}

impl<'b, STATE: StateContents<'b>> ConsensusMessage<'b, STATE> {
    /// The view number of the (leader|replica) when the message was sent
    /// or the view of the timeout
    pub fn view_number(&self) -> ViewNumber {
        match self {
            ConsensusMessage::Proposal(p) => {
                // view of leader in the leaf when proposal
                // this should match replica upon receipt
                p.leaf.view_number
            }
            ConsensusMessage::TimedOut(t) => {
                // view number on which the replica timed out waiting for proposal
                t.current_view
            }
            ConsensusMessage::Vote(v) => {
                // view number on which the replica votes for a proposal for
                // the leaf should have this view number
                v.current_view
            }
            ConsensusMessage::NextViewInterrupt(view_number) => *view_number,
        }
    }
}

#[derive(Serialize, Clone, Debug, std::hash::Hash, PartialEq, Eq)]
/// Messages related to sending data between nodes
pub enum DataMessage<'b, S: StateContents<'b>> {
    /// The newest entry that a node knows. This is send from existing nodes to a new node when the new node joins the network
    NewestQuorumCertificate {
        /// The newest [`QuorumCertificate`]
        quorum_certificate: QuorumCertificate<'b, S>,

        /// The relevant [`BlockContents`]
        ///
        /// [`BlockContents`]: ../traits/block_contents/trait.BlockContents.html
        block: S::Block,

        /// The relevant [`State`]
        ///
        /// [`State`]: ../traits/state/trait.State.html
        state: S,
    },

    /// Contains a transaction to be submitted
    SubmitTransaction(<S::Block as BlockContents<'b>>::Transaction),
}

#[derive(Serialize, Deserialize, Clone, Debug, std::hash::Hash, PartialEq, Eq)]
/// Signals the start of a new view
pub struct TimedOut<'b, State: StateContents<'b>> {
    /// The current view
    pub current_view: ViewNumber,
    /// The justification qc for this view
    pub justify: QuorumCertificate<'b, State>,
}

#[derive(Serialize, Deserialize, Clone, Debug, std::hash::Hash, PartialEq, Eq)]
/// Prepare qc from the leader
pub struct Proposal<'b, STATE: StateContents<'b>> {
    // NOTE: optimization could include view number to help look up parent leaf
    // could even do 16 bit numbers if we want
    /// The leaf being proposed (see pseudocode)
    pub leaf: Leaf<'b, STATE>,
    /// The proposal must be signed by the view leader
    pub signature: EncodedSignature,
}

/// A nodes vote on the prepare field.
///
/// This should not be used directly. Consider using [`PrepareVote`], [`PreCommitVote`] or [`CommitVote`] instead.
#[derive(Serialize, Deserialize, Clone, custom_debug::Debug, std::hash::Hash, PartialEq, Eq)]
pub struct Vote<'b, STATE: StateContents<'b>> {
    /// hash of the block being proposed
    /// TODO delete this when we delete block hash from the QC
    pub block_hash: Commitment<STATE::Block>,
    /// TODO we should remove this
    /// this is correct, but highly inefficient
    /// we should check a cache, and if that fails request the qc
    pub justify_qc: QuorumCertificate<'b, STATE>,
    /// The signature share associated with this vote
    pub signature: (EncodedPublicKey, EncodedSignature),
    /// Hash of the item being voted on
    #[debug(with = "fmt_leaf_hash")]
    pub leaf_hash: Commitment<Leaf<'b, STATE>>,
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
