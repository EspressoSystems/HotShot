//! Abstraction over on-disk storage of node state

use futures::future::BoxFuture;

use crate::{
    data::{BlockHash, Leaf, LeafHash, QuorumCertificate},
    traits::{BlockContents, State},
};

/// Result for a storage type
pub type StorageResult<T = ()> =
    std::result::Result<T, Box<dyn std::error::Error + Send + Sync + 'static>>;

/// Abstraction over on disk persistence of node state
///
/// This should be a cloneable handle to an underlying storage, with each clone pointing to the same
/// underlying storage.
///
/// This trait has been constructed for object saftey over convenience.
pub trait Storage<B: BlockContents<N> + 'static, S: State<N, Block = B> + 'static, const N: usize>:
    Clone + Send + Sync
{
    /// Retrieves a block from storage, returning `None` if it could not be found in local storage
    fn get_block<'b, 'a: 'b>(
        &'a self,
        hash: &'b BlockHash<N>,
    ) -> BoxFuture<'b, StorageResult<Option<B>>>;
    /// Inserts a block into storage. Make sure to call [`commit`] after all data is inserted.
    fn insert_block(&self, hash: BlockHash<N>, block: B) -> BoxFuture<'_, StorageResult>;
    /// Retrieves a Quorum Certificate from storage, by the hash of the block it refers to
    fn get_qc<'b, 'a: 'b>(
        &'a self,
        hash: &'b BlockHash<N>,
    ) -> BoxFuture<'b, StorageResult<Option<QuorumCertificate<N>>>>;
    /// Retrieves the Quorum Certificate associated with a particular view number
    fn get_qc_for_view(
        &self,
        view: u64,
    ) -> BoxFuture<'_, StorageResult<Option<QuorumCertificate<N>>>>;
    /// Inserts a Quorum Certificate into the storage. Should reject the QC if it is malformed or
    /// not from a decide stage. Make sure to call [`commit`] after all data is inserted.
    fn insert_qc(&self, qc: QuorumCertificate<N>) -> BoxFuture<'_, StorageResult>;
    /// Retrieves a leaf by its hash
    fn get_leaf<'b, 'a: 'b>(
        &'a self,
        hash: &'b LeafHash<N>,
    ) -> BoxFuture<'b, StorageResult<Option<Leaf<B, N>>>>;
    /// Retrieves a leaf by the hash of its block
    fn get_leaf_by_block<'b, 'a: 'b>(
        &'a self,
        hash: &'b BlockHash<N>,
    ) -> BoxFuture<'b, StorageResult<Option<Leaf<B, N>>>>;
    /// Inserts a leaf. Make sure to call [`commit`] after all data is inserted.
    fn insert_leaf(&self, leaf: Leaf<B, N>) -> BoxFuture<'_, StorageResult>;
    /// Inserts a `State`, indexed by the hash of the `Leaf` that created it. Make sure to call [`commit`] after all data is inserted.
    fn insert_state(&self, state: S, hash: LeafHash<N>) -> BoxFuture<'_, StorageResult>;
    /// Retrieves a `State`, indexed by the hash of the `Leaf` that created it
    fn get_state<'b, 'a: 'b>(
        &'a self,
        hash: &'b LeafHash<N>,
    ) -> BoxFuture<'b, StorageResult<Option<S>>>;

    /// Commit the changes in the Storage. This should be called every time one or multiple of the following functions are called:
    /// - `insert_block`
    /// - `insert_qc`
    /// - `insert_leaf`
    /// - `insert_state`
    fn commit(&self) -> BoxFuture<'_, StorageResult>;
}
