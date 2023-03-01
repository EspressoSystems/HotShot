//! Network message types
//!
//! This module contains types used to represent the various types of messages that
//! `HotShot` nodes can send among themselves.

use crate::{
    data::{LeafType, ProposalType},
    traits::{
        network::NetworkMsg,
        node_implementation::{
            CommitteeProposal, CommitteeVote, NodeImplementation, NodeType, QuorumProposal,
            QuorumVoteType,
        },
        signature_key::{EncodedPublicKey, EncodedSignature},
    },
};
use commit::Commitment;
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// The vote sent by consensus messages.
pub trait VoteType<TYPES: NodeType>:
    Debug + Clone + 'static + Serialize + for<'a> Deserialize<'a> + Send + Sync
{
    /// The view this vote was cast for.
    fn current_view(&self) -> TYPES::Time;
}

/// Incoming message
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(bound(deserialize = ""))]
pub struct Message<TYPES: NodeType, I: NodeImplementation<TYPES>>
{
    /// The sender of this message
    pub sender: TYPES::SignatureKey,

    /// The message kind
    pub kind: MessageKind<TYPES, I>,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> NetworkMsg
    for Message<TYPES, I>
{
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>>
    Message<TYPES, I>
{
    /// get the view number out of a message
    pub fn get_view_number(&self) -> TYPES::Time {
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
}

// TODO (da) make it more customized to the consensus layer, maybe separating the specific message
// data from the kind enum.
/// Enum representation of any message type
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(bound(deserialize = ""))]
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
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(bound(deserialize = ""))]
pub enum ProcessedConsensusMessage<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Leader's proposal
    Proposal(Proposal<QuorumProposal<TYPES, I>>, TYPES::SignatureKey),
    DAProposal(Proposal<CommitteeProposal<TYPES, I>>, TYPES::SignatureKey),
    /// Replica's vote on a proposal.
    Vote(QuorumVoteType<TYPES, I>, TYPES::SignatureKey),
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(bound(deserialize = ""))]
/// Messages related to the consensus protocol
pub enum ConsensusMessage<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    // PROPOSAL: ProposalType<NodeType = TYPES>,
    // VOTE: VoteType<TYPES>,
> {
    /// Leader's proposal
    Proposal(Proposal<QuorumProposal<TYPES, I>>),
    DAProposal(Proposal<CommitteeProposal<TYPES, I>>),

    /// Replica's vote on a proposal.
    Vote(QuorumVoteType<TYPES, I>),
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

/// A vote on DA proposal.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]
pub struct DAVote<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> {
    /// TODO we should remove this
    /// this is correct, but highly inefficient
    /// we should check a cache, and if that fails request the qc
    pub justify_qc_commitment: Commitment<LEAF::QuorumCertificate>,
    /// The signature share associated with this vote
    /// TODO ct/vrf make ConsensusMessage generic over I instead of serializing to a [`Vec<u8>`]
    pub signature: (EncodedPublicKey, EncodedSignature),
    /// The block commitment being voted on.
    pub block_commitment: Commitment<TYPES::BlockType>,
    /// The view this vote was cast for
    pub current_view: TYPES::Time,
    /// The vote token generated by this replica
    pub vote_token: TYPES::VoteTokenType,
}

/// A positive or negative vote on valiadting or commitment proposal.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]
pub struct YesOrNoVote<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> {
    /// TODO we should remove this
    /// this is correct, but highly inefficient
    /// we should check a cache, and if that fails request the qc
    pub justify_qc_commitment: Commitment<LEAF::QuorumCertificate>,
    /// The signature share associated with this vote
    /// TODO ct/vrf make ConsensusMessage generic over I instead of serializing to a [`Vec<u8>`]
    pub signature: (EncodedPublicKey, EncodedSignature),
    /// The leaf commitment being voted on.
    pub leaf_commitment: Commitment<LEAF>,
    /// The view this vote was cast for
    pub current_view: TYPES::Time,
    /// The vote token generated by this replica
    pub vote_token: TYPES::VoteTokenType,
}

/// A timeout vote.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]
pub struct TimeoutVote<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> {
    /// The justification qc for this view
    pub justify_qc: LEAF::QuorumCertificate,
    /// The signature share associated with this vote
    /// TODO ct/vrf make ConsensusMessage generic over I instead of serializing to a [`Vec<u8>`]
    pub signature: (EncodedPublicKey, EncodedSignature),
    /// The view this vote was cast for
    pub current_view: TYPES::Time,
    /// The vote token generated by this replica
    pub vote_token: TYPES::VoteTokenType,
}

/// Votes on validating or commitment proposal.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(bound(deserialize = ""))]
pub enum QuorumVote<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> {
    /// Posivite vote.
    Yes(YesOrNoVote<TYPES, LEAF>),
    /// Negative vote.
    No(YesOrNoVote<TYPES, LEAF>),
    /// Timeout vote.
    Timeout(TimeoutVote<TYPES, LEAF>),
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> VoteType<TYPES> for DAVote<TYPES, LEAF> {
    fn current_view(&self) -> TYPES::Time {
        self.current_view
    }
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> VoteType<TYPES>
    for QuorumVote<TYPES, LEAF>
{
    fn current_view(&self) -> TYPES::Time {
        match self {
            QuorumVote::Yes(v) | QuorumVote::No(v) => v.current_view,
            QuorumVote::Timeout(v) => v.current_view,
        }
    }
}
