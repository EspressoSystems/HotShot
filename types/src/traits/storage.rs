//! Abstraction over on-disk storage of node state
#![allow(missing_docs)]

use crate::{
    data::{Leaf, QuorumCertificate, TxnCommitment, ViewNumber},
    traits::{BlockContents, StateContents},
};
use async_trait::async_trait;
use commit::Commitment;
use derivative::Derivative;
use snafu::Snafu;
use std::collections::{BTreeMap, BTreeSet};

use super::signature_key::EncodedPublicKey;

/// Errors that can occur in the storage layer.
#[derive(Clone, Debug, Snafu)]
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
pub trait Storage<STATE>: Clone + Send + Sync
where
    STATE: StateContents + 'static,
{
    /// Append the list of views to this storage
    async fn append(&self, views: Vec<ViewEntry<STATE>>) -> Result;
    /// Cleans up the storage up to the given view. The given view number will still persist in this storage afterwards.
    async fn cleanup_storage_up_to_view(&self, view: ViewNumber) -> Result<usize>;
    /// Get the latest anchored view
    async fn get_anchored_view(&self) -> Result<StoredView<STATE>>;
    /// Commit this storage.
    async fn commit(&self) -> Result;

    /// Insert a single view. Shorthand for
    /// ```rust,ignore
    /// storage.append(vec![ViewEntry::Success(view)]).await
    /// ```
    async fn append_single_view(&self, view: StoredView<STATE>) -> Result {
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
pub trait TestableStorage<STATE>: Clone + Send + Sync + Storage<STATE>
where
    STATE: StateContents + 'static,
{
    /// Create ephemeral storage
    /// Will be deleted/lost immediately after storage is dropped
    /// # Errors
    /// Errors if it is not possible to construct temporary storage.
    fn construct_tmp_storage() -> Result<Self>;

    /// Return the full internal state. This is useful for debugging.
    async fn get_full_state(&self) -> StorageState<STATE>;
}

/// An internal representation of the data stored in a [`Storage`].
///
/// This should only be used for testing, never in production code.
#[derive(Debug, PartialEq, Eq)]
pub struct StorageState<STATE: StateContents> {
    /// The views that have been successful
    pub stored: BTreeMap<ViewNumber, StoredView<STATE>>,
    /// The views that have failed
    pub failed: BTreeSet<ViewNumber>,
}

/// An entry to `Storage::append`. This makes it possible to commit both succeeded and failed views at the same time
#[derive(Debug, PartialEq, Eq)]
pub enum ViewEntry<STATE>
where
    STATE: StateContents,
{
    /// A succeeded view
    Success(StoredView<STATE>),
    /// A failed view
    Failed(ViewNumber),
    // future improvement:
    // InProgress(InProgressView),
}

impl<STATE> From<StoredView<STATE>> for ViewEntry<STATE>
where
    STATE: StateContents,
{
    fn from(view: StoredView<STATE>) -> Self {
        Self::Success(view)
    }
}

/// A view stored in the [`Storage`]
#[derive(Derivative, Debug)]
#[derivative(PartialEq, Eq, Clone)]
pub struct StoredView<STATE: StateContents> {
    /// The view number of this view
    pub view_number: ViewNumber,
    /// The parent of this view
    pub parent: Commitment<Leaf<STATE>>,
    /// The justify QC of this view. See the hotstuff paper for more information on this.
    pub justify_qc: QuorumCertificate<STATE>,
    /// The state of this view
    pub state: STATE,
    /// The history of how this view came to be
    pub append: ViewAppend<<STATE as StateContents>::Block>,
    /// transactions rejected in this view
    pub rejected: Vec<TxnCommitment<STATE>>,
    /// the timestamp this view was recv-ed in nanonseconds
    #[derivative(PartialEq="ignore")]
    pub timestamp: i128,
    /// the proposer id
    #[derivative(PartialEq="ignore")]
    pub proposer_id: EncodedPublicKey,
}

impl<STATE> StoredView<STATE>
where
    STATE: StateContents,
{
    /// Create a new `StoredView` from the given QC, Block and State.
    ///
    /// Note that this will set the `parent` to `LeafHash::default()`, so this will not have a parent.
    pub fn from_qc_block_and_state(
        qc: QuorumCertificate<STATE>,
        block: <STATE as StateContents>::Block,
        state: STATE,
        parent_commitment: Commitment<Leaf<STATE>>,
        rejected: Vec<TxnCommitment<STATE>>,
        proposer_id: EncodedPublicKey
    ) -> Self {
        Self {
            append: ViewAppend::Block { block },
            view_number: qc.view_number,
            parent: parent_commitment,
            justify_qc: qc,
            state,
            rejected,
            timestamp: time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            proposer_id
        }
    }
}

/// Indicates how a view came to be
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ViewAppend<BLOCK: BlockContents> {
    /// The view was created by appending a block to the previous view
    Block {
        /// The block that was appended
        block: BLOCK,
    },
}

impl<B: BlockContents> ViewAppend<B> {
    /// Get the block deltas from this append
    pub fn into_deltas(self) -> B {
        match self {
            Self::Block { block, .. } => block,
        }
    }
}

impl<B: BlockContents> From<B> for ViewAppend<B> {
    fn from(block: B) -> Self {
        Self::Block { block }
    }
}
