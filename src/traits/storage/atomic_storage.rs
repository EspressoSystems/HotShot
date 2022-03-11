//! On-disk storage of node state. Based on [atomic_store](https://github.com/EspressoSystems/atomicstore).
mod append_store;
mod rolling_store;

use self::append_store::AppendStore;
use self::rolling_store::RollingStore;
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

struct AtomicStorageInner<Block, State, const N: usize>
where
    Block: DeserializeOwned + Serialize,
    State: DeserializeOwned + Serialize,
{
    /// The atomic store loader
    atomic_store: Mutex<AtomicStore>,

    /// The Blocks stored by this [`AtomicStorage`]
    blocks: RollingStore<BlockHash<N>, Block>,
    /// The [`QuorumCertificate`]s stored by this [`AtomicStorage`]
    ///
    /// In order to maintain the struct constraints, this list must be append only. Once a QC is
    /// inserted, it index _must not_ change
    qcs: AppendStore<QuorumCertificate<N>>,
    /// Index of the [`QuorumCertificate`]s by hash
    hash_to_qc: RollingStore<BlockHash<N>, usize>,
    /// Index of the [`QuorumCertificate`]s by view number
    view_to_qc: RollingStore<u64, usize>,
    /// The [`Leaf`s stored by this [`AtomicStorage`]
    ///
    /// In order to maintain the struct constraints, this list must be append only. Once a QC is
    /// inserted, it index _must not_ change
    leaves: AppendStore<Leaf<Block, N>>,
    /// Index of the [`Leaf`]s by their hashes
    hash_to_leaf: RollingStore<LeafHash<N>, usize>,
    /// Index of the [`Leaf`]s by their block's hashes
    block_to_leaf: RollingStore<BlockHash<N>, usize>,
    /// The store of states
    states: RollingStore<LeafHash<N>, State>,
}

#[derive(Clone)]
pub struct AtomicStorage<Block, State, const N: usize>
where
    Block: DeserializeOwned + Serialize,
    State: DeserializeOwned + Serialize,
{
    inner: Arc<AtomicStorageInner<Block, State, N>>,
}

impl<Block, State, const N: usize> AtomicStorage<Block, State, N>
where
    Block: DeserializeOwned + Serialize + Clone,
    State: DeserializeOwned + Serialize + Clone,
{
    #[allow(dead_code)]
    pub fn open(path: &Path) -> atomic_store::Result<Self> {
        let mut loader = AtomicStoreLoader::load(path, "phaselock")?;

        let blocks = RollingStore::load(&mut loader, "phaselock_blocks")?;
        let qcs = AppendStore::load(&mut loader, "phaselock_qcs")?;
        let hash_to_qc = RollingStore::load(&mut loader, "phaselock_hash_to_qc")?;
        let view_to_qc = RollingStore::load(&mut loader, "phaselock_view_to_qc")?;
        let leaves = AppendStore::load(&mut loader, "phaselock_leaves")?;
        let hash_to_leaf = RollingStore::load(&mut loader, "phaselock_hash_to_leaf")?;
        let block_to_leaf = RollingStore::load(&mut loader, "phaselock_block_to_leaf")?;
        let states = RollingStore::load(&mut loader, "phaselock_states")?;

        let atomic_store = AtomicStore::open(loader)?;

        Ok(Self {
            inner: Arc::new(AtomicStorageInner {
                atomic_store: Mutex::new(atomic_store),
                blocks,
                qcs,
                hash_to_qc,
                view_to_qc,
                leaves,
                hash_to_leaf,
                block_to_leaf,
                states,
            }),
        })
    }
}

