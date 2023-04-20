//! Network message types
//!
//! This module contains types used to represent the various types of messages that
//! `HotShot` nodes can send among themselves.

use crate::traits::network::ViewMessage;
use crate::{
    data::ProposalType,
    traits::{
        network::NetworkMsg,
        node_implementation::{
            CommitteeProposal, CommitteeVote, NodeImplementation, NodeType, QuorumProposal,
            QuorumVoteType,
        },
        signature_key::EncodedSignature,
    },
    vote::VoteType,
};
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// Incoming message
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""))]
pub struct Message<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// The sender of this message
    pub sender: TYPES::SignatureKey,

    /// The message kind
    pub kind: MessageKind<TYPES, I>,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> NetworkMsg for Message<TYPES, I> {}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> ViewMessage<TYPES> for Message<TYPES, I> {
    /// get the view number out of a message
    fn get_view_number(&self) -> TYPES::Time {
        match &self.kind {
            MessageKind::Consensus(c) => match c {
                ConsensusMessage::Proposal(p) => p.data.get_view_number(),
                ConsensusMessage::DAProposal(p) => p.data.get_view_number(),
                ConsensusMessage::Vote(v) => v.current_view(),
                ConsensusMessage::DAVote(v) => v.current_view(),
                ConsensusMessage::InternalTrigger(trigger) => match trigger {
                    InternalTrigger::Timeout(v) => *v,
                },
            },
            MessageKind::Data(DataMessage::SubmitTransaction(_, v)) => *v,
        }
    }
    fn purpose(&self) -> MessagePurpose {
        match &self.kind {
            MessageKind::Consensus(message_kind) => match message_kind {
                ConsensusMessage::Proposal(_) | ConsensusMessage::DAProposal(_) => {
                    MessagePurpose::Proposal
                }
                ConsensusMessage::Vote(_) | ConsensusMessage::DAVote(_) => MessagePurpose::Vote,
                ConsensusMessage::InternalTrigger(_) => MessagePurpose::Internal,
            },
            MessageKind::Data(message_kind) => match message_kind {
                DataMessage::SubmitTransaction(_, _) => MessagePurpose::Data,
            },
        }
    }
}

pub enum MessagePurpose {
    Proposal,
    Vote,
    Internal,
    Data,
}

// TODO (da) make it more customized to the consensus layer, maybe separating the specific message
// data from the kind enum.
/// Enum representation of any message type
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""))]
pub enum MessageKind<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Messages related to the consensus protocol
    Consensus(ConsensusMessage<TYPES, I>),
    /// Messages relating to sharing data between nodes
    Data(DataMessage<TYPES>),
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> From<ConsensusMessage<TYPES, I>>
    for MessageKind<TYPES, I>
{
    fn from(m: ConsensusMessage<TYPES, I>) -> Self {
        Self::Consensus(m)
    }
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> From<DataMessage<TYPES>>
    for MessageKind<TYPES, I>
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
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]
pub enum ProcessedConsensusMessage<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Leader's proposal for full Quorom voting
    Proposal(Proposal<QuorumProposal<TYPES, I>>, TYPES::SignatureKey),
    /// Proposal for data availability committee
    DAProposal(Proposal<CommitteeProposal<TYPES, I>>, TYPES::SignatureKey),
    /// Replica's vote on a proposal.
    Vote(QuorumVoteType<TYPES, I>, TYPES::SignatureKey),
    /// vote from the DA committee
    DAVote(CommitteeVote<TYPES, I>, TYPES::SignatureKey),
    /// Internal ONLY message indicating a view interrupt.
    #[serde(skip)]
    InternalTrigger(InternalTrigger<TYPES>),
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> From<ProcessedConsensusMessage<TYPES, I>>
    for ConsensusMessage<TYPES, I>
{
    /// row polymorphism would be great here
    fn from(value: ProcessedConsensusMessage<TYPES, I>) -> Self {
        match value {
            ProcessedConsensusMessage::Proposal(p, _) => ConsensusMessage::Proposal(p),
            ProcessedConsensusMessage::DAProposal(p, _) => ConsensusMessage::DAProposal(p),
            ProcessedConsensusMessage::Vote(v, _) => ConsensusMessage::Vote(v),
            ProcessedConsensusMessage::DAVote(v, _) => ConsensusMessage::DAVote(v),
            ProcessedConsensusMessage::InternalTrigger(a) => ConsensusMessage::InternalTrigger(a),
        }
    }
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> ProcessedConsensusMessage<TYPES, I> {
    /// row polymorphism would be great here
    pub fn new(value: ConsensusMessage<TYPES, I>, sender: TYPES::SignatureKey) -> Self {
        match value {
            ConsensusMessage::Proposal(p) => ProcessedConsensusMessage::Proposal(p, sender),
            ConsensusMessage::DAProposal(p) => ProcessedConsensusMessage::DAProposal(p, sender),
            ConsensusMessage::Vote(v) => ProcessedConsensusMessage::Vote(v, sender),
            ConsensusMessage::DAVote(v) => ProcessedConsensusMessage::DAVote(v, sender),
            ConsensusMessage::InternalTrigger(a) => ProcessedConsensusMessage::InternalTrigger(a),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""))]
/// Messages related to the consensus protocol
pub enum ConsensusMessage<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    // PROPOSAL: ProposalType<NodeType = TYPES>,
    // VOTE: VoteType<TYPES>,
> {
    /// Leader's proposal for full quorum voting
    Proposal(Proposal<QuorumProposal<TYPES, I>>),

    /// Proposal for data availability committee
    DAProposal(Proposal<CommitteeProposal<TYPES, I>>),

    /// Replica's vote on a proposal.
    Vote(QuorumVoteType<TYPES, I>),

    /// vote for data availability committee
    DAVote(CommitteeVote<TYPES, I>),

    /// Internal ONLY message indicating a view interrupt.
    #[serde(skip)]
    InternalTrigger(InternalTrigger<TYPES>),
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> ConsensusMessage<TYPES, I> {
    /// The view number of the (leader|replica) when the message was sent
    /// or the view of the timeout
    pub fn view_number(&self) -> TYPES::Time {
        match self {
            ConsensusMessage::Proposal(p) => {
                // view of leader in the leaf when proposal
                // this should match replica upon receipt
                p.data.get_view_number()
            }
            ConsensusMessage::DAProposal(p) => {
                // view of leader in the leaf when proposal
                // this should match replica upon receipt
                p.data.get_view_number()
            }
            ConsensusMessage::Vote(vote_message) => vote_message.current_view(),
            ConsensusMessage::DAVote(vote_message) => vote_message.current_view(),
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
