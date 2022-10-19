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
#[derive(Serialize, Deserialize, Derivative)]
#[serde(bound = "")]
#[derivative(Clone(bound = ""), Debug(bound = ""), PartialEq(bound = ""))]
pub struct Message<TYPES: NodeTypes> {
    /// The sender of this message
    pub sender: TYPES::SignatureKey,

    /// The message kind
    // #[serde(deserialize_with = "<MessageKind<STATE> as Deserialize>::deserialize")]
    pub kind: MessageKind<TYPES>,
}

/// Enum representation of any message type
#[derive(Serialize, Deserialize, Derivative)]
#[serde(bound = "")]
#[derivative(Clone(bound = ""), Debug(bound = ""), PartialEq(bound = ""))]
pub enum MessageKind<TYPES: NodeTypes> {
    /// Messages related to the consensus protocol
    Consensus(
        // #[serde(deserialize_with = "<ConsensusMessage<STATE> as Deserialize>::deserialize")]
        ConsensusMessage<TYPES>,
    ),
    /// Messages relating to sharing data between nodes
    Data(
        // #[serde(deserialize_with = "<DataMessage<STATE> as Deserialize>::deserialize")]
        DataMessage<TYPES>,
    ),
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

#[derive(Serialize, Deserialize, derivative::Derivative)]
#[serde(bound = "")]
#[derivative(Clone(bound = ""), Debug(bound = ""), PartialEq(bound = ""))]
/// Messages related to the consensus protocol
pub enum ConsensusMessage<TYPES: NodeTypes> {
    /// Leader's proposal
    Proposal(
        // #[serde(deserialize_with = "<Proposal<STATE> as Deserialize>::deserialize")]
        Proposal<TYPES>,
    ),
    /// Replica timed out
    TimedOut(
        // #[serde(deserialize_with = "<TimedOut<STATE> as Deserialize>::deserialize")]
        TimedOut<TYPES>,
    ),
    /// Replica votes
    Vote(
        // #[serde(deserialize_with = "<Vote<STATE> as Deserialize>::deserialize")]
        Vote<TYPES>,
    ),
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
                p.leaf.time
            }
            ConsensusMessage::TimedOut(t) => {
                // view number on which the replica timed out waiting for proposal
                t.current_time
            }
            ConsensusMessage::Vote(v) => {
                // view number on which the replica votes for a proposal for
                // the leaf should have this view number
                v.time
            }
            ConsensusMessage::NextViewInterrupt(time) => *time,
        }
    }
}

#[derive(Serialize, Deserialize, Derivative)]
#[serde(bound = "")]
#[derivative(Clone(bound = ""), Debug(bound = ""), PartialEq(bound = ""))]
/// Messages related to sending data between nodes
pub enum DataMessage<TYPES: NodeTypes> {
    /// The newest entry that a node knows. This is send from existing nodes to a new node when the new node joins the network
    NewestQuorumCertificate {
        /// The newest [`QuorumCertificate`]
        // #[serde(deserialize_with = "<QuorumCertificate<STATE> as Deserialize>::deserialize")]
        quorum_certificate: QuorumCertificate<TYPES>,

        /// The relevant [`BlockContents`]
        ///
        /// [`BlockContents`]: ../traits/block_contents/trait.BlockContents.html
        // #[serde(deserialize_with = "<STATE::BlockType as Deserialize>::deserialize")]
        block: TYPES::BlockType,

        /// The relevant [`State`]
        ///
        /// [`State`]: ../traits/state/trait.State.html
        // #[serde(deserialize_with = "<STATE as Deserialize>::deserialize")]
        state: TYPES::StateType,

        /// The parent leaf's commitment
        // #[serde(deserialize_with = "<Commitment<Leaf<STATE>>as Deserialize>::deserialize")]
        parent_commitment: Commitment<Leaf<TYPES>>,

        /// Transactions rejected in this view
        // #[serde(deserialize_with = "<Vec<TxnCommitment<STATE>>as Deserialize>::deserialize")]
        rejected: Vec<TYPES::Transaction>,

        /// the proposer id for this leaf
        proposer_id: EncodedPublicKey,
    },

    /// Contains a transaction to be submitted
    SubmitTransaction(TYPES::Transaction),
}

#[derive(Serialize, Deserialize, derivative::Derivative)]
#[serde(bound = "")]
#[derivative(Clone(bound = ""), Debug(bound = ""), PartialEq(bound = ""))]
/// Signals the start of a new view
pub struct TimedOut<TYPES: NodeTypes> {
    /// The current view
    pub current_time: TYPES::Time,
    /// The justification qc for this view
    // #[serde(deserialize_with = "<QuorumCertificate<STATE> as Deserialize>::deserialize")]
    pub justify_qc: QuorumCertificate<TYPES>,
}

#[derive(Serialize, Deserialize, derivative::Derivative)]
#[serde(bound = "")]
#[derivative(Clone(bound = ""), Debug(bound = ""), PartialEq(bound = ""))]
/// Prepare qc from the leader
pub struct Proposal<TYPES: NodeTypes> {
    // NOTE: optimization could include view number to help look up parent leaf
    // could even do 16 bit numbers if we want
    /// The leaf being proposed (see pseudocode)
    // #[serde(deserialize_with = "<ProposalLeaf<STATE> as Deserialize>::deserialize")]
    pub leaf: ProposalLeaf<TYPES>,
    /// The proposal must be signed by the view leader
    pub signature: EncodedSignature,
}

/// A nodes vote on the prepare field.
#[derive(Serialize, Deserialize, derivative::Derivative)]
#[serde(bound = "")]
#[derivative(
    Clone(bound = ""),
    Debug(bound = ""),
    PartialEq(bound = ""),
    Debug(bound = "")
)]
pub struct Vote<TYPES: NodeTypes> {
    /// hash of the block being proposed
    /// TODO delete this when we delete block hash from the QC
    // #[serde(deserialize_with = "<Commitment<STATE::BlockType> as Deserialize>::deserialize")]
    pub block_commitment: Commitment<TYPES::BlockType>,
    /// TODO we should remove this
    /// this is correct, but highly inefficient
    /// we should check a cache, and if that fails request the qc
    // #[serde(deserialize_with = "<QuorumCertificate<STATE> as Deserialize>::deserialize")]
    pub justify_qc: QuorumCertificate<TYPES>,
    /// The signature share associated with this vote
    /// TODO ct/vrf: use VoteToken
    /// TODO ct/vrf make ConsensusMessage generic over I instead of serializing to a Vec<u8>
    pub signature: (EncodedPublicKey, EncodedSignature),
    /// Hash of the item being voted on
    // #[serde(deserialize_with = "<Commitment<Leaf<STATE>> as Deserialize>::deserialize")]
    pub leaf_commitment: Commitment<Leaf<TYPES>>,
    /// The view this vote was cast for
    pub time: TYPES::Time,
    /// The vote token generated by this replica
    pub vote_token: TYPES::VoteTokenType,
}
