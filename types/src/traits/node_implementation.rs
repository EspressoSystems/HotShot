//! Composite trait for node behavior
//!
//! This module defines the [`NodeImplementation`] trait, which is a composite trait used for
//! describing the overall behavior of a node, as a composition of implementations of the node trait.

use super::{
    block_contents::Transaction,
    election::VoteToken,
    network::TestableNetworkingImplementation,
    signature_key::TestableSignatureKey,
    state::{ConsensusTime, TestableBlock, TestableState},
    storage::TestableStorage,
    State,
};
use crate::traits::{
    election::Election, network::NetworkingImplementation, signature_key::SignatureKey,
    storage::Storage, Block,
};
use std::{fmt::Debug, marker::PhantomData};

/// Node implementation aggregate trait
///
/// This trait exists to collect multiple behavior implementations into one type, to allow
/// `HotShot` to avoid annoying numbers of type arguments and type patching.
///
/// It is recommended you implement this trait on a zero sized type, as `HotShot`does not actually
/// store or keep a reference to any value implementing this trait.
pub trait NodeImplementation: Send + Sync + Debug + Clone + 'static {
    /// The time unit that this consensus is using
    type Time: ConsensusTime;
    /// Block type for this consensus implementation
    type BlockType: Block;
    /// The signature key type for this implementation
    type SignatureKey: SignatureKey;
    /// The vote token type for this implementation
    type VoteTokenType: VoteToken;

    /// State type for this consensus implementation
    type StateType: State<Time = Self::Time, BlockType = Self::BlockType>;

    /// Storage type for this consensus implementation
    type Storage: Storage<NodeTypesImpl<Self>> + Clone;
    /// Networking type for this consensus implementation
    type Networking: NetworkingImplementation<NodeTypesImpl<Self>>;
    /// Election
    /// Time is generic here to allow multiple implementations of election trait for difference
    /// consensus protocols
    type Election: Election<NodeTypesImpl<Self>>;
}

pub trait NodeTypes: 'static {
    type Time: ConsensusTime;
    type BlockType: Block<Transaction = Self::Transaction>;
    type SignatureKey: SignatureKey;
    type VoteTokenType: VoteToken;
    type Transaction: Transaction;

    type StateType: State<BlockType = Self::BlockType, Time = Self::Time>;
}

pub struct NodeTypesImpl<I>(PhantomData<I>);

impl<I> NodeTypes for NodeTypesImpl<I>
where
    I: NodeImplementation,
{
    type Time = I::Time;
    type SignatureKey = I::SignatureKey;
    type BlockType = I::BlockType;
    type StateType = I::StateType;
    type VoteTokenType = I::VoteTokenType;
    type Transaction = <I::BlockType as Block>::Transaction;
}

/// testable node implmeentation trait
pub trait TestableNodeImplementation: Send + Sync + Debug + Clone + 'static {
    ///
    type Time: ConsensusTime;

    /// State type for this consensus implementation
    type StateType: TestableState<BlockType = Self::BlockType, Time = Self::Time>;
    /// Storage type for this consensus implementation
    type Storage: TestableStorage<NodeTypesTestableImpl<Self>>;
    /// Networking type for this consensus implementation
    type Networking: TestableNetworkingImplementation<NodeTypesTestableImpl<Self>>;
    /// The signature key type for this implementation
    type SignatureKey: TestableSignatureKey;
    /// Election
    /// Time is generic here to allow multiple implementations of election trait for difference
    /// consensus protocols
    type Election: Election<NodeTypesTestableImpl<Self>>;
    /// block
    type BlockType: TestableBlock;
    /// vote token
    type VoteTokenType: VoteToken;

    /// propagate
    type NodeImplementation: NodeImplementation<
        StateType = Self::StateType,
        Storage = Self::Storage,
        Networking = Self::Networking,
        SignatureKey = Self::SignatureKey,
        Election = Self::Election,
    >;
}

pub struct NodeTypesTestableImpl<I>(PhantomData<I>);

impl<I> NodeTypes for NodeTypesTestableImpl<I>
where
    I: TestableNodeImplementation,
{
    type Time = I::Time;
    type SignatureKey = I::SignatureKey;
    type BlockType = I::BlockType;
    type StateType = I::StateType;
    type VoteTokenType = I::VoteTokenType;
    type Transaction = <I::BlockType as Block>::Transaction;
}

// /// Helper trait to make aliases.
// ///
// /// This allows you to replace
// ///
// /// ```ignore
// /// Message<
// ///     I::State,
// ///     I::SignatureKey
// /// >
// /// ```
// ///
// /// with
// ///
// /// ```ignore
// /// <I as TypeMap>::Message
// /// ```
// pub trait TypeMap {
//     /// Type alias for the [`Message`] enum.
//     type Message;
//     /// Type alias for the [`MessageKind`] enum.
//     type MessageKind;
//     /// Type alias for the [`ConsensusMessage`] enum.
//     type ConsensusMessage;
//     /// Type alias for the [`DataMessage`] enum.
//     type DataMessage;
//     /// Type alias for the [`BlockContents::Transaction`] implementation.
//     type Transaction;
// }

// impl<I: NodeImplementation> TypeMap for I {
//     type Message = Message<I::StateType, I::SignatureKey>;
//     type MessageKind = MessageKind<I::StateType>;
//     type ConsensusMessage = ConsensusMessage<I::StateType>;
//     type DataMessage = DataMessage<I::StateType>;
//     type Transaction = <<I::StateType as State>::BlockType as Block>::Transaction;
// }
