//! Abstraction over the global state that blocks modify
//!
//! This module provides the [`State`] trait, which serves as an abstraction over the current
//! network state, which is modified by the transactions contained within blocks.

use crate::traits::BlockContents;
use serde::{de::DeserializeOwned, Serialize};
use std::{error::Error, fmt::Debug, hash::Hash};

/// Abstraction over the state that blocks modify
///
/// This trait represents the behaviors that the 'global' ledger state must have:
///   * A defined error type ([`Error`](State::Error))
///   * The type of block that modifies this type of state ([`Block`](State::Block))
///   * A method to get a template (empty) next block from the current state
///     ([`next_block`](State::next_block))
///   * The ability to validate that a block is actually a valid extension of this state
///     ([`validate_block`](State::validate_block))
///   * The ability to produce a new state, with the modifications from the block applied
///     ([`append`](State::append))
pub trait State<const N: usize>:
    Serialize + DeserializeOwned + Clone + Debug + Hash + PartialEq + Eq + Send + Sync + Unpin
{
    /// The error type for this particular type of ledger state
    type Error: Error + Debug + Send + Sync;
    /// The type of block this state is associated with
    type Block: BlockContents<N>;

    /// Returns an empty, template next block given this current state
    fn next_block(&self) -> Self::Block;
    /// Returns true if and only if the provided block is valid and can extend this state
    fn validate_block(&self, block: &Self::Block) -> bool;
    /// Appends the given block to this state, returning an new state
    ///
    /// # Errors
    ///
    /// Should produce and error if appending this block would lead to an invalid state
    fn append(&self, block: &Self::Block) -> Result<Self, Self::Error>;
    /// Gets called to notify the persistence backend that this state has been committed
    fn on_commit(&self);
}

/// Dummy implementation of `State` for unit tests
pub mod dummy {
    #[allow(clippy::wildcard_imports)]
    use super::*;
    use crate::traits::block_contents::dummy::{DummyBlock, DummyError};
    use rand::Rng;
    use serde::Deserialize;

    /// The dummy state
    #[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
    pub struct DummyState {
        /// Some dummy data
        nonce: u64,
    }

    impl DummyState {
        /// Generate a random `DummyState`
        pub fn random() -> Self {
            let x = rand::thread_rng().gen();
            Self { nonce: x }
        }
    }

    impl State<32> for DummyState {
        type Error = DummyError;

        type Block = DummyBlock;

        fn next_block(&self) -> Self::Block {
            DummyBlock::random()
        }

        fn validate_block(&self, _block: &Self::Block) -> bool {
            false
        }

        fn append(&self, _block: &Self::Block) -> Result<Self, Self::Error> {
            Err(DummyError)
        }

        fn on_commit(&self) {}
    }
}
