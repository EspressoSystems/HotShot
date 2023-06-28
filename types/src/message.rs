//! Network message types
//!
//! This module contains types used to represent the various types of messages that
//! `HotShot` nodes can send among themselves.

use crate::certificate::ViewSyncCertificate;
use crate::data::DAProposal;
use crate::traits::consensus_type::validating_consensus::ValidatingConsensus;
use crate::traits::network::ViewMessage;
use crate::vote::{DAVote, QuorumVote};
use crate::{
    data::ProposalType,
    traits::{
        consensus_type::{sequencing_consensus::SequencingConsensus, ConsensusType},
        network::NetworkMsg,
        node_implementation::{ExchangesType, NodeImplementation, NodeType, QuorumProposalType},
        signature_key::EncodedSignature,
    },
    vote::{ViewSyncVote, VoteType},
};
use derivative::Derivative;
use either::Either::{self, Left, Right};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::marker::PhantomData;

/// Incoming message
#[derive(Serialize, Deserialize, Clone, Debug, Derivative)]
#[serde(bound(deserialize = "", serialize = ""))]
#[derivative(PartialEq)]
pub struct Message<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// The sender of this message
    pub sender: TYPES::SignatureKey,

    /// The message kind
    #[derivative(PartialEq = "ignore")]
    pub kind: MessageKind<TYPES::ConsensusType, TYPES, I>,

    /// Phantom data.
    pub _phantom: PhantomData<I>,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> NetworkMsg for Message<TYPES, I> {}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> ViewMessage<TYPES> for Message<TYPES, I> {
    /// get the view number out of a message
    fn get_view_number(&self) -> TYPES::Time {
        self.kind.get_view_number()
    }
    fn purpose(&self) -> MessagePurpose {
        self.kind.purpose()
    }
}

/// A message type agnostic description of a messages purpose
#[derive(PartialEq, Copy, Clone)]
pub enum MessagePurpose {
    /// Message contains a proposal
    Proposal,
    /// Message contains a vote
    Vote,
    /// Message for internal use
    Internal,
    /// Data message
    Data,
}

// TODO (da) make it more customized to the consensus layer, maybe separating the specific message
// data from the kind enum.
/// Enum representation of any message type
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""))]
pub enum MessageKind<
    CONSENSUS: ConsensusType,
    TYPES: NodeType<ConsensusType = CONSENSUS>,
    I: NodeImplementation<TYPES>,
> {
    /// Messages related to the consensus protocol
    Consensus(I::ConsensusMessage),
    /// Messages relating to sharing data between nodes
    Data(DataMessage<TYPES>),
    /// Phantom data.
    _Unreachable(PhantomData<I>),
}

impl<
        CONSENSUS: ConsensusType,
        TYPES: NodeType<ConsensusType = CONSENSUS>,
        I: NodeImplementation<TYPES>,
    > MessageKind<CONSENSUS, TYPES, I>
{
    // Can't implement `From<I::ConsensusMessage>` directly due to potential conflict with
    // `From<DataMessage>`.
    /// Construct a [`MessageKind`] from [`I::ConsensusMessage`].
    pub fn from_consensus_message(m: I::ConsensusMessage) -> Self {
        Self::Consensus(m)
    }
}

impl<
        CONSENSUS: ConsensusType,
        TYPES: NodeType<ConsensusType = CONSENSUS>,
        I: NodeImplementation<TYPES>,
    > From<DataMessage<TYPES>> for MessageKind<CONSENSUS, TYPES, I>
{
    fn from(m: DataMessage<TYPES>) -> Self {
        Self::Data(m)
    }
}

