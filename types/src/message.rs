//! Network message types
//!
//! This module contains types used to represent the various types of messages that
//! `HotShot` nodes can send among themselves.

use crate::traits::consensus_type::sequencing_consensus::SequencingConsensusType;
use crate::traits::network::ViewMessage;
use crate::{
    data::ProposalType,
    traits::{
        consensus_type::validating_consensus::ValidatingConsensusType,
        network::NetworkMsg,
        node_implementation::{
            CommitteeVote, DAProposalType, NodeImplementation, NodeType, QuorumProposalType,
            QuorumVoteType,
        },
        signature_key::EncodedSignature,
    },
    vote::VoteType,
};
use derivative::Derivative;
use either::Either::{self, Left, Right};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// Incoming message
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""))]
pub struct Message<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    CONSENSUSMESSAGE: ConsensusMessageType<TYPES, I>,
> {
    /// The sender of this message
    pub sender: TYPES::SignatureKey,

    /// The message kind
    pub kind: MessageKind<TYPES, I, CONSENSUSMESSAGE>,
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES>,
        CONSENSUSMESSAGE: ConsensusMessageType<TYPES, I>,
    > NetworkMsg for Message<TYPES, I, CONSENSUSMESSAGE>
{
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES>,
        CONSENSUS: ValidatingConsensusType,
        CONSENSUSMESSAGE: ValidatingMessageType<CONSENSUS, TYPES, I>,
    > ViewMessage<TYPES> for Message<TYPES, I, CONSENSUSMESSAGE>
{
    /// get the view number out of a message
    fn get_view_number(&self) -> TYPES::Time {
        match &self.kind {
            MessageKind::Consensus(message) => match message {
                GeneralConsensusMessage::Proposal(p) => p.data.get_view_number(),
                GeneralConsensusMessage::Vote(v) => v.current_view(),
                GeneralConsensusMessage::InternalTrigger(trigger) => match trigger {
                    InternalTrigger::Timeout(v) => *v,
                },
            },
            MessageKind::Data(DataMessage::SubmitTransaction(_, v)) => *v,
        }
    }
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES>,
        CONSENSUS: SequencingConsensusType,
        CONSENSUSMESSAGE: SequencingMessageType<CONSENSUS, TYPES, I>,
    > ViewMessage<TYPES> for Message<TYPES, I, CONSENSUSMESSAGE>
{
    /// get the view number out of a message
    fn get_view_number(&self) -> TYPES::Time {
        match &self.kind {
            MessageKind::Consensus(message) => match message {
                Left(general_message) => match general_message {
                    GeneralConsensusMessage::Proposal(p) => p.data.get_view_number(),
                    GeneralConsensusMessage::Vote(v) => v.current_view(),
                    GeneralConsensusMessage::InternalTrigger(trigger) => match trigger {
                        InternalTrigger::Timeout(v) => *v,
                    },
                },
                Right(committee_message) => match committee_message {
                    CommitteeConsensusMessage::DAProposal(p) => p.data.get_view_number(),
                    CommitteeConsensusMessage::DAVote(v) => v.current_view(),
                },
            },
            MessageKind::Data(DataMessage::SubmitTransaction(_, v)) => *v,
        }
    }
}

// TODO (da) make it more customized to the consensus layer, maybe separating the specific message
// data from the kind enum.
/// Enum representation of any message type
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""))]
pub enum MessageKind<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    CONSENSUSMESSAGE: ConsensusMessageType<TYPES, I>,
> {
    /// Messages related to the consensus protocol
    Consensus(CONSENSUSMESSAGE),
    /// Messages relating to sharing data between nodes
    Data(DataMessage<TYPES>),
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES>,
        CONSENSUSMESSAGE: ConsensusMessageType<TYPES, I>,
    > From<CONSENSUSMESSAGE> for MessageKind<TYPES, I, CONSENSUSMESSAGE>
{
    fn from(m: CONSENSUSMESSAGE) -> Self {
        Self::Consensus(m)
    }
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>, CONSENSUSMESSAGE> From<DataMessage<TYPES>>
    for MessageKind<TYPES, I, CONSENSUSMESSAGE>
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

/// A processed consensus message for both validating and sequencing consensus.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]
pub enum ProcessedGeneralConsensusMessage<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Leader's proposal for full Quorom voting
    Proposal(Proposal<QuorumProposalType<TYPES, I>>, TYPES::SignatureKey),
    /// Replica's vote on a proposal.
    Vote(QuorumVoteType<TYPES, I>, TYPES::SignatureKey),
    /// Internal ONLY message indicating a view interrupt.
    #[serde(skip)]
    InternalTrigger(InternalTrigger<TYPES>),
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> From<ProcessedGeneralConsensusMessage<TYPES, I>>
    for GeneralConsensusMessage<TYPES, I>
{
    /// row polymorphism would be great here
    fn from(value: ProcessedGeneralConsensusMessage<TYPES, I>) -> Self {
        match value {
            ProcessedGeneralConsensusMessage::Proposal(p, _) => {
                GeneralConsensusMessage::Proposal(p)
            }
            ProcessedGeneralConsensusMessage::Vote(v, _) => GeneralConsensusMessage::Vote(v),
            ProcessedGeneralConsensusMessage::InternalTrigger(a) => {
                GeneralConsensusMessage::InternalTrigger(a)
            }
        }
    }
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> ProcessedGeneralConsensusMessage<TYPES, I> {
    /// row polymorphism would be great here
    pub fn new(value: GeneralConsensusMessage<TYPES, I>, sender: TYPES::SignatureKey) -> Self {
        match value {
            GeneralConsensusMessage::Proposal(p) => {
                ProcessedGeneralConsensusMessage::Proposal(p, sender)
            }
            GeneralConsensusMessage::Vote(v) => ProcessedGeneralConsensusMessage::Vote(v, sender),
            GeneralConsensusMessage::InternalTrigger(a) => {
                ProcessedGeneralConsensusMessage::InternalTrigger(a)
            }
        }
    }
}

/// A processed consensus message for the DA committee in sequencing consensus.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]
pub enum ProcessedCommitteeConsensusMessage<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Proposal for data availability committee
    DAProposal(Proposal<DAProposalType<TYPES, I>>, TYPES::SignatureKey),
    /// vote from the DA committee
    DAVote(CommitteeVote<TYPES, I>, TYPES::SignatureKey),
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>>
    From<ProcessedCommitteeConsensusMessage<TYPES, I>> for CommitteeConsensusMessage<TYPES, I>
{
    /// row polymorphism would be great here
    fn from(value: ProcessedCommitteeConsensusMessage<TYPES, I>) -> Self {
        match value {
            ProcessedCommitteeConsensusMessage::DAProposal(p, _) => {
                CommitteeConsensusMessage::DAProposal(p)
            }
            ProcessedCommitteeConsensusMessage::DAVote(v, _) => {
                CommitteeConsensusMessage::DAVote(v)
            }
        }
    }
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> ProcessedCommitteeConsensusMessage<TYPES, I> {
    /// row polymorphism would be great here
    pub fn new(value: CommitteeConsensusMessage<TYPES, I>, sender: TYPES::SignatureKey) -> Self {
        match value {
            CommitteeConsensusMessage::DAProposal(p) => {
                ProcessedCommitteeConsensusMessage::DAProposal(p, sender)
            }
            CommitteeConsensusMessage::DAVote(v) => {
                ProcessedCommitteeConsensusMessage::DAVote(v, sender)
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""))]
/// Messages related to both validating and sequencing consensus.
pub enum GeneralConsensusMessage<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Leader's proposal for full quorum voting
    Proposal(Proposal<QuorumProposalType<TYPES, I>>),

    /// Replica's vote on a proposal.
    Vote(QuorumVoteType<TYPES, I>),

    /// Internal ONLY message indicating a view interrupt.
    #[serde(skip)]
    InternalTrigger(InternalTrigger<TYPES>),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""))]
/// Messages related to the sequencing consensus protocol for the DA committee.
pub enum CommitteeConsensusMessage<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Proposal for data availability committee
    DAProposal(Proposal<DAProposalType<TYPES, I>>),

    /// vote for data availability committee
    DAVote(CommitteeVote<TYPES, I>),
}

/// Messages related to the consensus protocol.
pub trait ConsensusMessageType<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    type GeneralConsensusMessage;
}

