//! Composite trait for node behavior
//!
//! This module defines the [`NodeImplementation`] trait, which is a composite trait used for
//! describing the overall behavior of a node, as a composition of implementations of the node trait.

use crate::{
    data::ViewNumber,
    message::{ConsensusMessage, DataMessage, Message, MessageKind},
    traits::{
        election::Election, network::NetworkingImplementation, signature_key::SignatureKey,
        storage::Storage, Block,
    },
};
use std::fmt::Debug;

use super::{State, state::{TestableState, TestableBlock}, network::TestableNetworkingImplementation, signature_key::TestableSignatureKey, storage::TestableStorage};

/// Node implementation aggregate trait
///
/// This trait exists to collect multiple behavior implementations into one type, to allow
/// `HotShot` to avoid annoying numbers of type arguments and type patching.
///
/// It is recommended you implement this trait on a zero sized type, as `HotShot`does not actually
/// store or keep a reference to any value implementing this trait.
pub trait NodeImplementation: Send + Sync + Debug + Clone + 'static {
    /// State type for this consensus implementation
    type StateType: State<Time = ViewNumber>;
    /// Storage type for this consensus implementation
    type Storage: Storage<Self::StateType> + Clone;
    /// Networking type for this consensus implementation
    type Networking: NetworkingImplementation<Message<Self::StateType, Self::SignatureKey>, Self::SignatureKey>
        + Clone;
    /// The signature key type for this implementation
    type SignatureKey: SignatureKey;
    /// Election
    /// Time is generic here to allow multiple implementations of election trait for difference
    /// consensus protocols
    type Election: Election<Self::SignatureKey, ViewNumber, StateType = Self::StateType>;
}

/// testable node implmeentation trait
pub trait TestableNodeImplementation: Send + Sync + Debug + Clone + 'static {
    /// State type for this consensus implementation
    type StateType: TestableState<BlockType = Self::Block>;
    /// Storage type for this consensus implementation
    type Storage: TestableStorage<Self::StateType>;
    /// Networking type for this consensus implementation
    type Networking: TestableNetworkingImplementation<Message<Self::StateType, Self::SignatureKey>, Self::SignatureKey>;
    /// The signature key type for this implementation
    type SignatureKey: TestableSignatureKey;
    /// Election
    /// Time is generic here to allow multiple implementations of election trait for difference
    /// consensus protocols
    type Election: Election<Self::SignatureKey, ViewNumber, StateType = Self::StateType>;
    /// block
    type Block: TestableBlock;

    /// propagate
    type NodeImplementation: NodeImplementation<StateType = Self::StateType, Storage = Self::Storage, Networking = Self::Networking, SignatureKey = Self::SignatureKey, Election = Self::Election>;
}

/// Helper trait to make aliases.
///
/// This allows you to replace
///
/// ```ignore
/// Message<
///     I::State,
///     I::SignatureKey
/// >
/// ```
///
/// with
///
/// ```ignore
/// <I as TypeMap>::Message
/// ```
pub trait TypeMap {
    /// Type alias for the [`Message`] enum.
    type Message;
    /// Type alias for the [`MessageKind`] enum.
    type MessageKind;
    /// Type alias for the [`ConsensusMessage`] enum.
    type ConsensusMessage;
    /// Type alias for the [`DataMessage`] enum.
    type DataMessage;
    /// Type alias for the [`BlockContents::Transaction`] implementation.
    type Transaction;
}

impl<I: NodeImplementation> TypeMap for I {
    type Message = Message<I::StateType, I::SignatureKey>;
    type MessageKind = MessageKind<I::StateType>;
    type ConsensusMessage = ConsensusMessage<I::StateType>;
    type DataMessage = DataMessage<I::StateType>;
    type Transaction = <<I::StateType as State>::BlockType as Block>::Transaction;
}
