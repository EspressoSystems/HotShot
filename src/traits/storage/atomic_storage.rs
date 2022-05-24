//! On-disk storage of node state. Based on [`atomic_store`](https://github.com/EspressoSystems/atomicstore).

mod dual_key_value_store;
mod hash_map_store;

use self::dual_key_value_store::DualKeyValueStore;
use self::hash_map_store::HashMapStore;
use crate::{
    data::{BlockHash, Leaf, LeafHash},
    traits::{BlockContents, State},
    QuorumCertificate,
};
use async_std::sync::Mutex;
use async_trait::async_trait;
use atomic_store::{AtomicStore, AtomicStoreLoader};
use futures::Future;
use phaselock_types::{
    data::ViewNumber,
    traits::storage::{AtomicStoreSnafu, Storage, StorageResult, StorageState, StorageUpdater},
};
use serde::{de::DeserializeOwned, Serialize};
use snafu::ResultExt;
use std::{path::Path, sync::Arc};
use tracing::{instrument, trace};

/// Inner state of an atomic storage
struct AtomicStorageInner<Block, State, const N: usize>
where
    Block: BlockContents<N> + DeserializeOwned + Serialize,
    State: DeserializeOwned + Serialize,
{
    /// The atomic store loader
    atomic_store: Mutex<AtomicStore>,

    /// The Blocks stored by this [`AtomicStorage`]
    blocks: HashMapStore<BlockHash<N>, Block>,

    /// The [`QuorumCertificate`]s stored by this [`AtomicStorage`]
    qcs: DualKeyValueStore<QuorumCertificate<N>>,

    /// The [`Leaf`s stored by this [`AtomicStorage`]
    ///
    /// In order to maintain the struct constraints, this list must be append only. Once a QC is
    /// inserted, it index _must not_ change
    leaves: DualKeyValueStore<Leaf<Block, N>>,

    /// The store of states
    states: HashMapStore<LeafHash<N>, State>,
}

/// Persistent [`Storage`] implementation, based upon [`atomic_store`].
#[derive(Clone)]
pub struct AtomicStorage<Block, State, const N: usize>
where
    Block: BlockContents<N> + DeserializeOwned + Serialize,
    State: DeserializeOwned + Serialize,
{
    /// Inner state of the atomic storage
    inner: Arc<AtomicStorageInner<Block, State, N>>,
}

impl<Block, State, const N: usize> Default for AtomicStorage<Block, State, N>
where
    Block: BlockContents<N> + DeserializeOwned + Serialize + Clone,
    State: DeserializeOwned + Serialize + Clone,
{
    fn default() -> Self {
        // TODO use result
        // let tempfile = NamedTempFile::new().unwrap();
        let file = tempfile::tempdir().unwrap();
        let path = file.path();
        println!("Using store in {:?}", path);
        Self::open(path).unwrap()
    }
}

impl<Block, State, const N: usize> AtomicStorage<Block, State, N>
where
    Block: BlockContents<N> + DeserializeOwned + Serialize + Clone,
    State: DeserializeOwned + Serialize + Clone,
{
    /// Creates an atomic storage at a given path. If files exist, will back up existing directory before creating.
    ///
    /// # Errors
    ///
    /// Returns the underlying errors that the following types can throw:
    /// - [`atomic_store::AtomicStoreLoader`]
    /// - [`atomic_store::AtomicStore`]
    /// - [`atomic_store::RollingLog`]
    /// - [`atomic_store::AppendLog`]
    pub fn create(path: &Path) -> atomic_store::Result<Self> {
        let loader = AtomicStoreLoader::create(path, "phaselock")?;
        Self::init_from_loader(loader)
    }

    /// Open an atomic storage at a given path.
    ///
    /// # Errors
    ///
    /// Returns the underlying errors that the following types can throw:
    /// - [`atomic_store::AtomicStoreLoader`]
    /// - [`atomic_store::AtomicStore`]
    /// - [`atomic_store::RollingLog`]
    /// - [`atomic_store::AppendLog`]
    pub fn open(path: &Path) -> atomic_store::Result<Self> {
        let loader = AtomicStoreLoader::load(path, "phaselock")?;
        Self::init_from_loader(loader)
    }

    /// Open an atomic storage with a given [`AtomicStoreLoader`]
    ///
    /// # Errors
    ///
    /// Returns the underlying errors that the following types can throw:
    /// - [`atomic_store::AtomicStore`]
    /// - [`atomic_store::RollingLog`]
    /// - [`atomic_store::AppendLog`]
    pub fn init_from_loader(mut loader: AtomicStoreLoader) -> atomic_store::Result<Self> {
        let blocks = HashMapStore::load(&mut loader, "phaselock_blocks")?;
        let qcs = DualKeyValueStore::open(&mut loader, "phaselock_qcs")?;
        let leaves = DualKeyValueStore::open(&mut loader, "phaselock_leaves")?;
        let states = HashMapStore::load(&mut loader, "phaselock_states")?;

        let atomic_store = AtomicStore::open(loader)?;

        Ok(Self {
            inner: Arc::new(AtomicStorageInner {
                atomic_store: Mutex::new(atomic_store),
                blocks,
                qcs,
                leaves,
                states,
            }),
        })
    }
}

