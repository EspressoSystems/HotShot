//! Abstraction over the contents of a block
//!
//! This module provides the [`BlockContents`] trait, which describes the behaviors that a block is
//! expected to have.

use commit::{Committable, Commitment};
use serde::{de::DeserializeOwned, Serialize, Deserialize};

use std::{collections::HashSet, error::Error, fmt::Debug, hash::Hash};

use crate::data::{BlockHash, LeafHash, TransactionHash, Leaf};

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
pub trait BlockContents:
    Serialize + Clone + Debug + Hash + PartialEq + Eq + Send + Sync + Unpin + Committable + DeserializeOwned
{
    /// The error type for this type of block
    type Error: Error + Debug + Send + Sync;

    /// The type of the transitions we are applying
    type Transaction :
        Clone + Serialize + DeserializeOwned + Debug + PartialEq + Eq + Sync + Send + Committable;

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
    use blake3::Hasher;
    use rand::Rng;
    use serde::Deserialize;

    pub use crate::traits::state::dummy::DummyState;

    /// The dummy block
    #[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
    pub struct DummyBlock {
        /// Some dummy data
        nonce: u64,
    }

    impl DummyBlock {
        /// Generate a random `DummyBlock`
        pub fn random() -> Self {
            let x = rand::thread_rng().gen();
            Self { nonce: x }
        }
    }

    /// Dummy error
    #[derive(Debug)]
    pub struct DummyError;

    #[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
    pub enum DummyTransaction {
        Dummy
    }

    impl Committable for DummyTransaction {
        fn commit(&self) -> commit::Commitment<Self> {
            todo!()
        }
    }

    impl std::error::Error for DummyError {}

    impl std::fmt::Display for DummyError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("A bad thing happened")
        }
    }

    impl BlockContents for DummyBlock {
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
            todo!()
        }
    }
}
