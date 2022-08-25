//! Abstraction over consumer-defined callbacks

use std::{fmt::Debug, marker::PhantomData};

use crate::traits::{BlockContents, StateContents};

/// Trait for a stateful event handler
///
/// The `HotShot` instance will keep around the provided value of this type, and call the `notify`
/// method every time a series of blocks are committed.
///
/// A do-nothing implementation ([`Stateless`]) is provided, as a convince for implementations that
/// do not need this functionality.
pub trait StatefulHandler: Send + Sync + Debug + 'static {
    /// Block type for this consensus implementation
    type Block: BlockContents + 'static;
    /// State type for this consensus implementation
    type State: StateContents<Block = Self::Block>;

    /// The `HotShot` implementation will call this method, with the series of blocks and states
    /// that are being committed, whenever a commit action takes place.
    ///
    /// The provided states and blocks are guaranteed to be in ascending order of age (newest to
    /// oldest).
    ///
    /// This functionality is discretionary, and whatever behavior is implemented in this method
    /// should not impact consensus logic.
    fn notify(&mut self, blocks: Vec<Self::Block>, states: Vec<Self::State>);
}

/// Dummy, do nothing implementation of [`StatefulHandler`]
pub struct Stateless<B, S> {
    /// Phantom for the block type
    _block: PhantomData<B>,
    /// Phantom for the state type
    _state: PhantomData<S>,
}

impl<B, S> Debug for Stateless<B, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stateless").finish()
    }
}

impl<B, S> Default for Stateless<B, S> {
    fn default() -> Self {
        Self {
            _block: PhantomData,
            _state: PhantomData,
        }
    }
}

impl<B, S> StatefulHandler for Stateless<B, S>
where
    B: BlockContents + 'static,
    S: StateContents<Block = B> + 'static,
{
    type Block = B;

    type State = S;

    fn notify(&mut self, _blocks: Vec<Self::Block>, _states: Vec<Self::State>) {}
}
