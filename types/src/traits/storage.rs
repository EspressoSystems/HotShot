//! Abstraction over on-disk storage of node state

use crate::{
    data::{Leaf, QuorumCertificate, ViewNumber},
    message::ConsensusMessage,
    traits::StateContents,
};
use async_trait::async_trait;
use commit::{Commitment, Committable};
use futures::Future;
use serde::{Deserialize, Serialize};
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
pub trait Storage<STATE: StateContents>: Clone + Send + Sync {
    /// Retrieves a block from storage, returning `None` if it could not be found in local storage
    async fn get_block(
        &self,
        hash: &Commitment<STATE::Block>,
    ) -> StorageResult<Option<STATE::Block>>;

    /// Retrieves a Quorum Certificate from storage, by the hash of the block it refers to
    async fn get_qc(
        &self,
        hash: &Commitment<STATE::Block>,
    ) -> StorageResult<Option<QuorumCertificate<STATE>>>;

    /// Retrieves the Quorum Certificate associated with a particular view number
    async fn get_qc_for_view(
        &self,
        view: ViewNumber,
    ) -> StorageResult<Option<QuorumCertificate<STATE>>>;

    /// Retrieves a leaf by its hash
    async fn get_leaf(&self, hash: &Commitment<Leaf<STATE>>) -> StorageResult<Option<Leaf<STATE>>>;

    /// Retrieves a leaf by the hash of its block
    async fn get_leaf_by_block(
        &self,
        hash: &Commitment<STATE::Block>,
    ) -> StorageResult<Option<Leaf<STATE>>>;

    /// Retrieves a `State`, indexed by the hash of the `Leaf` that created it
    async fn get_state(&self, hash: &Commitment<Leaf<STATE>>) -> StorageResult<Option<STATE>>;

    /// Calls the given `update_fn` for a list of modifications, then stores these.
    ///
    /// If an error occurs somewhere, the caller can assume that no data is stored at all.
    // async fn update<'b, F, FUT>(&'b self, update_fn: F) -> StorageResult
    // where
    //     F: FnOnce(Box<dyn StorageUpdater<'_, STATE> + 'b>) -> FUT + Send + 'b,
    //     FUT: Future<Output = StorageResult> + Send + 'b;

    /// Get the internal state of this storage system.
    ///
    /// This function should only be used for testing, never in production code.
    async fn get_internal_state(&self) -> StorageState<STATE>;

    /// Retrieves the newest Quorum Certificate
    async fn get_newest_qc(&self) -> StorageResult<Option<QuorumCertificate<STATE>>>;

    // /// Retrieves the newest Quorum Certificate
    // #[deprecated(note = "Use `locked_qc` or `prepare_qc` instead")]
    // async fn get_newest_qc(&self) -> StorageResult<Option<QuorumCertificate<N>>> {
    //     self.locked_qc().await
    // }

    // /// Retrieves the latest Quorum Certificate that was voted on in the hotstuff prepare phase
    // async fn prepare_qc(&self) -> StorageResult<Option<QuorumCertificate<N>>>;

    // /// Retrieves the latest Quorum Certificate that was voted on in the hotstuff commit phase
    // async fn locked_qc(&self) -> StorageResult<Option<QuorumCertificate<N>>>;

    // /// Retrieves the active hotstuff phases.
    // async fn active_hotstuff_phases(&self) -> StorageResult<Vec<View<B, S, N>>>;

    // /// Get the hotstuff phase by the given [`ViewNumber`]
    // async fn hotstuff_phase(&self, view_number: ViewNumber)
    //     -> StorageResult<Option<View<B, S, N>>>;
}

/// Extra requirements on Storage implementations required for testing
pub trait TestableStorage<S: StateContents + 'static>: Clone + Send + Sync + Storage<S> {
    /// Create ephemeral storage
    /// Will be deleted/lost immediately after storage is dropped
    /// # Errors
    /// Errors if it is not possible to construct temporary storage.
    fn construct_tmp_storage() -> StorageResult<Self>;
}

/// An internal representation of the data stored in a [`Storage`].
///
/// This should only be used for testing, never in production code.
#[derive(Debug, PartialEq, Eq)]
pub struct StorageState<STATE: StateContents> {
    /// A list of all the blocks in the storage, sorted by [`BlockHash`].
    pub blocks: Vec<STATE::Block>,
    /// A list of all the [`QuorumCertificate`] in the storage, sorted by view_number.
    pub quorum_certificates: Vec<QuorumCertificate<STATE>>,
    /// A list of all the [`Leaf`] in the storage, sorted by [`LeafHash`].
    pub leafs: Vec<Leaf<STATE>>,
    /// A list of all the states in the storage, storted by [`LeafHash`]
    pub states: Vec<STATE>,
}

/// Trait to be used with [`Storage`]'s `update` function.
#[async_trait]
pub trait StorageUpdater<'a, STATE: StateContents + 'static>: Send {
    /// Inserts a block into storage.
    async fn insert_block(
        &mut self,
        hash: Commitment<STATE::Block>,
        block: STATE::Block,
    ) -> StorageResult;
    /// Inserts a Quorum Certificate into the storage. Should reject the QC if it is malformed or
    /// not from a decide stage.
    async fn insert_qc(&mut self, qc: QuorumCertificate<STATE>) -> StorageResult;
    /// Inserts a leaf.
    async fn insert_leaf(&mut self, leaf: Leaf<STATE>) -> StorageResult;
    /// Inserts a `State`, indexed by the hash of the `Leaf` that created it.
    async fn insert_state(&mut self, state: STATE, hash: Commitment<Leaf<STATE>>) -> StorageResult;
}

/// State of a single hotshot-consensus  view
#[derive(Serialize, Deserialize, Clone)]
pub struct View<STATE: StateContents> {
    /// The view number of this phase.
    pub view_number: ViewNumber,

    /// All messages that have been received on this phase.
    ///
    /// Some things that can be derived from this and are not stored independently
    /// - Prepare leader `high_qc`
    /// - Precommit leader `Prepare` block
    /// - Commit leader `PreCommit` block
    /// - Decide leader `Commit` block
    #[serde(deserialize_with = "<Vec<ConsensusMessage<STATE>> as Deserialize>::deserialize")]
    pub messages: Vec<ConsensusMessage<STATE>>,

    /// if `true` this phase is done and will not run any more updates
    pub done: bool,
}
