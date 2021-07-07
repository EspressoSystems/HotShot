use serde::{de::DeserializeOwned, Serialize};

use std::{error::Error, fmt::Debug, hash::Hash};

use crate::data::BlockHash;

/// The block trait
pub trait BlockContents<const N: usize>:
    Serialize + DeserializeOwned + Clone + Debug + Hash + PartialEq + Eq + Send + Sync
{
    /// The type of the state machine we are applying transitions to
    type State: Clone + Send + Sync;
    /// The type of the transitions we are applying
    type Transaction: Clone
        + Serialize
        + DeserializeOwned
        + Debug
        + Hash
        + PartialEq
        + Eq
        + Sync
        + Send;
    /// The error type for this state machine
    type Error: Error + Debug + Send + Sync;

    /// Creates a new block, currently devoid of transactions, given the current state.
    ///
    /// Allows the new block to reference any data from the state that its in.
    fn next_block(state: &Self::State) -> Self;

    /// Attempts to add a transaction, returning an Error if not compatible with the current state
    ///
    /// # Errors
    ///
    /// Should return an error if this transaction leads to an invalid block
    fn add_transaction(
        &self,
        state: &Self::State,
        tx: &Self::Transaction,
    ) -> std::result::Result<Self, Self::Error>;
    /// ensures that the block is append able to the current state
    fn validate_block(&self, state: &Self::State) -> bool;
    /// Appends the block to the state
    ///
    /// # Errors
    ///
    /// Should produce an error if this block leads to an invalid state
    fn append_to(&self, state: &Self::State) -> std::result::Result<Self::State, Self::Error>;
    /// Produces a hash for the contents of the block
    fn hash(&self) -> BlockHash<N>;
    /// Produces a hash for a transaction
    ///
    /// TODO: Abstract out into transaction trait
    fn hash_transaction(tx: &Self::Transaction) -> BlockHash<N>;
    /// Produces a hash for an arbitrary sequence of bytes
    ///
    /// Used to produce hashes for internal `HotStuff` control structures
    fn hash_bytes(bytes: &[u8]) -> BlockHash<N>;
}
