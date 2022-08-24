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
    /// Append the list of views to this storage
    async fn append(&self, views: Vec<ViewEntry<BLOCK, STATE, N>>) -> Result;
    /// Cleans up the storage up to the given view. The given view number will still persist in this storage afterwards.
    async fn cleanup_storage_up_to_view(&self, view: ViewNumber) -> Result<usize>;
    /// Get the latest anchored view
    async fn get_anchored_view(&self) -> Result<StoredView<BLOCK, STATE, N>>;
    /// Commit this storage.
    async fn commit(&self) -> Result;

    /// Insert a single view. Shorthand for
    /// ```rust,ignore
    /// storage.append(vec![ViewEntry::Success(view)]).await
    /// ```
    async fn append_single_view(&self, view: StoredView<BLOCK, STATE, N>) -> Result {
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
pub trait TestableStorage<BLOCK, STATE, const N: usize>:
    Clone + Send + Sync + Storage<BLOCK, STATE, N>
where
    BLOCK: BlockContents<N> + 'static,
    STATE: State<N, Block = BLOCK> + 'static,
{
    /// Create ephemeral storage
    /// Will be deleted/lost immediately after storage is dropped
    /// # Errors
    /// Errors if it is not possible to construct temporary storage.
    fn construct_tmp_storage(block: BLOCK, storage: STATE) -> Result<Self>;

    /// Return the full internal state. This is useful for debugging.
    async fn get_full_state(&self) -> StorageState<BLOCK, STATE, N>;
}

/// An internal representation of the data stored in a [`Storage`].
///
/// This should only be used for testing, never in production code.
#[derive(Debug, PartialEq, Eq)]
pub struct StorageState<BLOCK: BlockContents<N>, STATE: State<N>, const N: usize> {
    /// The views that have been successful
    pub stored: BTreeMap<ViewNumber, StoredView<BLOCK, STATE, N>>,
    /// The views that have failed
    pub failed: BTreeSet<ViewNumber>,
}

/// An entry to `Storage::append`. This makes it possible to commit both succeeded and failed views at the same time
#[derive(Debug, PartialEq, Eq)]
pub enum ViewEntry<BLOCK, STATE, const N: usize>
where
    BLOCK: BlockContents<N>,
    STATE: State<N>,
{
    /// A succeeded view
    Success(StoredView<BLOCK, STATE, N>),
    /// A failed view
    Failed(ViewNumber),
    // future improvement:
    // InProgress(InProgressView),
}

impl<BLOCK, STATE, const N: usize> From<StoredView<BLOCK, STATE, N>> for ViewEntry<BLOCK, STATE, N>
where
    BLOCK: BlockContents<N>,
    STATE: State<N>,
{
    fn from(view: StoredView<BLOCK, STATE, N>) -> Self {
        Self::Success(view)
    }
}

/// A view stored in the [`Storage`]
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct StoredView<B: BlockContents<N>, S: State<N>, const N: usize> {
    /// The view number of this view
    pub view_number: ViewNumber,
    /// The parent of this view
    pub parent: LeafHash<N>,
    /// The justify QC of this view. See the hotstuff paper for more information on this.
    pub justify_qc: QuorumCertificate<N>,
    /// The state of this view
    pub state: S,
    /// The history of how this view came to be
    pub append: ViewAppend<B, N>,
}

impl<BLOCK, STATE, const N: usize> StoredView<BLOCK, STATE, N>
where
    BLOCK: BlockContents<N>,
    STATE: State<N>,
{
    /// Create a new `StoredView` from the given QC, Block and State.
    ///
    /// Note that this will set the `parent` to `LeafHash::default()`, so this will not have a parent.
    pub fn from_qc_block_and_state(qc: QuorumCertificate<N>, block: BLOCK, state: STATE) -> Self {
        Self {
            append: ViewAppend::Block {
                block,
                rejected_transactions: BTreeSet::new(),
            },
            view_number: qc.view_number,
            parent: LeafHash::default(),
            justify_qc: qc,
            state,
        }
    }
}

/// Indicates how a view came to be
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ViewAppend<B: BlockContents<N>, const N: usize> {
    /// The view was created by appending a block to the previous view
    Block {
        /// The block that was appended
        block: B,
        /// A set of transactions that were rejected while trying to establish the above block
        rejected_transactions: BTreeSet<TransactionHash<N>>,
    },
}

impl<B, const N: usize> ViewAppend<B, N>
where
    B: BlockContents<N>,
{
    /// Get the block deltas from this append
    pub fn into_deltas(self) -> B {
        match self {
            Self::Block { block, .. } => block,
        }
    }
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
