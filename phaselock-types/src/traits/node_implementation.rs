//! Composite trait for node behavior
//!
//! This module defines the [`NodeImplementation`] trait, which is a composite trait used for
//! describing the overall behavior of a node, as a composition of implementations of the node trait.

use std::fmt::Debug;

use crate::{
    message::Message,
    traits::{
        network::NetworkingImplementation, stateful_handler::StatefulHandler, storage::Storage,
        BlockContents,
    },
};

/// Node implementation aggregate trait
///
/// This trait exists to collect multiple behavior implementations into one type, to allow
/// `PhaseLock` to avoid annoying numbers of type arguments and type patching.
///
/// It is recommended you implement this trait on a zero sized type, as `PhaseLock`does not actually
/// store or keep a reference to any value implementing this trait.
pub trait NodeImplementation<const N: usize>: Send + Sync + Debug + Clone + 'static {
    /// Block type for this consensus implementation
    type Block: BlockContents<N> + 'static;
    /// State type for this consensus implementation
    type State: crate::traits::State<N, Block = Self::Block>;
    /// Storage type for this consensus implementation
    type Storage: Storage<Self::Block, Self::State, N> + Clone;
    /// Networking type for this consensus implementation
    type Networking: NetworkingImplementation<
            Message<
                Self::Block,
                <<Self as NodeImplementation<N>>::Block as BlockContents<N>>::Transaction,
                Self::State,
                N,
            >,
        > + Clone;
    /// Stateful call back handler for this consensus implementation
    type StatefulHandler: StatefulHandler<N, Block = Self::Block, State = Self::State>;
}
