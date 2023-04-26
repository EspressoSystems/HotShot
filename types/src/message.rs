//! Network message types
//!
//! This module contains types used to represent the various types of messages that
//! `HotShot` nodes can send among themselves.

use crate::data::DAProposal;
use crate::traits::consensus_type::validating_consensus::ValidatingConsensus;
use crate::traits::network::ViewMessage;
use crate::vote::{DAVote, QuorumVote};
use crate::{
    data::{ProposalType, ValidatingLeaf},
    traits::{
        consensus_type::sequencing_consensus::SequencingConsensus,
        network::NetworkMsg,
        node_implementation::{
            DAProposalType, ExchangesType, NodeImplementation, NodeType, QuorumProposalType,
            SequencingExchangesType, ValidatingExchangesType,
        },
        signature_key::EncodedSignature,
    },
    vote::VoteType,
};
use derivative::Derivative;
use either::Either::{self, Left, Right};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::marker::PhantomData;

/// Incoming message
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""))]
pub struct Message<
    TYPES: NodeType,
    I: NodeImplementation<TYPES, ConsensusMessage = CONSENSUSMESSAGE>,
    CONSENSUSMESSAGE: ConsensusMessageType<TYPES, I>
        + Clone
        + Debug
        + Send
        + Sync
        + 'static
        + for<'a> Deserialize<'a>
        + Serialize,
> {
    /// The sender of this message
    pub sender: TYPES::SignatureKey,

    /// The message kind
    pub kind: MessageKind<TYPES, I, CONSENSUSMESSAGE>,

    /// Phantom data.
    pub _phantom: PhantomData<I>,
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES, ConsensusMessage = CONSENSUSMESSAGE>,
        CONSENSUSMESSAGE: ConsensusMessageType<TYPES, I>
            + Clone
            + Debug
            + Send
            + Sync
            + 'static
            + for<'a> Deserialize<'a>
            + Serialize,
    > NetworkMsg for Message<TYPES, I, CONSENSUSMESSAGE>
{
}

impl<
        TYPES: NodeType<ConsensusType = ValidatingConsensus>,
        I: NodeImplementation<
            TYPES,
            Leaf = ValidatingLeaf<TYPES>,
            ConsensusMessage = ValidatingMessage<TYPES, I>,
        >,
    > ViewMessage<TYPES> for Message<TYPES, I, ValidatingMessage<TYPES, I>>
where
    I::Exchanges:
        ValidatingExchangesType<TYPES, I::Leaf, Message<TYPES, I, ValidatingMessage<TYPES, I>>>,
{
    /// get the view number out of a message
    fn get_view_number(&self) -> TYPES::Time {
        match &self.kind {
            MessageKind::Consensus(message) => match &message.0 {
                GeneralConsensusMessage::Proposal(p) => p.data.get_view_number(),
                GeneralConsensusMessage::Vote(v) => v.current_view(),
                GeneralConsensusMessage::InternalTrigger(trigger) => match trigger {
                    InternalTrigger::Timeout(v) => *v,
                },
            },
            MessageKind::Data(DataMessage::SubmitTransaction(_, v)) => *v,
            MessageKind::_Unreachable(_) => unimplemented!(),
        }
    }
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
    > ViewMessage<TYPES> for Message<TYPES, I, SequencingMessage<TYPES, I>>
where
    I::Exchanges:
        SequencingExchangesType<TYPES, I::Leaf, Message<TYPES, I, SequencingMessage<TYPES, I>>>,
{
    /// get the view number out of a message
    fn get_view_number(&self) -> TYPES::Time {
        match &self.kind {
            MessageKind::Consensus(message) => match &message.0 {
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
            MessageKind::_Unreachable(_) => unimplemented!(),
        }
    }
}

/// Enum representation of any message type
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""))]
pub enum MessageKind<
    TYPES: NodeType,
    I: NodeImplementation<TYPES, ConsensusMessage = CONSENSUSMESSAGE>,
    CONSENSUSMESSAGE: ConsensusMessageType<TYPES, I> + for<'a> Deserialize<'a> + Serialize,
