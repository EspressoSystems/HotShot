//! Abstraction over on-disk storage of node state
#![allow(missing_docs)]

use super::{node_implementation::NodeTypes, signature_key::EncodedPublicKey};
use crate::{
    data::{Leaf, QuorumCertificate, ViewNumber},
    traits::Block,
};
use async_trait::async_trait;
use commit::Commitment;
use derivative::Derivative;
use snafu::Snafu;
use std::collections::{BTreeMap, BTreeSet};

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
pub trait Storage<TYPES>: Clone + Send + Sync + Sized
where
    TYPES: NodeTypes + 'static,
{
    /// Append the list of views to this storage
    async fn append(&self, views: Vec<ViewEntry<TYPES>>) -> Result;
    /// Cleans up the storage up to the given view. The given view number will still persist in this storage afterwards.
    async fn cleanup_storage_up_to_view(&self, view: ViewNumber) -> Result<usize>;
    /// Get the latest anchored view
    async fn get_anchored_view(&self) -> Result<StoredView<TYPES>>;
    /// Commit this storage.
    async fn commit(&self) -> Result;

    /// Insert a single view. Shorthand for
    /// ```rust,ignore
    /// storage.append(vec![ViewEntry::Success(view)]).await
    /// ```
    async fn append_single_view(&self, view: StoredView<TYPES>) -> Result {
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
pub trait TestableStorage<TYPES>: Clone + Send + Sync + Storage<TYPES>
where
    TYPES: NodeTypes + 'static,
{
    /// Create ephemeral storage
    /// Will be deleted/lost immediately after storage is dropped
    /// # Errors
    /// Errors if it is not possible to construct temporary storage.
    fn construct_tmp_storage() -> Result<Self>;

    /// Return the full internal state. This is useful for debugging.
    async fn get_full_state(&self) -> StorageState<TYPES>;
}

/// An internal representation of the data stored in a [`Storage`].
///
/// This should only be used for testing, never in production code.
#[derive(Debug, PartialEq, Eq)]
pub struct StorageState<TYPES: NodeTypes> {
    /// The views that have been successful
    pub stored: BTreeMap<ViewNumber, StoredView<TYPES>>,
    /// The views that have failed
    pub failed: BTreeSet<ViewNumber>,
}

/// An entry to `Storage::append`. This makes it possible to commit both succeeded and failed views at the same time
#[derive(Debug, PartialEq, Eq)]
pub enum ViewEntry<TYPES>
where
    TYPES: NodeTypes,
{
    /// A succeeded view
    Success(StoredView<TYPES>),
    /// A failed view
    Failed(ViewNumber),
    // future improvement:
    // InProgress(InProgressView),
}

impl<TYPES> From<StoredView<TYPES>> for ViewEntry<TYPES>
where
    TYPES: NodeTypes,
{
    fn from(view: StoredView<TYPES>) -> Self {
        Self::Success(view)
    }
}

/// A view stored in the [`Storage`]
#[derive(Derivative, Debug)]
#[derivative(PartialEq, Eq, Clone)]
pub struct StoredView<TYPES: NodeTypes> {
    /// The view number of this view
    pub time: TYPES::Time,
    /// The parent of this view
    pub parent: Commitment<Leaf<TYPES>>,
    /// The justify QC of this view. See the hotstuff paper for more information on this.
    pub justify_qc: QuorumCertificate<TYPES>,
    /// The state of this view
    pub state: TYPES::StateType,
    /// The history of how this view came to be
    pub append: ViewAppend<TYPES::BlockType>,
    /// transactions rejected in this view
    pub rejected: Vec<TYPES::Transaction>,
    /// the timestamp this view was recv-ed in nanonseconds
    #[derivative(PartialEq = "ignore")]
    pub timestamp: i128,
    /// the proposer id
    #[derivative(PartialEq = "ignore")]
    pub proposer_id: EncodedPublicKey,
}

impl<TYPES> StoredView<TYPES>
where
    TYPES: NodeTypes,
{
    /// Create a new `StoredView` from the given QC, Block and State.
    ///
    /// Note that this will set the `parent` to `LeafHash::default()`, so this will not have a parent.
    pub fn from_qc_block_and_state(
        qc: QuorumCertificate<TYPES>,
        block: TYPES::BlockType,
        state: TYPES::StateType,
        parent_commitment: Commitment<Leaf<TYPES>>,
        rejected: Vec<<TYPES::BlockType as Block>::Transaction>,
        proposer_id: EncodedPublicKey,
    ) -> Self {
        Self {
            append: ViewAppend::Block { block },
            time: qc.time.clone(),
            parent: parent_commitment,
            justify_qc: qc,
            state,
            rejected,
            timestamp: time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            proposer_id,
        }
    }
}

/// Indicates how a view came to be
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ViewAppend<BLOCK: Block> {
    /// The view was created by appending a block to the previous view
    Block {
        /// The block that was appended
        block: BLOCK,
    },
}

impl<BLOCK: Block> ViewAppend<BLOCK> {
    /// Get the block deltas from this append
    pub fn into_deltas(self) -> BLOCK {
        match self {
            Self::Block { block, .. } => block,
        }
    }
}

impl<BLOCK: Block> From<BLOCK> for ViewAppend<BLOCK> {
    fn from(block: BLOCK) -> Self {
        Self::Block { block }
    }
}
