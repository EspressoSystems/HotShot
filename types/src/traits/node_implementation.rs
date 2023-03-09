//! Composite trait for node behavior
//!
//! This module defines the [`NodeImplementation`] trait, which is a composite trait used for
//! describing the overall behavior of a node, as a composition of implementations of the node trait.
#![allow(clippy::missing_docs_in_private_items)]
#![allow(missing_docs)]

use serde::Deserialize;

use super::{
    block_contents::Transaction,
    election::{ConsensusExchange, ElectionConfig, Membership, VoteToken},
    network::{CommunicationChannel, NetworkMsg, TestableNetworkingImplementation},
    signature_key::TestableSignatureKey,
    state::{ConsensusTime, ConsensusType, TestableBlock, TestableState},
    storage::TestableStorage,
    State,
};
use crate::message::Message;
use crate::{
    data::{LeafType, ProposalType},
    traits::{
        signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey},
        storage::Storage,
        Block,
    },
    vote::{Accumulator, VoteType},
};
use commit::Commitment;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::hash::Hash;

/// Node implementation aggregate trait
///
/// This trait exists to collect multiple behavior implementations into one type, to allow
/// `HotShot` to avoid annoying numbers of type arguments and type patching.
///
/// It is recommended you implement this trait on a zero sized type, as `HotShot`does not actually
/// store or keep a reference to any value implementing this trait.

pub trait NodeImplementation<TYPES: NodeType>: Send + Sync + Debug + Clone + 'static {
    // type Message: NetworkMsg;
    type Leaf: LeafType<NodeType = TYPES>;

    /// Storage type for this consensus implementation
    type Storage: Storage<TYPES, Self::Leaf> + Clone;

    /// Membership
    /// Time is generic here to allow multiple implementations of membership trait for difference
    /// consensus protocols
    type QuorumExchange: ConsensusExchange<TYPES, Self::Leaf, Message<TYPES, Self>>;

    type CommitteeExchange: ConsensusExchange<TYPES, Self::Leaf, Message<TYPES, Self>>;
}

pub type QuorumProposal<TYPES: NodeType, I: NodeImplementation<TYPES>> =
    <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
    >>::Proposal;
pub type CommitteeProposal<TYPES: NodeType, I: NodeImplementation<TYPES>> =
    <<I as NodeImplementation<TYPES>>::CommitteeExchange as ConsensusExchange<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
    >>::Proposal;

pub type QuorumVoteType<TYPES: NodeType, I: NodeImplementation<TYPES>> =
    <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
    >>::Vote;
pub type CommitteeVote<TYPES: NodeType, I: NodeImplementation<TYPES>> =
    <<I as NodeImplementation<TYPES>>::CommitteeExchange as ConsensusExchange<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
    >>::Vote;

pub type QuorumNetwork<TYPES: NodeType, I: NodeImplementation<TYPES>> =
    <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
    >>::Networking;
pub type CommitteeNetwork<TYPES: NodeType, I: NodeImplementation<TYPES>> =
    <<I as NodeImplementation<TYPES>>::CommitteeExchange as ConsensusExchange<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
    >>::Networking;

pub type QuorumMembership<TYPES: NodeType, I: NodeImplementation<TYPES>> =
    <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
    >>::Membership;
pub type CommitteeMembership<TYPES: NodeType, I: NodeImplementation<TYPES>> =
    <<I as NodeImplementation<TYPES>>::CommitteeExchange as ConsensusExchange<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
    >>::Membership;

/// Trait with all the type definitions that are used in the current hotshot setup.
pub trait NodeType:
    Clone
    + Copy
    + Debug
    + Hash
    + PartialEq
    + Eq
    + PartialOrd
    + Ord
    + Default
    + serde::Serialize
    + for<'de> Deserialize<'de>
    + Send
    + Sync
    + 'static
{
    /// the type of consensus (seuqencing or validating)
    type ConsensusType: ConsensusType;
    /// The time type that this hotshot setup is using.
    ///
    /// This should be the same `Time` that `StateType::Time` is using.
    type Time: ConsensusTime;
    /// The block type that this hotshot setup is using.
    ///
    /// This should be the same block that `StateType::BlockType` is using.
    type BlockType: Block<Transaction = Self::Transaction>;
    /// The signature key that this hotshot setup is using.
    type SignatureKey: SignatureKey;
    /// The vote token that this hotshot setup is using.
    type VoteTokenType: VoteToken;
    /// The transaction type that this hotshot setup is using.
    ///
    /// This should be equal to `Block::Transaction`
    type Transaction: Transaction;
    /// The election config type that this hotshot setup is using.
    type ElectionConfigType: ElectionConfig;

    /// The state type that this hotshot setup is using.
    type StateType: State<BlockType = Self::BlockType, Time = Self::Time>;
}

/// testable node implmeentation trait
pub trait TestableNodeImplementation<TYPES: NodeType>: NodeImplementation<TYPES>
where
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType, Time = TYPES::Time>,
    TYPES::SignatureKey: TestableSignatureKey,
    <Self as NodeImplementation<TYPES>>::Storage:
        TestableStorage<TYPES, <Self as NodeImplementation<TYPES>>::Leaf>,
{
}
