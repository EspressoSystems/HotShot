//! Abstraction over on-disk storage of node state

use crate::{
    data::{BlockHash, Leaf, LeafHash, QuorumCertificate},
    traits::{BlockContents, State},
};
use async_trait::async_trait;
use futures::Future;
use snafu::Snafu;

/// Errors that can occur in the storage layer.
#[derive(Snafu, Debug)]
#[snafu(visibility(pub))]
pub enum StorageError {
    /// The atomic store encountered an error
    AtomicStore {
        /// The inner error
        source: atomic_store::error::PersistenceError,
    },
    /// An inconsistency was found in the data.
    InconsistencyError {
        /// The description of the inconsistency
        description: String,
    },
}

/// Result for a storage type
pub type StorageResult<T = ()> = std::result::Result<T, StorageError>;

/// Abstraction over on disk persistence of node state
///
/// This should be a cloneable handle to an underlying storage, with each clone pointing to the same
/// underlying storage.
///
/// This trait has been constructed for object saftey over convenience.
#[async_trait]
pub trait Storage<B: BlockContents<N> + 'static, S: State<N, Block = B> + 'static, const N: usize>:
    Clone + Send + Sync
{
    /// Retrieves a block from storage, returning `None` if it could not be found in local storage
    async fn get_block<'b, 'a: 'b>(&'a self, hash: &'b BlockHash<N>) -> StorageResult<Option<B>>;
    /// Retrieves a Quorum Certificate from storage, by the hash of the block it refers to
    async fn get_qc<'b, 'a: 'b>(
        &'a self,
        hash: &'b BlockHash<N>,
    ) -> StorageResult<Option<QuorumCertificate<N>>>;
    /// Retrieves the newest Quorum Certificate
    async fn get_newest_qc(&self) -> StorageResult<Option<QuorumCertificate<N>>>;
    /// Retrieves the Quorum Certificate associated with a particular view number
    async fn get_qc_for_view(&self, view: u64) -> StorageResult<Option<QuorumCertificate<N>>>;
    /// Retrieves a leaf by its hash
    async fn get_leaf<'b, 'a: 'b>(
        &'a self,
        hash: &'b LeafHash<N>,
    ) -> StorageResult<Option<Leaf<B, N>>>;
    /// Retrieves a leaf by the hash of its block
    async fn get_leaf_by_block<'b, 'a: 'b>(
        &'a self,
        hash: &'b BlockHash<N>,
    ) -> StorageResult<Option<Leaf<B, N>>>;
    /// Retrieves a `State`, indexed by the hash of the `Leaf` that created it
    async fn get_state<'b, 'a: 'b>(&'a self, hash: &'b LeafHash<N>) -> StorageResult<Option<S>>;

    /// Calls the given `update_fn` for a list of modifications, then stores these.
    ///
    /// If an error occurs somewhere, the caller can assume that no data is stored at all.
    async fn update<'a, F, FUT>(&'a self, update_fn: F) -> StorageResult
    where
        F: FnOnce(Box<dyn StorageUpdater<'a, B, S, N> + 'a>) -> FUT + Send + 'a,
        FUT: Future<Output = StorageResult> + Send + 'a;

    /// Get the internal state of this storage system.
    ///
    /// This function should only be used for testing, never in production code.
    async fn get_internal_state(&self) -> StorageState<B, S, N>;
}

/// An internal representation of the data stored in a [`Storage`].
///
/// This should only be used for testing, never in production code.
#[derive(Debug, PartialEq)]
pub struct StorageState<B, S, const N: usize> {
    /// A list of all the blocks in the storage, sorted by [`BlockHash`].
    pub blocks: Vec<B>,
    /// A list of all the [`QuorumCertificate`] in the storage, sorted by view_number.
    pub quorum_certificates: Vec<QuorumCertificate<N>>,
    /// A list of all the [`Leaf`] in the storage, sorted by [`LeafHash`].
    pub leafs: Vec<Leaf<B, N>>,
    /// A list of all the states in the storage, storted by [`LeafHash`]
    pub states: Vec<S>,
}

/// Trait to be used with [`Storage`]'s `update` function.
#[async_trait]
pub trait StorageUpdater<
    'a,
    B: BlockContents<N> + 'static,
    S: State<N, Block = B> + 'static,
    const N: usize,
>: Send
{
    /// Inserts a block into storage.
    async fn insert_block(&mut self, hash: BlockHash<N>, block: B) -> StorageResult;
    /// Inserts a Quorum Certificate into the storage. Should reject the QC if it is malformed or
    /// not from a decide stage.
    async fn insert_qc(&mut self, qc: QuorumCertificate<N>) -> StorageResult;
    /// Inserts a leaf.
    async fn insert_leaf(&mut self, leaf: Leaf<B, N>) -> StorageResult;
    /// Inserts a `State`, indexed by the hash of the `Leaf` that created it.
    async fn insert_state(&mut self, state: S, hash: LeafHash<N>) -> StorageResult;
}
