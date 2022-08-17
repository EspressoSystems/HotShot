//! Abstraction over on-disk storage of node state
#![allow(missing_docs)]

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    data::{LeafHash, QuorumCertificate, TransactionHash, ViewNumber},
    traits::{BlockContents, State},
};
use async_trait::async_trait;
use snafu::Snafu;

/// Errors that can occur in the storage layer.
#[derive(Snafu, Debug)]
#[snafu(visibility(pub))]
pub enum StorageError {
    /// No genesis view was inserted
    NoGenesisView,
}

/// Result for a storage type
pub type Result<T = ()> = std::result::Result<T, StorageError>;

/// Abstraction over on disk persistence of node state
///
/// This should be a cloneable handle to an underlying storage, with each clone pointing to the same
/// underlying storage.
///
/// This trait has been constructed for object saftey over convenience.
#[async_trait]
pub trait Storage<BLOCK, STATE, const N: usize>: Clone + Send + Sync
where
    BLOCK: BlockContents<N> + 'static,
    STATE: State<N, Block = BLOCK> + 'static,
{
    // required
    async fn append(&self, views: Vec<ViewEntry<BLOCK, STATE, N>>) -> Result;
    async fn cleanup_storage_up_to_view(&self, view: ViewNumber) -> Result<usize>;
    async fn get_anchored_view(&self) -> Result<StoredView<BLOCK, STATE, N>>;

    async fn insert_single_view(&self, view: StoredView<BLOCK, STATE, N>) -> Result {
        self.append(vec![ViewEntry::Success(view)]).await
    }
    // future improvement:
    // async fn get_future_views(&self) -> Vec<FutureView>;
    //     async fn add_transaction(&self, transactions: Transaction) -> TransactionHash;
    //     async fn get_transactions(&self) -> Vec<Transaction>;
    //     async fn get_transaction(&self, hash: TransactionHash) -> Option<Transaction>;
    //     async fn remove_transaction(&self, hash: TransactionHash) -> Option<Transaction>;
}

/// Extra requirements on Storage implementations required for testing
#[async_trait]
pub trait TestableStorage<B, S, const N: usize>: Clone + Send + Sync + Storage<B, S, N>
where
    B: BlockContents<N> + 'static,
    S: State<N, Block = B> + 'static,
{
    /// Create ephemeral storage
    /// Will be deleted/lost immediately after storage is dropped
    /// # Errors
    /// Errors if it is not possible to construct temporary storage.
    fn construct_tmp_storage() -> Result<Self>;

    /// Return the full internal state. This is useful for debugging.
    async fn get_full_state(&self) -> StorageState<B, S, N>;
}

/// An internal representation of the data stored in a [`Storage`].
///
/// This should only be used for testing, never in production code.
#[derive(Debug, PartialEq, Eq)]
pub struct StorageState<BLOCK: BlockContents<N>, STATE: State<N>, const N: usize> {
    pub stored: BTreeMap<ViewNumber, StoredView<BLOCK, STATE, N>>,
    pub failed: BTreeSet<ViewNumber>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum ViewEntry<B: BlockContents<N>, S: State<N>, const N: usize> {
    Success(StoredView<B, S, N>),
    Failed(ViewNumber),
    // future improvement:
    // InProgress(InProgressView),
}

impl<B, S, const N: usize> From<StoredView<B, S, N>> for ViewEntry<B, S, N>
where
    B: BlockContents<N>,
    S: State<N>,
{
    fn from(view: StoredView<B, S, N>) -> Self {
        Self::Success(view)
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct StoredView<B: BlockContents<N>, S: State<N>, const N: usize> {
    pub view_number: ViewNumber,
    pub parent: LeafHash<N>,
    pub qc: QuorumCertificate<N>,
    pub state: S,
    pub append: ViewAppend<B, N>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ViewAppend<B: BlockContents<N>, const N: usize> {
    UnknownParent,
    Block {
        block: B,
        rejected_transactions: BTreeSet<TransactionHash<N>>,
    },
}

impl<B, const N: usize> From<B> for ViewAppend<B, N>
where
    B: BlockContents<N>,
{
    fn from(block: B) -> Self {
        Self::Block {
            block,
            rejected_transactions: BTreeSet::new(),
        }
    }
}
