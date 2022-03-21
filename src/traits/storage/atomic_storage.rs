//! On-disk storage of node state. Based on [`atomic_store`](https://github.com/EspressoSystems/atomicstore).

mod dual_key_value_store;
mod hash_map_store;

use self::dual_key_value_store::DualKeyValueStore;
use self::hash_map_store::HashMapStore;
use crate::{
    data::{BlockHash, Leaf, LeafHash},
    traits::{
        storage::{Storage, StorageResult},
        BlockContents, State,
    },
    QuorumCertificate,
};
use async_std::sync::Mutex;
use atomic_store::{AtomicStore, AtomicStoreLoader};
use futures::future::{BoxFuture, FutureExt};
use serde::{de::DeserializeOwned, Serialize};
use std::{path::Path, sync::Arc};
use tracing::{info_span, trace, Instrument};

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

impl<Block, State, const N: usize> AtomicStorage<Block, State, N>
where
    Block: BlockContents<N> + DeserializeOwned + Serialize + Clone,
    State: DeserializeOwned + Serialize + Clone,
{
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
        let mut loader = AtomicStoreLoader::load(path, "phaselock")?;

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

impl<B: BlockContents<N> + 'static, S: State<N, Block = B> + 'static, const N: usize>
    Storage<B, S, N> for AtomicStorage<B, S, N>
{
    fn get_block<'b, 'a: 'b>(
        &'a self,
        hash: &'b BlockHash<N>,
    ) -> BoxFuture<'b, StorageResult<Option<B>>> {
        async move { Ok(self.inner.blocks.get(hash).await) }
            .instrument(info_span!("AtomicStorage::get_block", ?hash))
            .boxed()
    }

    fn insert_block(&self, hash: BlockHash<N>, block: B) -> BoxFuture<'_, StorageResult> {
        async move {
            trace!(?block, "inserting block");
            self.inner.blocks.insert(hash, block).await?;
            Ok(())
        }
        .instrument(info_span!("AtomicStorage::insert_block", ?hash))
        .boxed()
    }

    fn get_qc<'b, 'a: 'b>(
        &'a self,
        hash: &'b BlockHash<N>,
    ) -> BoxFuture<'b, StorageResult<Option<QuorumCertificate<N>>>> {
        self.inner
            .qcs
            .load_by_key_1_ref(hash)
            .map(Ok)
            .instrument(info_span!("AtomicStorage::get_qc", ?hash))
            .boxed()
    }

    fn get_qc_for_view(
        &self,
        view: u64,
    ) -> BoxFuture<'_, StorageResult<Option<QuorumCertificate<N>>>> {
        self.inner
            .qcs
            .load_by_key_2(view)
            .map(Ok)
            .instrument(info_span!("AtomicStorage::get_qc_for_view"))
            .boxed()
    }

    fn insert_qc(&self, qc: QuorumCertificate<N>) -> BoxFuture<'_, StorageResult> {
        async move {
            self.inner.qcs.insert(qc).await?;
            Ok(())
        }
        .instrument(info_span!("AtomicStorage::insert_qc"))
        .boxed()
    }

    fn get_leaf<'b, 'a: 'b>(
        &'a self,
        hash: &'b LeafHash<N>,
    ) -> BoxFuture<'b, StorageResult<Option<Leaf<B, N>>>> {
        self.inner
            .leaves
            .load_by_key_1_ref(hash)
            .map(Ok)
            .instrument(info_span!("AtomicStorage::get_leaf", ?hash))
            .boxed()
    }

    fn get_leaf_by_block<'b, 'a: 'b>(
        &'a self,
        hash: &'b BlockHash<N>,
    ) -> BoxFuture<'b, StorageResult<Option<Leaf<B, N>>>> {
        self.inner
            .leaves
            .load_by_key_2_ref(hash)
            .map(Ok)
            .instrument(info_span!("AtomicStorage::get_by_block", ?hash))
            .boxed()
    }

    fn insert_leaf(&self, leaf: Leaf<B, N>) -> BoxFuture<'_, StorageResult> {
        async move {
            self.inner.leaves.insert(leaf).await?;
            Ok(())
        }
        .instrument(info_span!("AtomicStorage::insert_leaf"))
        .boxed()
    }

    fn insert_state(&self, state: S, hash: LeafHash<N>) -> BoxFuture<'_, StorageResult> {
        async move {
            trace!(?hash, "Inserting state");
            self.inner.states.insert(hash, state).await?;
            Ok(())
        }
        .instrument(info_span!("AtomicStorage::insert_state"))
        .boxed()
    }

    fn get_state<'b, 'a: 'b>(
        &'a self,
        hash: &'b LeafHash<N>,
    ) -> BoxFuture<'b, StorageResult<Option<S>>> {
        self.inner.states.get(hash).map(Ok).boxed()
    }

    fn commit(&self) -> BoxFuture<'_, StorageResult> {
        async move {
            self.inner.blocks.commit_version().await?;
            self.inner.qcs.commit_version().await?;
            self.inner.leaves.commit_version().await?;
            self.inner.states.commit_version().await?;
            self.inner.atomic_store.lock().await.commit_version()?;

            Ok(())
        }
        .boxed()
    }
}