/// Messages related to the validating consensus protocol.
pub trait ValidatingMessageType<
    CONSENSUS: ValidatingConsensusType,
    TYPES: NodeType<ConsensusType = CONSENSUS>,
    I: NodeImplementation<TYPES>,
>: ConsensusMessageType<TYPES, I>
{
}

/// Messages related to the sequencing consensus protocol.
pub trait SequencingMessageType<
    CONSENSUS: SequencingConsensusType,
    TYPES: NodeType<ConsensusType = CONSENSUS>,
    I: NodeImplementation<TYPES>,
>: ConsensusMessageType<TYPES, I>
{
    type CommitteeConsensusMessage;
}

pub struct ValidatingMessage<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    GeneralConsensusMessage<TYPES, I>,
);

impl<
        CONSENSUS: ValidatingConsensusType,
        TYPES: NodeType<ConsensusType = CONSENSUS>,
        I: NodeImplementation<TYPES>,
    > ConsensusMessageType<TYPES, I> for ValidatingMessage<TYPES, I>
{
    type GeneralConsensusMessage = GeneralConsensusMessage<TYPES, I>;
}

impl<
        CONSENSUS: ValidatingConsensusType,
        TYPES: NodeType<ConsensusType = CONSENSUS>,
        I: NodeImplementation<TYPES>,
    > ValidatingMessageType<CONSENSUS, TYPES, I> for ValidatingMessage<TYPES, I>
{
}

