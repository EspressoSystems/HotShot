//! Abstraction over the global state that blocks modify
//!
//! This module provides the [`State`] trait, which serves as an abstraction over the current
//! network state, which is modified by the transactions contained within blocks.

use crate::{data::ViewNumber, traits::Block};
use commit::Committable;
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
pub trait State:
    Serialize
    + DeserializeOwned
    + Clone
    + Debug
    + Default
    + Hash
    + PartialEq
    + Eq
    + Send
    + Sync
    + Committable
{
    /// The error type for this particular type of ledger state
    type Error: Error + Debug + Send + Sync;
    /// The type of block this state is associated with
    type BlockType: Block;
    /// Time abstraction needed for reward collection
    type Time: ConsensusTime;

    /// Returns an empty, template next block given this current state
    fn next_block(&self) -> Self::BlockType;
    /// Returns true if and only if the provided block is valid and can extend this state
    fn validate_block(&self, block: &Self::BlockType, time: &Self::Time) -> bool;
    /// Appends the given block to this state, returning an new state
    ///
    /// # Errors
    ///
    /// Should produce and error if appending this block would lead to an invalid state
    fn append(&self, block: &Self::BlockType, time: &Self::Time) -> Result<Self, Self::Error>;
    /// Gets called to notify the persistence backend that this state has been committed
    fn on_commit(&self);
}

/// Trait for time abstraction needed for reward collection
pub trait ConsensusTime: PartialOrd {}

/// extra functions required on state to be usable by hotshot-testing
pub trait TestableState: State<Time = ViewNumber>
where
    <Self as State>::BlockType: TestableBlock,
{
    /// Creates random transaction if possible
    /// otherwise panics
    fn create_random_transaction(
        &self,
        rng: &mut dyn rand::RngCore,
    ) -> <Self::BlockType as Block>::Transaction;
}

/// extra functions required on block to be usable by hotshot-testing
pub trait TestableBlock: Block {
    /// generate a genesis block
    fn genesis() -> Self;
}

/// Dummy implementation of `State` for unit tests
pub mod dummy {
    #[allow(clippy::wildcard_imports)]
    use super::*;
    use crate::{
        data::ViewNumber,
        traits::block_contents::dummy::{DummyBlock, DummyError},
    };
    use rand::Rng;
    use serde::Deserialize;

    /// The dummy state
    #[derive(Clone, Debug, Default, Hash, PartialEq, Eq, Serialize, Deserialize)]
    pub struct DummyState {
        /// Some dummy data
        nonce: u64,
    }

    impl Committable for DummyState {
        fn commit(&self) -> commit::Commitment<Self> {
            commit::RawCommitmentBuilder::new("Dummy State Comm")
                .u64_field("Nonce", self.nonce)
                .finalize()
        }
    }

    impl DummyState {
        /// Generate a random `DummyState`
        pub fn random(r: &mut dyn rand::RngCore) -> Self {
            Self {
                nonce: r.gen_range(1..1_000_000),
            }
        }
    }

    impl State for DummyState {
        type Error = DummyError;

        type BlockType = DummyBlock;
        type Time = ViewNumber;

        fn next_block(&self) -> Self::BlockType {
            DummyBlock { nonce: self.nonce }
        }

        fn validate_block(&self, _block: &Self::BlockType, _time: &Self::Time) -> bool {
            false
        }

        fn append(
            &self,
            _block: &Self::BlockType,
            _time: &Self::Time,
        ) -> Result<Self, Self::Error> {
            Err(DummyError)
        }

        fn on_commit(&self) {}
    }
}
