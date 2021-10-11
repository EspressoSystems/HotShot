use std::fmt::Debug;

use crate::{message::Message, networking::NetworkingImplementation, BlockContents, Storage};

/// Node implementation aggregate trait
pub trait NodeImplementation<const N: usize>: Send + Sync + Debug + Clone {
    /// Block type for this consensus implementation
    type Block: BlockContents<N> + 'static;
    /// Storage type for this consensus implementation
    type Storage: Storage<Self::Block, N> + Clone;
    /// Networking type for this consensus implementation
    type Networking: NetworkingImplementation<
            Message<
                Self::Block,
                <<Self as NodeImplementation<N>>::Block as BlockContents<N>>::Transaction,
                N,
            >,
        > + Clone;
}
