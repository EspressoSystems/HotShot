//! Network message types
//!
//! This module contains types used to represent the various types of messages that
//! `HotShot` nodes can send among themselves.

use crate::{
    data::{Leaf, ProposalLeaf, QuorumCertificate},
    traits::{
        node_implementation::NodeTypes,
        signature_key::{EncodedPublicKey, EncodedSignature},
    },
};
use commit::Commitment;
use derivative::Derivative;
use serde::{Deserialize, Serialize};

/// Incoming message
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]
pub struct Message<TYPES: NodeTypes> {
    /// The sender of this message
    pub sender: TYPES::SignatureKey,

    /// The message kind
    pub kind: MessageKind<TYPES>,
}

/// Enum representation of any message type
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]
pub enum MessageKind<TYPES: NodeTypes> {
    /// Messages related to the consensus protocol
    Consensus(ConsensusMessage<TYPES>),
    /// Messages relating to sharing data between nodes
    Data(DataMessage<TYPES>),
}

impl<TYPES: NodeTypes> From<ConsensusMessage<TYPES>> for MessageKind<TYPES> {
    fn from(m: ConsensusMessage<TYPES>) -> Self {
        Self::Consensus(m)
    }
}

impl<TYPES: NodeTypes> From<DataMessage<TYPES>> for MessageKind<TYPES> {
    fn from(m: DataMessage<TYPES>) -> Self {
        Self::Data(m)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]
/// Messages related to the consensus protocol
pub enum ConsensusMessage<TYPES: NodeTypes> {
    /// Leader's proposal
    Proposal(Proposal<TYPES>),
    /// Replica timed out
    TimedOut(TimedOut<TYPES>),
    /// Replica votes
    Vote(Vote<TYPES>),
    /// Internal ONLY message indicating a NextView interrupt
    /// View number this nextview interrupt was generated for
    /// used so we ignore stale nextview interrupts within a task
    #[serde(skip)]
    NextViewInterrupt(TYPES::Time),
}

impl<TYPES: NodeTypes> ConsensusMessage<TYPES> {
    /// The view number of the (leader|replica) when the message was sent
    /// or the view of the timeout
    pub fn time(&self) -> TYPES::Time {
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
            ConsensusMessage::NextViewInterrupt(time) => *time,
        }
    }
}

#[derive(Serialize, Deserialize, Derivative, Clone, Debug, PartialEq, Eq)]
#[serde(bound(deserialize = ""))]
/// Messages related to sending data between nodes
pub enum DataMessage<TYPES: NodeTypes> {
    /// The newest entry that a node knows. This is send from existing nodes to a new node when the new node joins the network
    NewestQuorumCertificate {
        /// The newest [`QuorumCertificate`]
        quorum_certificate: QuorumCertificate<TYPES>,

        /// The relevant [`BlockContents`]
        ///
        /// [`BlockContents`]: ../traits/block_contents/trait.BlockContents.html
        block: TYPES::BlockType,

        /// The relevant [`State`]
        ///
        /// [`State`]: ../traits/state/trait.State.html
        state: TYPES::StateType,

        /// The parent leaf's commitment
        parent_commitment: Commitment<Leaf<TYPES>>,

        /// Transactions rejected in this view
        rejected: Vec<TYPES::Transaction>,

        /// the proposer id for this leaf
        proposer_id: EncodedPublicKey,
    },

    /// Contains a transaction to be submitted
    SubmitTransaction(TYPES::Transaction),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(bound(deserialize = ""))]
/// Signals the start of a new view
pub struct TimedOut<TYPES: NodeTypes> {
    /// The current view
    pub current_view: TYPES::Time,
    /// The justification qc for this view
    pub justify_qc: QuorumCertificate<TYPES>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]
/// Prepare qc from the leader
pub struct Proposal<TYPES: NodeTypes> {
    // NOTE: optimization could include view number to help look up parent leaf
    // could even do 16 bit numbers if we want
    /// The leaf being proposed (see pseudocode)
    pub leaf: ProposalLeaf<TYPES>,
    /// The proposal must be signed by the view leader
    pub signature: EncodedSignature,
}

/// A nodes vote on the prepare field.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]
pub struct Vote<TYPES: NodeTypes> {
    /// hash of the block being proposed
    /// TODO delete this when we delete block hash from the QC
    pub block_commitment: Commitment<TYPES::BlockType>,
    /// TODO we should remove this
    /// this is correct, but highly inefficient
    /// we should check a cache, and if that fails request the qc
    pub justify_qc_commitment: Commitment<QuorumCertificate<TYPES>>,
    /// The signature share associated with this vote
    /// TODO ct/vrf make ConsensusMessage generic over I instead of serializing to a Vec<u8>
    pub signature: (EncodedPublicKey, EncodedSignature),
    /// Hash of the item being voted on
    pub leaf_commitment: Commitment<Leaf<TYPES>>,
    /// The view this vote was cast for
    pub current_view: TYPES::Time,
    /// The vote token generated by this replica
    pub vote_token: TYPES::VoteTokenType,
}
