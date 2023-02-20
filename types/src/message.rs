//! Network message types
//!
//! This module contains types used to represent the various types of messages that
//! `HotShot` nodes can send among themselves.

use crate::{
    data::ProposalType,
    traits::{network::NetworkMsg, node_implementation::NodeType, signature_key::EncodedSignature},
    vote::VoteType,
};
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// Incoming message
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(bound(deserialize = ""))]
pub struct Message<TYPES: NodeType, PROPOSAL: ProposalType<NodeType = TYPES>, VOTE: VoteType<TYPES>>
{
    /// The sender of this message
    pub sender: TYPES::SignatureKey,

    /// The message kind
    pub kind: MessageKind<TYPES, PROPOSAL, VOTE>,
}

impl<TYPES: NodeType, PROPOSAL: ProposalType<NodeType = TYPES>, VOTE: VoteType<TYPES>> NetworkMsg
    for Message<TYPES, PROPOSAL, VOTE>
{
}

impl<TYPES: NodeType, PROPOSAL: ProposalType<NodeType = TYPES>, VOTE: VoteType<TYPES>>
    Message<TYPES, PROPOSAL, VOTE>
{
    /// get the view number out of a message
    pub fn get_view_number(&self) -> TYPES::Time {
        match &self.kind {
            MessageKind::Consensus(c) => match c {
                ConsensusMessage::Proposal(p) => p.data.get_view_number(),
                ConsensusMessage::Vote(v) => v.current_view(),
                ConsensusMessage::InternalTrigger(trigger) => match trigger {
                    InternalTrigger::Timeout(v) => *v,
                },
            },
            MessageKind::Data(DataMessage::SubmitTransaction(_, v)) => *v,
        }
    }
}

// TODO (da) make it more customized to the consensus layer, maybe separating the specific message
// data from the kind enum.
/// Enum representation of any message type
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(bound(deserialize = ""))]
pub enum MessageKind<
    TYPES: NodeType,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
> {
    /// Messages related to the consensus protocol
    Consensus(ConsensusMessage<TYPES, PROPOSAL, VOTE>),
    /// Messages relating to sharing data between nodes
    Data(DataMessage<TYPES>),
}

impl<TYPES: NodeType, PROPOSAL: ProposalType<NodeType = TYPES>, VOTE: VoteType<TYPES>>
    From<ConsensusMessage<TYPES, PROPOSAL, VOTE>> for MessageKind<TYPES, PROPOSAL, VOTE>
{
    fn from(m: ConsensusMessage<TYPES, PROPOSAL, VOTE>) -> Self {
        Self::Consensus(m)
    }
}

impl<TYPES: NodeType, PROPOSAL: ProposalType<NodeType = TYPES>, VOTE: VoteType<TYPES>>
    From<DataMessage<TYPES>> for MessageKind<TYPES, PROPOSAL, VOTE>
{
    fn from(m: DataMessage<TYPES>) -> Self {
        Self::Data(m)
    }
}

/// Internal triggers sent by consensus messages.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(bound(deserialize = ""))]
pub enum InternalTrigger<TYPES: NodeType> {
    // May add other triggers if necessary.
    /// Internal timeout at the specified view number.
    Timeout(TYPES::Time),
}

/// a processed consensus message
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(bound(deserialize = ""))]
pub enum ProcessedConsensusMessage<
    TYPES: NodeType,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
> {
    /// Leader's proposal
    Proposal(Proposal<PROPOSAL>, TYPES::SignatureKey),
    /// Replica's vote on a proposal.
    Vote(VOTE, TYPES::SignatureKey),
    /// Internal ONLY message indicating a view interrupt.
    #[serde(skip)]
    InternalTrigger(InternalTrigger<TYPES>),
}

impl<TYPES: NodeType, PROPOSAL: ProposalType<NodeType = TYPES>, VOTE: VoteType<TYPES>>
    From<ProcessedConsensusMessage<TYPES, PROPOSAL, VOTE>>
    for ConsensusMessage<TYPES, PROPOSAL, VOTE>
{
    /// row polymorphism would be great here
    fn from(value: ProcessedConsensusMessage<TYPES, PROPOSAL, VOTE>) -> Self {
        match value {
            ProcessedConsensusMessage::Proposal(p, _) => ConsensusMessage::Proposal(p),
            ProcessedConsensusMessage::Vote(v, _) => ConsensusMessage::Vote(v),
            ProcessedConsensusMessage::InternalTrigger(a) => ConsensusMessage::InternalTrigger(a),
        }
    }
}

impl<TYPES: NodeType, PROPOSAL: ProposalType<NodeType = TYPES>, VOTE: VoteType<TYPES>>
    ProcessedConsensusMessage<TYPES, PROPOSAL, VOTE>
{
    /// row polymorphism would be great here
    pub fn new(
        value: ConsensusMessage<TYPES, PROPOSAL, VOTE>,
        sender: TYPES::SignatureKey,
    ) -> Self {
        match value {
            ConsensusMessage::Proposal(p) => ProcessedConsensusMessage::Proposal(p, sender),
            ConsensusMessage::Vote(v) => ProcessedConsensusMessage::Vote(v, sender),
            ConsensusMessage::InternalTrigger(a) => ProcessedConsensusMessage::InternalTrigger(a),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(bound(deserialize = ""))]
/// Messages related to the consensus protocol
pub enum ConsensusMessage<
    TYPES: NodeType,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
> {
    /// Leader's proposal
    Proposal(Proposal<PROPOSAL>),
    /// Replica's vote on a proposal.
    Vote(VOTE),
    /// Internal ONLY message indicating a view interrupt.
    #[serde(skip)]
    InternalTrigger(InternalTrigger<TYPES>),
}

impl<TYPES: NodeType, PROPOSAL: ProposalType<NodeType = TYPES>, VOTE: VoteType<TYPES>>
    ConsensusMessage<TYPES, PROPOSAL, VOTE>
{
    /// The view number of the (leader|replica) when the message was sent
    /// or the view of the timeout
    pub fn view_number(&self) -> TYPES::Time {
        match self {
            ConsensusMessage::Proposal(p) => {
                // view of leader in the leaf when proposal
                // this should match replica upon receipt
                p.data.get_view_number()
            }
            ConsensusMessage::Vote(vote_message) => vote_message.current_view(),
            ConsensusMessage::InternalTrigger(trigger) => match trigger {
                InternalTrigger::Timeout(time) => *time,
            },
        }
    }
}

#[derive(Serialize, Deserialize, Derivative, Clone, Debug, PartialEq, Eq)]
#[serde(bound(deserialize = ""))]
/// Messages related to sending data between nodes
pub enum DataMessage<TYPES: NodeType> {
    /// Contains a transaction to be submitted
    /// TODO rethink this when we start to send these messages
    /// we only need the view number for broadcast
    SubmitTransaction(TYPES::Transaction, TYPES::Time),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(bound(deserialize = ""))]
/// Prepare qc from the leader
pub struct Proposal<PROPOSAL: ProposalType> {
    // NOTE: optimization could include view number to help look up parent leaf
    // could even do 16 bit numbers if we want
    /// The data being proposed.
    pub data: PROPOSAL,
    /// The proposal must be signed by the view leader
    pub signature: EncodedSignature,
}
