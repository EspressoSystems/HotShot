//! Network message types
//!
//! This module contains types used to represent the various types of messages that
//! `HotShot` nodes can send among themselves.

use crate::{
    data::{Leaf, ProposalLeaf, QuorumCertificate, TxnCommitment, ViewNumber},
    traits::{
        signature_key::{EncodedPublicKey, EncodedSignature},
        Block, State,
    },
};
use commit::Commitment;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// Incoming message
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message<STATE: State, K> {
    /// The sender of this message
    pub sender: K,

    /// The message kind
    #[serde(deserialize_with = "<MessageKind<STATE> as Deserialize>::deserialize")]
    pub kind: MessageKind<STATE>,
}

/// Enum representation of any message type
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MessageKind<STATE: State> {
    /// Messages related to the consensus protocol
    Consensus(
        #[serde(deserialize_with = "<ConsensusMessage<STATE> as Deserialize>::deserialize")]
        ConsensusMessage<STATE>,
    ),
    /// Messages relating to sharing data between nodes
    Data(
        #[serde(deserialize_with = "<DataMessage<STATE> as Deserialize>::deserialize")]
        DataMessage<STATE>,
    ),
}

impl<STATE: State> From<ConsensusMessage<STATE>> for MessageKind<STATE> {
    fn from(m: ConsensusMessage<STATE>) -> Self {
        Self::Consensus(m)
    }
}

impl<STATE: State> From<DataMessage<STATE>> for MessageKind<STATE> {
    fn from(m: DataMessage<STATE>) -> Self {
        Self::Data(m)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, std::hash::Hash, PartialEq, Eq)]
/// Messages related to the consensus protocol
pub enum ConsensusMessage<STATE: State> {
    /// Leader's proposal
    Proposal(
        #[serde(deserialize_with = "<Proposal<STATE> as Deserialize>::deserialize")]
        Proposal<STATE>,
    ),
    /// Replica timed out
    TimedOut(
        #[serde(deserialize_with = "<TimedOut<STATE> as Deserialize>::deserialize")]
        TimedOut<STATE>,
    ),
    /// Replica votes
    Vote(#[serde(deserialize_with = "<Vote<STATE> as Deserialize>::deserialize")] Vote<STATE>),
    /// Internal ONLY message indicating a NextView interrupt
    /// View number this nextview interrupt was generated for
    /// used so we ignore stale nextview interrupts within a task
    #[serde(skip)]
    NextViewInterrupt(ViewNumber),
}

impl<STATE: State> ConsensusMessage<STATE> {
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
/// Messages related to sending data between nodes
pub enum DataMessage<STATE: State> {
    /// The newest entry that a node knows. This is send from existing nodes to a new node when the new node joins the network
    NewestQuorumCertificate {
        /// The newest [`QuorumCertificate`]
        #[serde(deserialize_with = "<QuorumCertificate<STATE> as Deserialize>::deserialize")]
        quorum_certificate: QuorumCertificate<STATE>,

        /// The relevant [`BlockContents`]
        ///
        /// [`BlockContents`]: ../traits/block_contents/trait.BlockContents.html
        #[serde(deserialize_with = "<STATE::BlockType as Deserialize>::deserialize")]
        block: STATE::BlockType,

        /// The relevant [`State`]
        ///
        /// [`State`]: ../traits/state/trait.State.html
        #[serde(deserialize_with = "<STATE as Deserialize>::deserialize")]
        state: STATE,

        /// The parent leaf's commitment
        #[serde(deserialize_with = "<Commitment<Leaf<STATE>>as Deserialize>::deserialize")]
        parent_commitment: Commitment<Leaf<STATE>>,

        /// Transactions rejected in this view
        #[serde(deserialize_with = "<Vec<TxnCommitment<STATE>>as Deserialize>::deserialize")]
        rejected: Vec<TxnCommitment<STATE>>,

        /// the proposer id for this leaf
        proposer_id: EncodedPublicKey,
    },

    /// Contains a transaction to be submitted
    SubmitTransaction(<STATE::BlockType as Block>::Transaction),
}

#[derive(Serialize, Deserialize, Clone, Debug, std::hash::Hash, PartialEq, Eq)]
/// Signals the start of a new view
pub struct TimedOut<STATE: State> {
    /// The current view
    pub current_view: ViewNumber,
    /// The justification qc for this view
    #[serde(deserialize_with = "<QuorumCertificate<STATE> as Deserialize>::deserialize")]
    pub justify_qc: QuorumCertificate<STATE>,
}

#[derive(Serialize, Deserialize, Clone, Debug, std::hash::Hash, PartialEq, Eq)]
/// Prepare qc from the leader
pub struct Proposal<STATE: State> {
    // NOTE: optimization could include view number to help look up parent leaf
    // could even do 16 bit numbers if we want
    /// The leaf being proposed (see pseudocode)
    #[serde(deserialize_with = "<ProposalLeaf<STATE> as Deserialize>::deserialize")]
    pub leaf: ProposalLeaf<STATE>,
    /// The proposal must be signed by the view leader
    pub signature: EncodedSignature,
}

/// A nodes vote on the prepare field.
#[derive(Serialize, Deserialize, Clone, custom_debug::Debug, std::hash::Hash, PartialEq, Eq)]
pub struct Vote<STATE: State> {
    /// hash of the block being proposed
    /// TODO delete this when we delete block hash from the QC
    #[debug(skip)]
    #[serde(deserialize_with = "<Commitment<STATE::BlockType> as Deserialize>::deserialize")]
    pub block_commitment: Commitment<STATE::BlockType>,
    /// TODO we should remove this
    /// this is correct, but highly inefficient
    /// we should check a cache, and if that fails request the qc
    #[serde(
        deserialize_with = "<Commitment<QuorumCertificate<STATE>> as Deserialize>::deserialize"
    )]
    pub justify_qc_commitment: Commitment<QuorumCertificate<STATE>>,
    /// The signature share associated with this vote
    /// TODO ct/vrf make ConsensusMessage generic over I instead of serializing to a Vec<u8>
    pub signature: (EncodedPublicKey, EncodedSignature),
    /// Hash of the item being voted on
    #[debug(skip)]
    #[serde(deserialize_with = "<Commitment<Leaf<STATE>> as Deserialize>::deserialize")]
    pub leaf_commitment: Commitment<Leaf<STATE>>,
    /// The view this vote was cast for
    pub current_view: ViewNumber,
    /// The vote token generated by this replica
    /// TODO ed Update to actual type once ConsensusMessage is generic over I
    pub vote_token: Vec<u8>,
}