impl<
        CONSENSUS: ConsensusType,
        TYPES: NodeType<ConsensusType = CONSENSUS>,
        I: NodeImplementation<TYPES>,
    > ViewMessage<TYPES> for MessageKind<CONSENSUS, TYPES, I>
{
    fn get_view_number(&self) -> TYPES::Time {
        match &self {
            MessageKind::Consensus(message) => message.view_number(),
            MessageKind::Data(DataMessage::SubmitTransaction(_, v)) => *v,
            MessageKind::_Unreachable(_) => unimplemented!(),
        }
    }

    fn purpose(&self) -> MessagePurpose {
        match &self {
            MessageKind::Consensus(message) => message.purpose(),
            MessageKind::Data(message) => match message {
                DataMessage::SubmitTransaction(_, _) => MessagePurpose::Data,
            },
            MessageKind::_Unreachable(_) => unimplemented!(),
        }
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
pub enum ProcessedGeneralConsensusMessage<TYPES: NodeType, I: NodeImplementation<TYPES>>
where
    I::Exchanges: ExchangesType<TYPES::ConsensusType, TYPES, I::Leaf, Message<TYPES, I>>,
{
    /// Leader's proposal for full Quorom voting
    Proposal(Proposal<QuorumProposalType<TYPES, I>>, TYPES::SignatureKey),
    /// Replica's vote on a proposal.
    Vote(QuorumVote<TYPES, I::Leaf>, TYPES::SignatureKey),
    /// Internal ONLY message indicating a view interrupt.
    #[serde(skip)]
    InternalTrigger(InternalTrigger<TYPES>),
    /// A view sync related message - either a vote or certificate
    ViewSync(ViewSyncMessageType<TYPES>),
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> From<ProcessedGeneralConsensusMessage<TYPES, I>>
    for GeneralConsensusMessage<TYPES, I>
where
    I::Exchanges: ExchangesType<TYPES::ConsensusType, TYPES, I::Leaf, Message<TYPES, I>>,
{
    fn from(value: ProcessedGeneralConsensusMessage<TYPES, I>) -> Self {
        match value {
            ProcessedGeneralConsensusMessage::Proposal(p, _) => {
                GeneralConsensusMessage::Proposal(p)
            }
            ProcessedGeneralConsensusMessage::Vote(v, _) => GeneralConsensusMessage::Vote(v),
            ProcessedGeneralConsensusMessage::InternalTrigger(a) => {
                GeneralConsensusMessage::InternalTrigger(a)
            }
            ProcessedGeneralConsensusMessage::ViewSync(_) => todo!(),
        }
    }
}

impl<
        TYPES: NodeType<ConsensusType = ValidatingConsensus>,
        I: NodeImplementation<TYPES, ConsensusMessage = ValidatingMessage<TYPES, I>>,
    > From<ProcessedGeneralConsensusMessage<TYPES, I>> for ValidatingMessage<TYPES, I>
{
    fn from(value: ProcessedGeneralConsensusMessage<TYPES, I>) -> Self {
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
            ProcessedGeneralConsensusMessage::ViewSync(_) => todo!(),
        }
    }
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> ProcessedGeneralConsensusMessage<TYPES, I>
where
    I::Exchanges: ExchangesType<TYPES::ConsensusType, TYPES, I::Leaf, Message<TYPES, I>>,
{
    /// Create a [`ProcessedGeneralConsensusMessage`] from a [`GeneralConsensusMessage`].
    /// # Panics
    /// if reaching the unimplemented `ViewSync` case.
    pub fn new(value: GeneralConsensusMessage<TYPES, I>, sender: TYPES::SignatureKey) -> Self {
        match value {
            GeneralConsensusMessage::Proposal(p) => {
                ProcessedGeneralConsensusMessage::Proposal(p, sender)
            }
            GeneralConsensusMessage::Vote(v) => ProcessedGeneralConsensusMessage::Vote(v, sender),
            GeneralConsensusMessage::InternalTrigger(a) => {
                ProcessedGeneralConsensusMessage::InternalTrigger(a)
            }
            GeneralConsensusMessage::ViewSync(_) => todo!(),
        }
    }
}

/// A processed consensus message for the DA committee in sequencing consensus.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]
pub enum ProcessedCommitteeConsensusMessage<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
> {
    /// Proposal for data availability committee
    DAProposal(Proposal<DAProposal<TYPES>>, TYPES::SignatureKey),
    /// vote from the DA committee
    DAVote(DAVote<TYPES, I::Leaf>, TYPES::SignatureKey),
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
    > From<ProcessedCommitteeConsensusMessage<TYPES, I>> for CommitteeConsensusMessage<TYPES, I>
{
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
{
    /// Create a [`ProcessedCommitteeConsensusMessage`] from a [`CommitteeConsensusMessage`].
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

/// A processed consensus message for sequencing consensus.
pub type ProcessedSequencingMessage<TYPES, I> = Either<
    ProcessedGeneralConsensusMessage<TYPES, I>,
    ProcessedCommitteeConsensusMessage<TYPES, I>,
>;

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
    > From<ProcessedSequencingMessage<TYPES, I>> for SequencingMessage<TYPES, I>
{
    fn from(value: ProcessedSequencingMessage<TYPES, I>) -> Self {
        match value {
            Left(message) => SequencingMessage(Left(message.into())),
            Right(message) => SequencingMessage(Right(message.into())),
        }
    }
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
    > From<ProcessedGeneralConsensusMessage<TYPES, I>> for ProcessedSequencingMessage<TYPES, I>
{
    fn from(value: ProcessedGeneralConsensusMessage<TYPES, I>) -> Self {
        Left(value)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""))]
/// Messages related to both validating and sequencing consensus.
pub enum GeneralConsensusMessage<TYPES: NodeType, I: NodeImplementation<TYPES>>
where
    I::Exchanges: ExchangesType<TYPES::ConsensusType, TYPES, I::Leaf, Message<TYPES, I>>,
{
    /// Leader's proposal for full quorum voting
    Proposal(Proposal<QuorumProposalType<TYPES, I>>),

    /// Replica's vote on a proposal.
    Vote(QuorumVote<TYPES, I::Leaf>),

    /// Internal ONLY message indicating a view interrupt.
    #[serde(skip)]
    InternalTrigger(InternalTrigger<TYPES>),

    /// View Sync related message - either a vote or certificate
    ViewSync(ViewSyncMessageType<TYPES>),
}

/// A view sync message
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""))]
pub enum ViewSyncMessageType<TYPES: NodeType> {
    /// A view sync vote
    Vote(ViewSyncVote<TYPES>),
    /// A view sync certificate
    Certificate(ViewSyncCertificate<TYPES>),
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""))]
/// Messages related to the sequencing consensus protocol for the DA committee.
pub enum CommitteeConsensusMessage<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
> {
    /// Proposal for data availability committee
    DAProposal(Proposal<DAProposal<TYPES>>),

    /// vote for data availability committee
    DAVote(DAVote<TYPES, I::Leaf>),
}

/// Messages related to the consensus protocol.
pub trait ConsensusMessageType<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// The type of messages for both validating and sequencing consensus.
    type GeneralConsensusMessage;

    /// The type of processed consensus messages.
    type ProcessedConsensusMessage: Send;

    /// Get the view number when the message was sent or the view of the timeout.
    fn view_number(&self) -> TYPES::Time;

    /// Get the message purpose.
    fn purpose(&self) -> MessagePurpose;
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
    I: NodeImplementation<TYPES, ConsensusMessage = ValidatingMessage<TYPES, I>>,
>(pub GeneralConsensusMessage<TYPES, I>);

impl<
        TYPES: NodeType<ConsensusType = ValidatingConsensus>,
        I: NodeImplementation<TYPES, ConsensusMessage = ValidatingMessage<TYPES, I>>,
    > ConsensusMessageType<TYPES, I> for ValidatingMessage<TYPES, I>
{
    type GeneralConsensusMessage = GeneralConsensusMessage<TYPES, I>;
    type ProcessedConsensusMessage = ProcessedGeneralConsensusMessage<TYPES, I>;

    // TODO: Disable panic after the `ViewSync` case is implemented.
    #[allow(clippy::panic)]
    fn view_number(&self) -> TYPES::Time {
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
            GeneralConsensusMessage::ViewSync(_) => todo!(),
        }
    }

    // TODO: Disable panic after the `ViewSync` case is implemented.
    #[allow(clippy::panic)]
    fn purpose(&self) -> MessagePurpose {
        match &self.0 {
            GeneralConsensusMessage::Proposal(_) => MessagePurpose::Proposal,
            GeneralConsensusMessage::Vote(_) => MessagePurpose::Vote,
            GeneralConsensusMessage::InternalTrigger(_) => MessagePurpose::Internal,
            GeneralConsensusMessage::ViewSync(_) => todo!(),
        }
    }
}

