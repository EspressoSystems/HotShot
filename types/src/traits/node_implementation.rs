//! Composite trait for node behavior
//!
//! This module defines the [`NodeImplementation`] trait, which is a composite trait used for
//! describing the overall behavior of a node, as a composition of implementations of the node trait.

use commit::{Commitment, Committable};

use crate::{
    message::{ConsensusMessage, DataMessage, Message, MessageKind},
    traits::{
        election::Election, network::NetworkingImplementation, signature_key::SignatureKey,
        stateful_handler::StatefulHandler, storage::Storage, BlockContents,
    },
};
use std::fmt::Debug;

/// Node implementation aggregate trait
///
/// This trait exists to collect multiple behavior implementations into one type, to allow
/// `HotShot` to avoid annoying numbers of type arguments and type patching.
///
/// It is recommended you implement this trait on a zero sized type, as `HotShot`does not actually
/// store or keep a reference to any value implementing this trait.
pub trait NodeImplementation: Send + Sync + Debug + Clone + 'static {
    /// Block type for this consensus implementation
    type Block: BlockContents + 'static;
    /// State type for this consensus implementation
    type State: crate::traits::StateContents<Block = Self::Block>;
    /// Storage type for this consensus implementation
    type Storage: Storage<Self::State> + Clone;
    /// Networking type for this consensus implementation
    type Networking: NetworkingImplementation<Message<Self::State, Self::SignatureKey>, Self::SignatureKey>
        + Clone;
    /// Stateful call back handler for this consensus implementation
    type StatefulHandler: StatefulHandler<Block = Self::Block, State = Self::State>;
    /// The election algorithm
    type Election: Election<Self::SignatureKey>;
    /// The signature key type for this implementation
    type SignatureKey: SignatureKey;
}

/// Helper trait to make aliases.
///
/// This allows you to replace
///
/// ```ignore
/// Message<
///     I::Block,
///     <I::Block as BlockContents<N>>::Transaction,
///     I::State,
///     N,
/// >
/// ```
///
/// with
///
/// ```ignore
/// <I as TypeMap<N>>::Message
/// ```
pub trait TypeMap {
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
    type MessageKind = MessageKind<I::State>;
    type ConsensusMessage = ConsensusMessage<I::State>;
    type DataMessage = DataMessage<I::State>;
    type Transaction = <I::Block as BlockContents>::Transaction;
}
