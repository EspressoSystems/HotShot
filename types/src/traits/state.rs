//! Abstraction over the global state that blocks modify
//!
//! This module provides the [`State`] trait, which serves as an compatibility over the current
//! network state, which is modified by the transactions contained within blocks.
#![allow(clippy::missing_docs_in_private_items)]
#![allow(missing_docs)]

use crate::traits::Block;
use commit::Committable;
use espresso_systems_common::hotshot::tag;
use serde::{de::DeserializeOwned, Serialize};
use std::{error::Error, fmt::Debug, hash::Hash, ops, ops::Deref};

/// Abstraction over the state that blocks modify
///
/// This trait represents the behaviors that the 'global' ledger state must have:
///   * A defined error type ([`Error`](State::Error))
///   * The type of block that modifies this type of state ([`Block`](State::BlockType))
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
    /// Time compatibility needed for reward collection
    type Time: ConsensusTime;

    /// Returns an empty, template next block given this current state
    fn next_block(&self) -> Self::BlockType;

    /// Returns true if and only if the provided block is valid and can extend this state
    fn validate_block(&self, block: &Self::BlockType, view_number: &Self::Time) -> bool;

    /// Appends the given block to this state, returning an new state
    ///
    /// # Errors
    ///
    /// Should produce and error if appending this block would lead to an invalid state
    fn append(
        &self,
        block: &Self::BlockType,
        view_number: &Self::Time,
    ) -> Result<Self, Self::Error>;

    /// Gets called to notify the persistence backend that this state has been committed
    fn on_commit(&self);
}

// TODO Seuqnecing here means involving DA in consensus

/// may need to make these public
pub trait SequencingConsensusType
where
    Self: ConsensusType,
{
}
pub trait ValidatingConsensusType
where
    Self: ConsensusType,
{
}
pub trait ConsensusType: Clone + Send + Sync {}

#[derive(Clone)]
pub struct SequencingConsensus;
impl SequencingConsensusType for SequencingConsensus {}
impl ConsensusType for SequencingConsensus {}

#[derive(Clone)]
pub struct ValidatingConsensus;
impl ConsensusType for ValidatingConsensus {}
impl ValidatingConsensusType for ValidatingConsensus {}

/// Trait for time compatibility needed for reward collection
pub trait ConsensusTime:
    PartialOrd
    + Ord
    + Send
    + Sync
    + Debug
    + Clone
    + Copy
    + Hash
    + Deref<Target = u64>
    + serde::Serialize
    + for<'de> serde::Deserialize<'de>
    + ops::AddAssign<u64>
    + ops::Add<u64, Output = Self>
    + 'static
{
    /// Create a new instance of this time unit at time number 0
    #[must_use]
    fn genesis() -> Self {
        Self::new(0)
    }
    /// Create a new instance of this time unit
    fn new(val: u64) -> Self;
}

/// extra functions required on state to be usable by hotshot-testing
pub trait TestableState: State
where
    <Self as State>::BlockType: TestableBlock,
{
    /// Creates random transaction if possible
    /// otherwise panics
    /// `padding` is the bytes of padding to add to the transaction
    fn create_random_transaction(
        state: Option<&Self>,
        rng: &mut dyn rand::RngCore,
        padding: u64,
    ) -> <Self::BlockType as Block>::Transaction;
}

/// extra functions required on block to be usable by hotshot-testing
pub trait TestableBlock: Block + std::fmt::Debug {
    /// generate a genesis block
    fn genesis() -> Self;

    fn txn_count(&self) -> u64;
}

/// Dummy implementation of `State` for unit tests
pub mod dummy {
    use super::{tag, Committable, Debug, Hash, Serialize, State, TestableState};
    use crate::{
        data::ViewNumber,
        traits::block_contents::dummy::{DummyBlock, DummyError, DummyTransaction},
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

        fn tag() -> String {
            tag::DUMMY_STATE.to_string()
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

        fn validate_block(&self, _block: &Self::BlockType, _view_number: &Self::Time) -> bool {
            false
        }

        fn append(
            &self,
            _block: &Self::BlockType,
            _view_number: &Self::Time,
        ) -> Result<Self, Self::Error> {
            Ok(Self {
                nonce: self.nonce + 1,
            })
        }

        fn on_commit(&self) {}
    }

    impl TestableState for DummyState {
        fn create_random_transaction(
            _state: Option<&Self>,
            _: &mut dyn rand::RngCore,
            _: u64,
        ) -> DummyTransaction {
            DummyTransaction::Dummy
        }
    }
}
