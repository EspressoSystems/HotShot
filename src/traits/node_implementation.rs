use std::fmt::Debug;

use crate::{
    message::Message, networking::NetworkingImplementation, traits::state, BlockContents, Storage,
};

/// Node implementation aggregate trait
pub trait NodeImplementation<const N: usize>: Send + Sync + Debug + Clone + 'static {
    /// Block type for this consensus implementation
    type Block: BlockContents<N> + 'static;
    /// State type for this consensus implementation
    type State: state::State<N, Block = Self::Block>;
    /// Storage type for this consensus implementation
    type Storage: Storage<Self::Block, Self::State, N> + Clone;
    /// Networking type for this consensus implementation
    type Networking: NetworkingImplementation<
            Message<
                Self::Block,
                <<Self as NodeImplementation<N>>::Block as BlockContents<N>>::Transaction,
                N,
            >,
        > + Clone;
}
