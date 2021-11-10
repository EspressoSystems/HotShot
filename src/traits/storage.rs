//! Abstraction over on-disk storage of node state

use futures::future::BoxFuture;

use crate::{
    data::{BlockHash, Leaf},
    BlockContents, QuorumCertificate,
};

use super::state::State;

pub mod memory_storage;

/// Result for a storage type
pub enum StorageResult<T> {
    /// The item was located in storage
    Some(T),
    /// The item was not found
    None,
    /// An error occurred
    Err(Box<dyn std::error::Error + Send + Sync + 'static>),
}

impl<T> StorageResult<T> {
    /// Returns true if the result is a `Some`
    pub fn is_some(&self) -> bool {
        matches!(self, StorageResult::Some(_))
    }
    /// Returns true if the result is a `None`
    pub fn is_none(&self) -> bool {
        matches!(self, StorageResult::None)
    }
    /// Returns true if the result is a `Err`
    pub fn is_err(&self) -> bool {
        matches!(self, StorageResult::Err(_))
    }
    /// Converts to an option
    pub fn ok(self) -> Option<T> {
        match self {
            StorageResult::Some(x) => Some(x),
            StorageResult::None | StorageResult::Err(_) => None,
        }
    }
    /// Unwraps a `Some` value, panicking otherwise, this is a testing only function
    #[cfg(test)]
    #[allow(clippy::panic, clippy::missing_panics_doc)]
    pub fn unwrap(self) -> T {
        if let StorageResult::Some(x) = self {
            x
        } else {
            panic!("Unwrapped an empty/error value!");
        }
    }
}

/// Abstraction over on disk persistence of node state;
///
/// This should be a cloneable handle to an underlying storage, with each clone pointing to the same
/// underlying storage.
///
/// This trait has been constructed for object saftey over convenience.
pub trait Storage<B: BlockContents<N> + 'static, S: State<N, Block = B> + 'static, const N: usize>:
    Clone + Send + Sync
{
    /// Retrieves a block from storage, returning `None` if it could not be found in local storage
    fn get_block<'b, 'a: 'b>(&'a self, hash: &'b BlockHash<N>) -> BoxFuture<'b, StorageResult<B>>;
    /// Inserts a block into storage
    fn insert_block(&self, hash: BlockHash<N>, block: B) -> BoxFuture<'_, StorageResult<()>>;
    /// Retrieves a Quorum Certificate from storage, by the hash of the block it refers to
    fn get_qc<'b, 'a: 'b>(
        &'a self,
        hash: &'b BlockHash<N>,
    ) -> BoxFuture<'b, StorageResult<QuorumCertificate<N>>>;
    /// Retrieves the Quorum Certificate associated with a particular view number
    fn get_qc_for_view(&self, view: u64) -> BoxFuture<'_, StorageResult<QuorumCertificate<N>>>;
    /// Inserts a Quorum Certificate into the storage. Should reject the QC if it is malformed or
    /// not from a decide stage
    fn insert_qc(&self, qc: QuorumCertificate<N>) -> BoxFuture<'_, StorageResult<()>>;
    /// Retrieves a leaf by its hash
    fn get_leaf<'b, 'a: 'b>(
        &'a self,
        hash: &'b BlockHash<N>,
    ) -> BoxFuture<'b, StorageResult<Leaf<B, N>>>;
    /// Retrieves a leaf by the hash of its block
    fn get_leaf_by_block<'b, 'a: 'b>(
        &'a self,
        hash: &'b BlockHash<N>,
    ) -> BoxFuture<'b, StorageResult<Leaf<B, N>>>;
    /// Inserts a leaf
    fn insert_leaf(&self, leaf: Leaf<B, N>) -> BoxFuture<'_, StorageResult<()>>;
    /// Inserts a `State`, indexed by the hash of the `Leaf` that created it
    fn insert_state(&self, state: S, hash: BlockHash<N>) -> BoxFuture<'_, StorageResult<()>>;
    /// Retrieves a `State`, indexed by the hash of the `Leaf` that created it
    fn get_state<'b, 'a: 'b>(&'a self, hash: &'b BlockHash<N>) -> BoxFuture<'_, StorageResult<S>>;
}