#[async_trait]
impl<B: BlockContents<N> + 'static, S: State<N, Block = B> + 'static, const N: usize>
    Storage<B, S, N> for AtomicStorage<B, S, N>
{
    #[instrument(name = "AtomicStorage::get_block", skip_all)]
    async fn get_block(&self, hash: &BlockHash<N>) -> StorageResult<Option<B>> {
        Ok(self.inner.blocks.get(hash).await)
    }

    #[instrument(name = "AtomicStorage::get_qc", skip_all)]
    async fn get_qc(&self, hash: &BlockHash<N>) -> StorageResult<Option<QuorumCertificate<N>>> {
        Ok(self.inner.qcs.load_by_key_1_ref(hash).await)
    }

    #[instrument(name = "AtomicStorage::get_newest_qc", skip_all)]
    async fn get_newest_qc(&self) -> StorageResult<Option<QuorumCertificate<N>>> {
        Ok(self.inner.qcs.load_latest(|qc| qc.view_number).await)
    }

    #[instrument(name = "AtomicStorage::get_qc_for_view", skip_all)]
    async fn get_qc_for_view(
        &self,
        view: ViewNumber,
    ) -> StorageResult<Option<QuorumCertificate<N>>> {
        Ok(self.inner.qcs.load_by_key_2(view).await)
    }

    #[instrument(name = "AtomicStorage::get_leaf", skip_all)]
    async fn get_leaf(&self, hash: &LeafHash<N>) -> StorageResult<Option<Leaf<B, N>>> {
        Ok(self.inner.leaves.load_by_key_1_ref(hash).await)
    }

    #[instrument(name = "AtomicStorage::get_leaf_by_block", skip_all)]
    async fn get_leaf_by_block(&self, hash: &BlockHash<N>) -> StorageResult<Option<Leaf<B, N>>> {
        Ok(self.inner.leaves.load_by_key_2_ref(hash).await)
    }

    #[instrument(name = "AtomicStorage::get_state", skip_all)]
    async fn get_state(&self, hash: &LeafHash<N>) -> StorageResult<Option<S>> {
        Ok(self.inner.states.get(hash).await)
    }

    async fn update<'a, F, FUT>(&'a self, update_fn: F) -> StorageResult
    where
        F: FnOnce(Box<dyn StorageUpdater<'a, B, S, N> + 'a>) -> FUT + Send + 'a,
        FUT: Future<Output = StorageResult> + Send + 'a,
    {
        let updater = Box::new(AtomicStorageUpdater { inner: &self.inner });
        update_fn(updater).await?;

        // Make sure to commit everything
        self.inner
            .blocks
            .commit_version()
            .await
            .context(AtomicStoreSnafu)?;
        self.inner
            .qcs
            .commit_version()
            .await
            .context(AtomicStoreSnafu)?;
        self.inner
            .leaves
            .commit_version()
            .await
            .context(AtomicStoreSnafu)?;
        self.inner
            .states
            .commit_version()
            .await
            .context(AtomicStoreSnafu)?;
        self.inner
            .atomic_store
            .lock()
            .await
            .commit_version()
            .context(AtomicStoreSnafu)?;

        Ok(())
    }

    async fn get_internal_state(&self) -> StorageState<B, S, N> {
        let mut blocks: Vec<(BlockHash<N>, B)> =
            self.inner.blocks.load_all().await.into_iter().collect();
        blocks.sort_by_key(|(hash, _)| *hash);
        let blocks = blocks.into_iter().map(|(_, block)| block).collect();

        let mut leafs: Vec<Leaf<B, N>> = self.inner.leaves.load_all().await;
        leafs.sort_by_cached_key(Leaf::hash);

        let mut quorum_certificates = self.inner.qcs.load_all().await;
        quorum_certificates.sort_by_key(|qc| qc.view_number);

        let mut states: Vec<(LeafHash<N>, S)> =
            self.inner.states.load_all().await.into_iter().collect();
        states.sort_by_key(|(hash, _)| *hash);
        let states = states.into_iter().map(|(_, state)| state).collect();

        StorageState {
            blocks,
            quorum_certificates,
            leafs,
            states,
        }
    }
}

/// Implementation of [`StorageUpdater`] for the [`AtomicStorage`]
struct AtomicStorageUpdater<
    'a,
    B: BlockContents<N> + 'static,
    S: State<N, Block = B> + 'static,
    const N: usize,
> {
    /// A reference to the internals of the [`AtomicStorage`]
    inner: &'a AtomicStorageInner<B, S, N>,
}

#[async_trait]
impl<'a, B: BlockContents<N> + 'static, S: State<N, Block = B> + 'static, const N: usize>
    StorageUpdater<'a, B, S, N> for AtomicStorageUpdater<'a, B, S, N>
{
    #[instrument(name = "AtomicStorage::get_block", skip_all)]
    async fn insert_block(&mut self, hash: BlockHash<N>, block: B) -> StorageResult {
        trace!(?block, "inserting block");
        self.inner
            .blocks
            .insert(hash, block)
            .await
            .context(AtomicStoreSnafu)?;
        Ok(())
    }

    #[instrument(name = "AtomicStorage::insert_leaf", skip_all)]
    async fn insert_leaf(&mut self, leaf: Leaf<B, N>) -> StorageResult {
        self.inner.leaves.insert(leaf).await
    }

    #[instrument(name = "AtomicStorage::insert_qc", skip_all)]
    async fn insert_qc(&mut self, qc: QuorumCertificate<N>) -> StorageResult {
        self.inner.qcs.insert(qc).await
    }

    #[instrument(name = "AtomicStorage::insert_state", skip_all)]
    async fn insert_state(&mut self, state: S, hash: LeafHash<N>) -> StorageResult {
        trace!(?hash, "Inserting state");
        self.inner
            .states
            .insert(hash, state)
            .await
            .context(AtomicStoreSnafu)?;
        Ok(())
    }
}