> {
    /// Messages related to the consensus protocol
    Consensus(CONSENSUSMESSAGE),
    /// Messages relating to sharing data between nodes
    Data(DataMessage<TYPES>),
    /// Phantom data.
    _Unreachable(PhantomData<I>),
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES, ConsensusMessage = CONSENSUSMESSAGE>,
        CONSENSUSMESSAGE: ConsensusMessageType<TYPES, I> + for<'a> Deserialize<'a> + Serialize,
    > MessageKind<TYPES, I, CONSENSUSMESSAGE>
{
    // Can't implement `From<CONSENSUSMESSAGE>` directly due to potential conflict with
    // `From<DataMessage>`.
    pub fn from_consensus_message(m: CONSENSUSMESSAGE) -> Self {
        Self::Consensus(m)
    }
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES, ConsensusMessage = CONSENSUSMESSAGE>,
        CONSENSUSMESSAGE: ConsensusMessageType<TYPES, I> + for<'a> Deserialize<'a> + Serialize,
    > From<DataMessage<TYPES>> for MessageKind<TYPES, I, CONSENSUSMESSAGE>
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
pub enum ProcessedGeneralConsensusMessage<
    TYPES: NodeType,
    I: NodeImplementation<TYPES, ConsensusMessage = CONSENSUSMESSAGE>,
    CONSENSUSMESSAGE: ConsensusMessageType<TYPES, I>
        + Clone
        + Debug
        + Send
        + Sync
        + 'static
        + for<'a> Deserialize<'a>
        + Serialize,
> where
    I::Exchanges:
        ExchangesType<TYPES::ConsensusType, TYPES, I::Leaf, Message<TYPES, I, CONSENSUSMESSAGE>>,
{
    /// Leader's proposal for full Quorom voting
    Proposal(
        Proposal<QuorumProposalType<TYPES, I, CONSENSUSMESSAGE>>,
        TYPES::SignatureKey,
    ),
    /// Replica's vote on a proposal.
    Vote(QuorumVote<TYPES, I::Leaf>, TYPES::SignatureKey),
    /// Internal ONLY message indicating a view interrupt.
    #[serde(skip)]
    InternalTrigger(InternalTrigger<TYPES>),
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES, ConsensusMessage = CONSENSUSMESSAGE>,
        CONSENSUSMESSAGE: ConsensusMessageType<TYPES, I>
            + Clone
            + Debug
            + Send
            + Sync
            + 'static
            + for<'a> Deserialize<'a>
            + Serialize,
    > From<ProcessedGeneralConsensusMessage<TYPES, I, CONSENSUSMESSAGE>>
    for GeneralConsensusMessage<TYPES, I, CONSENSUSMESSAGE>
where
    I::Exchanges:
        ExchangesType<TYPES::ConsensusType, TYPES, I::Leaf, Message<TYPES, I, CONSENSUSMESSAGE>>,
{
    /// row polymorphism would be great here
    fn from(value: ProcessedGeneralConsensusMessage<TYPES, I, CONSENSUSMESSAGE>) -> Self {
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

impl<
        TYPES: NodeType<ConsensusType = ValidatingConsensus>,
        I: NodeImplementation<
            TYPES,
            Leaf = ValidatingLeaf<TYPES>,
            ConsensusMessage = ValidatingMessage<TYPES, I>,
        >,
    > From<ProcessedGeneralConsensusMessage<TYPES, I, ValidatingMessage<TYPES, I>>>
    for ValidatingMessage<TYPES, I>
where
    I::Exchanges:
        ValidatingExchangesType<TYPES, I::Leaf, Message<TYPES, I, ValidatingMessage<TYPES, I>>>,
{
    /// row polymorphism would be great here
    fn from(
        value: ProcessedGeneralConsensusMessage<TYPES, I, ValidatingMessage<TYPES, I>>,
    ) -> Self {
        match value {
            ProcessedGeneralConsensusMessage::Proposal(p, _) => {
                ValidatingMessage(GeneralConsensusMessage::Proposal(p))
            }
            ProcessedGeneralConsensusMessage::Vote(v, _) => {
                ValidatingMessage(GeneralConsensusMessage::Vote(v))
            }
            ProcessedGeneralConsensusMessage::InternalTrigger(a) => {
                ValidatingMessage(GeneralConsensusMessage::InternalTrigger(a))
            }
        }
    }
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES, ConsensusMessage = CONSENSUSMESSAGE>,
        CONSENSUSMESSAGE: ConsensusMessageType<TYPES, I>
            + Clone
            + Debug
            + Send
            + Sync
            + 'static
            + for<'a> Deserialize<'a>
            + Serialize,
    > ProcessedGeneralConsensusMessage<TYPES, I, CONSENSUSMESSAGE>
where
    I::Exchanges:
        ExchangesType<TYPES::ConsensusType, TYPES, I::Leaf, Message<TYPES, I, CONSENSUSMESSAGE>>,
{
    /// row polymorphism would be great here
    pub fn new(
        value: GeneralConsensusMessage<TYPES, I, CONSENSUSMESSAGE>,
        sender: TYPES::SignatureKey,
    ) -> Self {
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
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]
pub enum ProcessedCommitteeConsensusMessage<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
> where
    I::Exchanges:
        SequencingExchangesType<TYPES, I::Leaf, Message<TYPES, I, SequencingMessage<TYPES, I>>>,
{
    /// Proposal for data availability committee
    DAProposal(Proposal<DAProposal<TYPES>>, TYPES::SignatureKey),
    /// vote from the DA committee
    DAVote(DAVote<TYPES, I::Leaf>, TYPES::SignatureKey),
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
    > From<ProcessedCommitteeConsensusMessage<TYPES, I>> for CommitteeConsensusMessage<TYPES, I>
where
    I::Exchanges:
        SequencingExchangesType<TYPES, I::Leaf, Message<TYPES, I, SequencingMessage<TYPES, I>>>,
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

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
    > ProcessedCommitteeConsensusMessage<TYPES, I>
where
    I::Exchanges:
        SequencingExchangesType<TYPES, I::Leaf, Message<TYPES, I, SequencingMessage<TYPES, I>>>,
{
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

pub type ProcessedSequencingMessage<TYPES, I, CONSENSUSMESSAGE> = Either<
    ProcessedGeneralConsensusMessage<TYPES, I, CONSENSUSMESSAGE>,
    ProcessedCommitteeConsensusMessage<TYPES, I>,
>;

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
    > From<ProcessedSequencingMessage<TYPES, I, SequencingMessage<TYPES, I>>>
    for SequencingMessage<TYPES, I>
where
    I::Exchanges:
        SequencingExchangesType<TYPES, I::Leaf, Message<TYPES, I, SequencingMessage<TYPES, I>>>,
{
    /// row polymorphism would be great here
    fn from(value: ProcessedSequencingMessage<TYPES, I, SequencingMessage<TYPES, I>>) -> Self {
        match value {
            Left(message) => SequencingMessage(Left(message.into())),
            Right(message) => SequencingMessage(Right(message.into())),
        }
    }
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
    > From<ProcessedGeneralConsensusMessage<TYPES, I, SequencingMessage<TYPES, I>>>
    for ProcessedSequencingMessage<TYPES, I, SequencingMessage<TYPES, I>>
where
    I::Exchanges:
        SequencingExchangesType<TYPES, I::Leaf, Message<TYPES, I, SequencingMessage<TYPES, I>>>,
{
    /// row polymorphism would be great here
    fn from(
        value: ProcessedGeneralConsensusMessage<TYPES, I, SequencingMessage<TYPES, I>>,
    ) -> Self {
        Left(value)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""))]
/// Messages related to both validating and sequencing consensus.
pub enum GeneralConsensusMessage<
    TYPES: NodeType,
    I: NodeImplementation<TYPES, ConsensusMessage = CONSENSUSMESSAGE>,
    CONSENSUSMESSAGE: ConsensusMessageType<TYPES, I>
        + Clone
        + Debug
        + Send
        + Sync
        + 'static
        + for<'a> Deserialize<'a>
        + Serialize,
> where
    I::Exchanges:
        ExchangesType<TYPES::ConsensusType, TYPES, I::Leaf, Message<TYPES, I, CONSENSUSMESSAGE>>,
{
    /// Leader's proposal for full quorum voting
    Proposal(Proposal<QuorumProposalType<TYPES, I, CONSENSUSMESSAGE>>),

    /// Replica's vote on a proposal.
    Vote(QuorumVote<TYPES, I::Leaf>),

    /// Internal ONLY message indicating a view interrupt.
    #[serde(skip)]
    InternalTrigger(InternalTrigger<TYPES>),
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""))]
/// Messages related to the sequencing consensus protocol for the DA committee.
pub enum CommitteeConsensusMessage<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
> where
    I::Exchanges:
        SequencingExchangesType<TYPES, I::Leaf, Message<TYPES, I, SequencingMessage<TYPES, I>>>,
{
    /// Proposal for data availability committee
    DAProposal(Proposal<DAProposal<TYPES>>),

    /// vote for data availability committee
    DAVote(DAVote<TYPES, I::Leaf>),
}

/// Messages related to the consensus protocol.
pub trait ConsensusMessageType<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Messages for both validating and sequencing consensus.
    type GeneralConsensusMessage;

    type ProcessedConsensusMessage: Send;
}

/// Messages related to the validating consensus protocol.
pub trait ValidatingMessageType<
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    I: NodeImplementation<TYPES>,
>: ConsensusMessageType<TYPES, I>
{
}

/// Messages related to the sequencing consensus protocol.
pub trait SequencingMessageType<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<TYPES>,
>: ConsensusMessageType<TYPES, I>
{
    /// Messages for DA committee only.
    type CommitteeConsensusMessage;
}

/// Messages for validating consensus.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(bound(deserialize = "", serialize = ""))]
pub struct ValidatingMessage<
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    I: NodeImplementation<
        TYPES,
        Leaf = ValidatingLeaf<TYPES>,
        ConsensusMessage = ValidatingMessage<TYPES, I>,
    >,
>(pub GeneralConsensusMessage<TYPES, I, ValidatingMessage<TYPES, I>>)
where
    I::Exchanges: ValidatingExchangesType<
        TYPES,
        ValidatingLeaf<TYPES>,
        Message<TYPES, I, ValidatingMessage<TYPES, I>>,
    >;

impl<
        TYPES: NodeType<ConsensusType = ValidatingConsensus>,
        I: NodeImplementation<
            TYPES,
            Leaf = ValidatingLeaf<TYPES>,
            ConsensusMessage = ValidatingMessage<TYPES, I>,
        >,
    > ConsensusMessageType<TYPES, I> for ValidatingMessage<TYPES, I>
where
    I::Exchanges:
        ValidatingExchangesType<TYPES, I::Leaf, Message<TYPES, I, ValidatingMessage<TYPES, I>>>,
{
    type GeneralConsensusMessage = GeneralConsensusMessage<TYPES, I, Self>;
    type ProcessedConsensusMessage = ProcessedGeneralConsensusMessage<TYPES, I, Self>;
}

impl<
        TYPES: NodeType<ConsensusType = ValidatingConsensus>,
        I: NodeImplementation<
            TYPES,
            Leaf = ValidatingLeaf<TYPES>,
            ConsensusMessage = ValidatingMessage<TYPES, I>,
        >,
    > ValidatingMessageType<TYPES, I> for ValidatingMessage<TYPES, I>
where
    I::Exchanges:
        ValidatingExchangesType<TYPES, I::Leaf, Message<TYPES, I, ValidatingMessage<TYPES, I>>>,
{
}

impl<
        TYPES: NodeType<ConsensusType = ValidatingConsensus>,
        I: NodeImplementation<
            TYPES,
            Leaf = ValidatingLeaf<TYPES>,
            ConsensusMessage = ValidatingMessage<TYPES, I>,
        >,
    > ValidatingMessage<TYPES, I>
where
    I::Exchanges:
        ValidatingExchangesType<TYPES, I::Leaf, Message<TYPES, I, ValidatingMessage<TYPES, I>>>,
{
    /// The view number of the (leader|replica) when the message was sent or the view of the
    /// timeout.
    pub fn view_number(&self) -> TYPES::Time {
        match &self.0 {
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

/// Messages for sequencing consensus.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(bound(deserialize = "", serialize = ""))]
pub struct SequencingMessage<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
>(
    pub  Either<
        GeneralConsensusMessage<TYPES, I, SequencingMessage<TYPES, I>>,
        CommitteeConsensusMessage<TYPES, I>,
    >,
)
where
    I::Exchanges:
        SequencingExchangesType<TYPES, I::Leaf, Message<TYPES, I, SequencingMessage<TYPES, I>>>;

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
    > ConsensusMessageType<TYPES, I> for SequencingMessage<TYPES, I>
where
    I::Exchanges:
        SequencingExchangesType<TYPES, I::Leaf, Message<TYPES, I, SequencingMessage<TYPES, I>>>,
{
    type GeneralConsensusMessage = GeneralConsensusMessage<TYPES, I, Self>;
    type ProcessedConsensusMessage = ProcessedSequencingMessage<TYPES, I, Self>;
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
    > SequencingMessageType<TYPES, I> for SequencingMessage<TYPES, I>
where
    I::Exchanges:
        SequencingExchangesType<TYPES, I::Leaf, Message<TYPES, I, SequencingMessage<TYPES, I>>>,
{
    type CommitteeConsensusMessage = CommitteeConsensusMessage<TYPES, I>;
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
    > SequencingMessage<TYPES, I>
where
    I::Exchanges:
        SequencingExchangesType<TYPES, I::Leaf, Message<TYPES, I, SequencingMessage<TYPES, I>>>,
{
    /// The view number of the (leader|replica|committee member) when the message was sent or the
    /// view of the timeout.
    pub fn view_number(&self) -> TYPES::Time {
        match &self.0 {
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
