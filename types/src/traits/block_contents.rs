//! Abstraction over the contents of a block
//!
//! This module provides the [`BlockContents`] trait, which describes the behaviors that a block is
//! expected to have.

use serde::{de::DeserializeOwned, Serialize};

use std::{error::Error, fmt::Debug, hash::Hash};

use crate::data::{BlockHash, LeafHash, TransactionHash};

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
pub trait BlockContents<const N: usize>:
    Serialize + DeserializeOwned + Clone + Debug + Hash + PartialEq + Eq + Send + Sync + Unpin
{
    /// The error type for this type of block
    type Error: Error + Debug + Send + Sync;

    /// The type of the transitions we are applying
    type Transaction: Transaction<N>;

    /// Attempts to add a transaction, returning an Error if it would result in a structurally
    /// invalid block
    ///
    /// # Errors
    ///
    /// Should return an error if this transaction leads to an invalid block
    fn add_transaction_raw(&self, tx: &Self::Transaction)
        -> std::result::Result<Self, Self::Error>;

    /// Produces a hash for the contents of the block
    fn hash(&self) -> BlockHash<N>;
    /// Produces a hash for a transaction
    ///
    /// TODO: Abstract out into transaction trait
    fn hash_transaction(tx: &Self::Transaction) -> TransactionHash<N>;
    /// Produces a hash for an arbitrary sequence of bytes
    ///
    /// Used to produce hashes for internal `HotShot` control structures
    fn hash_leaf(bytes: &[u8]) -> LeafHash<N>;
}

/// A transaction trait for [`BlockContents`]
pub trait Transaction<const N: usize>:
    Clone + Serialize + DeserializeOwned + Debug + Hash + PartialEq + Eq + Sync + Send
{
}

impl<const N: usize> Transaction<N> for () {}

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

    impl std::error::Error for DummyError {}

    impl std::fmt::Display for DummyError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("A bad thing happened")
        }
    }

    impl BlockContents<32> for DummyBlock {
        type Error = DummyError;

        type Transaction = ();

        fn add_transaction_raw(
            &self,
            _tx: &Self::Transaction,
        ) -> std::result::Result<Self, Self::Error> {
            Err(DummyError)
        }
        fn hash(&self) -> BlockHash<32> {
            let mut hasher = Hasher::new();
            hasher.update(&self.nonce.to_le_bytes());
            let x = *hasher.finalize().as_bytes();
            x.into()
        }

        fn hash_transaction(_tx: &Self::Transaction) -> TransactionHash<32> {
            let mut hasher = Hasher::new();
            hasher.update(&[1_u8]);
            let x = *hasher.finalize().as_bytes();
            x.into()
        }

        fn hash_leaf(bytes: &[u8]) -> LeafHash<32> {
            let mut hasher = Hasher::new();
            hasher.update(bytes);
            let x = *hasher.finalize().as_bytes();
            x.into()
        }
    }
}