/// Messages for sequencing consensus.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(bound(deserialize = "", serialize = ""))]
pub struct SequencingMessage<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
>(pub Either<GeneralConsensusMessage<TYPES, I>, CommitteeConsensusMessage<TYPES, I>>);

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
    > ConsensusMessageType<TYPES, I> for SequencingMessage<TYPES, I>
{
    type GeneralConsensusMessage = GeneralConsensusMessage<TYPES, I>;
    type ProcessedConsensusMessage = ProcessedSequencingMessage<TYPES, I>;

    // TODO: Disable panic after the `ViewSync` case is implemented.
    #[allow(clippy::panic)]
    fn view_number(&self) -> TYPES::Time {
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
                    GeneralConsensusMessage::ViewSync(_) => todo!(),
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

    // TODO: Disable panic after the `ViewSync` case is implemented.
    #[allow(clippy::panic)]
    fn purpose(&self) -> MessagePurpose {
        match &self.0 {
            Left(general_message) => match general_message {
                GeneralConsensusMessage::Proposal(_) => MessagePurpose::Proposal,
                GeneralConsensusMessage::Vote(_) => MessagePurpose::Vote,
                GeneralConsensusMessage::InternalTrigger(_) => MessagePurpose::Internal,
                GeneralConsensusMessage::ViewSync(_) => todo!(),
            },
            Right(committee_message) => match committee_message {
                CommitteeConsensusMessage::DAProposal(_) => MessagePurpose::Proposal,
                CommitteeConsensusMessage::DAVote(_) => MessagePurpose::Vote,
            },
        }
    }
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, I>>,
    > SequencingMessageType<TYPES, I> for SequencingMessage<TYPES, I>
{
    type CommitteeConsensusMessage = CommitteeConsensusMessage<TYPES, I>;
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
