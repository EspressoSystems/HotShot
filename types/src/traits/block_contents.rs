//! Abstraction over the contents of a block
//!
//! This module provides the [`BlockContents`] trait, which describes the behaviors that a block is
//! expected to have.

use commit::{Commitment, Committable};
use serde::{de::DeserializeOwned, Serialize};

use std::{collections::HashSet, error::Error, fmt::Debug, hash::Hash};

/// Abstraction over the contents of a block
///
/// This trait encapsulates the behaviors that a block must have in order to be used by consensus:
///   * Must have a predefined error type ([`BlockContents::Error`])
///   * Must have a transaction type that can be compared for equality, serialized and serialized,
///     sent between threads, and can have a hash produced of it
///     ([`hash_transaction`](BlockContents::hash_transaction))
///   * Must be able to be produced incrementally by appending transactions
///     ([`add_transaction_raw`](BlockContents::add_transaction_raw))
///   * Must be hashable ([`hash`](BlockContents::hash))
pub trait Block:
    Serialize + Clone + Debug + Hash + PartialEq + Eq + Send + Sync + Committable + DeserializeOwned
{
    /// The error type for this type of block
    type Error: Error + Debug + Send + Sync;

    /// The type of the transitions we are applying
    type Transaction: Clone
        + Serialize
        + DeserializeOwned
        + Debug
        + PartialEq
        + Eq
        + Sync
        + Send
        + Committable;

    /// Attempts to add a transaction, returning an Error if it would result in a structurally
    /// invalid block
    ///
    /// # Errors
    ///
    /// Should return an error if this transaction leads to an invalid block
    fn add_transaction_raw(&self, tx: &Self::Transaction)
        -> std::result::Result<Self, Self::Error>;

    /// returns hashes of all the transactions in this block
    fn contained_transactions(&self) -> HashSet<Commitment<Self::Transaction>>;
}

/// Dummy implementation of `BlockContents` for unit tests
pub mod dummy {
    #[allow(clippy::wildcard_imports)]
    use super::*;
    use rand::Rng;
    use serde::Deserialize;

    pub use crate::traits::state::dummy::DummyState;

    /// The dummy block
    #[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
    pub struct DummyBlock {
        /// Some dummy data
        pub nonce: u64,
    }

    impl DummyBlock {
        /// Generate a random `DummyBlock`
        pub fn random(rng: &mut dyn rand::RngCore) -> Self {
            Self { nonce: rng.gen() }
        }
    }

    /// Dummy error
    #[derive(Debug)]
    pub struct DummyError;

    /// dummy transaction. No functionality
    #[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
    pub enum DummyTransaction {
        /// the only variant. Dummy.
        Dummy,
    }

    impl Committable for DummyTransaction {
        fn commit(&self) -> commit::Commitment<Self> {
            commit::RawCommitmentBuilder::new("Dummy Block Comm")
                .u64_field("Dummy Field", 0)
                .finalize()
        }
    }

    impl std::error::Error for DummyError {}

    impl std::fmt::Display for DummyError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("A bad thing happened")
        }
    }

    impl Block for DummyBlock {
        type Error = DummyError;

        type Transaction = DummyTransaction;

        fn add_transaction_raw(
            &self,
            _tx: &Self::Transaction,
        ) -> std::result::Result<Self, Self::Error> {
            Err(DummyError)
        }

        fn contained_transactions(&self) -> HashSet<Commitment<Self::Transaction>> {
            HashSet::new()
        }
    }

    impl Committable for DummyBlock {
        fn commit(&self) -> commit::Commitment<Self> {
            commit::RawCommitmentBuilder::new("Dummy Block Comm")
                .u64_field("Nonce", self.nonce)
                .finalize()
        }
    }
}