impl<
        CONSENSUS: ValidatingConsensusType,
        TYPES: NodeType<ConsensusType = CONSENSUS>,
        I: NodeImplementation<TYPES>,
    > ValidatingMessage<TYPES, I>
{
    /// The view number of the (leader|replica) when the message was sent or the view of the
    /// timeout.
    pub fn view_number(&self) -> TYPES::Time {
        match self.0 {
            GeneralConsensusMessage::Proposal(p) => {
                // view of leader in the leaf when proposal
                // this should match replica upon receipt
                p.data.get_view_number()
            }
            GeneralConsensusMessage::Vote(vote_message) => vote_message.current_view(),
            GeneralConsensusMessage::InternalTrigger(trigger) => match trigger {
                InternalTrigger::Timeout(time) => *time,
            },
        }
    }
}

pub struct SequencingMessage<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    Either<GeneralConsensusMessage<TYPES, I>, CommitteeConsensusMessage<TYPES, I>>,
);

impl<
        CONSENSUS: SequencingConsensusType,
        TYPES: NodeType<ConsensusType = CONSENSUS>,
        I: NodeImplementation<TYPES>,
    > ConsensusMessageType<TYPES, I> for SequencingMessage<TYPES, I>
{
    type GeneralConsensusMessage = GeneralConsensusMessage<TYPES, I>;
}

impl<
        CONSENSUS: SequencingConsensusType,
        TYPES: NodeType<ConsensusType = CONSENSUS>,
        I: NodeImplementation<TYPES>,
    > SequencingMessageType<CONSENSUS, TYPES, I> for SequencingMessage<TYPES, I>
{
    type CommitteeConsensusMessage = CommitteeConsensusMessage<TYPES, I>;
}

impl<
        CONSENSUS: SequencingConsensusType,
        TYPES: NodeType<ConsensusType = CONSENSUS>,
        I: NodeImplementation<TYPES>,
    > SequencingMessage<TYPES, I>
{
    /// The view number of the (leader|replica|committee member) when the message was sent or the
    /// view of the timeout.
    pub fn view_number(&self) -> TYPES::Time {
        match self.0 {
            Left(general_message) => {
                match general_message {
                    GeneralConsensusMessage::Proposal(p) => {
                        // view of leader in the leaf when proposal
                        // this should match replica upon receipt
                        p.data.get_view_number()
                    }
                    GeneralConsensusMessage::Vote(vote_message) => vote_message.current_view(),
                    GeneralConsensusMessage::InternalTrigger(trigger) => match trigger {
                        InternalTrigger::Timeout(time) => *time,
                    },
                }
            }
            Right(committee_message) => {
                match committee_message {
                    CommitteeConsensusMessage::DAProposal(p) => {
                        // view of leader in the leaf when proposal
                        // this should match replica upon receipt
                        p.data.get_view_number()
                    }
                    CommitteeConsensusMessage::DAVote(vote_message) => vote_message.current_view(),
                }
            }
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