impl<B: BlockContents<N> + 'static, S: State<N, Block = B> + 'static, const N: usize>
    Storage<B, S, N> for AtomicStorage<B, S, N>
{
    fn get_block<'b, 'a: 'b>(&'a self, hash: &'b BlockHash<N>) -> BoxFuture<'b, StorageResult<B>> {
        async move {
            if let Some(block) = self.inner.blocks.get(hash).await {
                trace!("Block found");
                StorageResult::Some(block)
            } else {
                trace!("Block not found");
                StorageResult::None
            }
        }
        .instrument(info_span!("MemoryStorage::get_block", ?hash))
        .boxed()
    }

    fn insert_block(&self, hash: BlockHash<N>, block: B) -> BoxFuture<'_, StorageResult<()>> {
        async move {
            trace!(?block, "inserting block");
            self.inner.blocks.insert(hash, block).await;
            StorageResult::Some(())
        }
        .instrument(info_span!("MemoryStorage::insert_block", ?hash))
        .boxed()
    }

    fn get_qc<'b, 'a: 'b>(
        &'a self,
        hash: &'b BlockHash<N>,
    ) -> BoxFuture<'b, StorageResult<QuorumCertificate<N>>> {
        async move {
            // Check to see if we have the qc
            let index = self.inner.hash_to_qc.get(hash).await;
            if let Some(index) = index {
                trace!("Found qc");
                let qc = self.inner.qcs.get(index).await.unwrap();
                StorageResult::Some(qc)
            } else {
                trace!("Did not find qc");
                StorageResult::None
            }
        }
        .instrument(info_span!("MemoryStorage::get_qc", ?hash))
        .boxed()
    }

    fn get_qc_for_view(&self, view: u64) -> BoxFuture<'_, StorageResult<QuorumCertificate<N>>> {
        async move {
            // Check to see if we have the qc
            let index = self.inner.view_to_qc.get(&view).await;
            if let Some(index) = index {
                trace!("Found qc");
                let qc = self.inner.qcs.get(index).await.unwrap();
                StorageResult::Some(qc)
            } else {
                trace!("Did not find qc");
                StorageResult::None
            }
        }
        .instrument(info_span!("MemoryStorage::get_qc_for_view", ?view))
        .boxed()
    }

    fn insert_qc(&self, qc: QuorumCertificate<N>) -> BoxFuture<'_, StorageResult<()>> {
        async move {
            // Insert the qc into the main vec and the add the references
            let view = qc.view_number;
            let hash = qc.block_hash;
            let index = self.inner.qcs.append(qc).await;
            self.inner.view_to_qc.insert(view, index).await;
            self.inner.hash_to_qc.insert(hash, index).await;
            StorageResult::Some(())
        }
        .instrument(info_span!("MemoryStorage::insert_qc"))
        .boxed()
    }

    fn get_leaf<'b, 'a: 'b>(
        &'a self,
        hash: &'b LeafHash<N>,
    ) -> BoxFuture<'b, StorageResult<Leaf<B, N>>> {
        async move {
            // Check to see if we have the leaf
            let index = self.inner.hash_to_leaf.get(hash).await;
            if let Some(index) = index {
                trace!("Found leaf");
                let leaf = self.inner.leaves.get(index).await.unwrap();
                StorageResult::Some(leaf)
            } else {
                trace!("Did not find leaf");
                StorageResult::None
            }
        }
        .instrument(info_span!("MemoryStorage::get_leaf", ?hash))
        .boxed()
    }

    fn get_leaf_by_block<'b, 'a: 'b>(
        &'a self,
        hash: &'b BlockHash<N>,
    ) -> BoxFuture<'b, StorageResult<Leaf<B, N>>> {
        async move {
            // Check to see if we have the leaf
            let index = self.inner.block_to_leaf.get(hash).await;
            if let Some(index) = index {
                trace!("Found leaf");
                let leaf = self.inner.leaves.get(index).await.unwrap();
                StorageResult::Some(leaf)
            } else {
                trace!("Did not find leaf");
                StorageResult::None
            }
        }
        .instrument(info_span!("MemoryStorage::get_by_block", ?hash))
        .boxed()
    }

    fn insert_leaf(&self, leaf: Leaf<B, N>) -> BoxFuture<'_, StorageResult<()>> {
        async move {
            let hash = leaf.hash();
            trace!(?leaf, ?hash, "Inserting");
            let block_hash = BlockContents::hash(&leaf.item);
            let index = self.inner.leaves.append(leaf).await;
            self.inner.hash_to_leaf.insert(hash, index).await;
            self.inner.block_to_leaf.insert(block_hash, index).await;
            StorageResult::Some(())
        }
        .instrument(info_span!("MemoryStorage::insert_leaf"))
        .boxed()
    }

    fn insert_state(&self, state: S, hash: LeafHash<N>) -> BoxFuture<'_, StorageResult<()>> {
        async move {
            trace!(?hash, "Inserting state");
            self.inner.states.insert(hash, state).await;
            StorageResult::Some(())
        }
        .instrument(info_span!("MemoryStorage::insert_state"))
        .boxed()
    }

    fn get_state<'b, 'a: 'b>(&'a self, hash: &'b LeafHash<N>) -> BoxFuture<'b, StorageResult<S>> {
        async move {
            if let Some(state) = self.inner.states.get(hash).await {
                StorageResult::Some(state)
            } else {
                StorageResult::None
            }
        }
        .boxed()
    }

    fn commit(
        &self,
    ) -> BoxFuture<'_, Result<(), Box<dyn std::error::Error + Send + Sync + 'static>>> {
        async move {
            let blocks = self.inner.blocks.commit().await?;
            let qcs = self.inner.qcs.commit().await?;
            let hash_to_qc = self.inner.hash_to_qc.commit().await?;
            let view_to_qc = self.inner.view_to_qc.commit().await?;
            let leaves = self.inner.leaves.commit().await?;
            let hash_to_leaf = self.inner.hash_to_leaf.commit().await?;
            let block_to_leaf = self.inner.block_to_leaf.commit().await?;
            let states = self.inner.states.commit().await?;
            self.inner.atomic_store.lock().await.commit_version()?;

            blocks.apply();
            qcs.apply();
            hash_to_qc.apply();
            view_to_qc.apply();
            leaves.apply();
            hash_to_leaf.apply();
            block_to_leaf.apply();
            states.apply();

            Ok(())
        }
        .boxed()
    }
}
