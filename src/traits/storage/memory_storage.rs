//! [`HashMap`](std::collections::HashMap) and [`Vec`] based implementation of the storage trait
//!
//! This module provides a non-persisting, dummy adapter for the [`Storage`] trait

use crate::traits::{BlockContents, State};
use async_std::sync::RwLock;
use async_trait::async_trait;
use hotshot_types::{data::ViewNumber, traits::storage::*};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Internal state for a [`MemoryStorage`]
struct MemoryStorageInternal<BLOCK, STATE, const N: usize>
where
    BLOCK: BlockContents<N> + 'static,
    STATE: State<N, Block = BLOCK> + 'static,
{
    stored: BTreeMap<ViewNumber, StoredView<BLOCK, STATE, N>>,
    failed: BTreeSet<ViewNumber>,
}

/// In memory, ephemeral, storage for a [`HotShot`](crate::HotShot) instance
#[derive(Clone)]
pub struct MemoryStorage<BLOCK, STATE, const N: usize>
where
    BLOCK: BlockContents<N> + 'static,
    STATE: State<N, Block = BLOCK> + 'static,
{
    /// The inner state of this [`MemoryStorage`]
    inner: Arc<RwLock<MemoryStorageInternal<BLOCK, STATE, N>>>,
}

impl<BLOCK, STATE, const N: usize> Default for MemoryStorage<BLOCK, STATE, N>
where
    BLOCK: BlockContents<N> + 'static,
    STATE: State<N, Block = BLOCK> + 'static,
{
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(MemoryStorageInternal {
                stored: BTreeMap::new(),
                failed: BTreeSet::new(),
            })),
        }
    }
}

#[async_trait]
impl<B, S, const N: usize> TestableStorage<B, S, N> for MemoryStorage<B, S, N>
where
    B: BlockContents<N> + 'static,
    S: State<N, Block = B> + 'static,
{
    fn construct_tmp_storage() -> Result<Self> {
        Ok(Self::default())
    }

    async fn get_full_state(&self) -> StorageState<B, S, N> {
        let inner = self.inner.read().await;
        StorageState {
            stored: inner.stored.clone(),
            failed: inner.failed.clone(),
        }
    }
}

#[async_trait]
impl<BLOCK, STATE, const N: usize> Storage<BLOCK, STATE, N> for MemoryStorage<BLOCK, STATE, N>
where
    BLOCK: BlockContents<N> + 'static,
    STATE: State<N, Block = BLOCK> + 'static,
{
    async fn append(&self, views: Vec<ViewEntry<BLOCK, STATE, N>>) -> Result {
        let mut inner = self.inner.write().await;
        for view in views {
            match view {
                ViewEntry::Failed(num) => {
                    inner.failed.insert(num);
                }
                ViewEntry::Success(view) => {
                    inner.stored.insert(view.view_number, view);
                }
            }
        }
        Ok(())
    }

    async fn cleanup_storage_up_to_view(&self, view: ViewNumber) -> Result<usize> {
        let mut inner = self.inner.write().await;

        // .split_off will return everything after the given key, including the key.
        // we want to remove `view` so we split_off on next_view
        let next_view = view + 1;
        let stored_after = inner.stored.split_off(&next_view);
        // .split_off will return the map we want to keep stored, so we need to swap them
        let old_stored = std::mem::replace(&mut inner.stored, stored_after);

        // same for the BTreeSet
        let failed_after = inner.failed.split_off(&next_view);
        let old_failed = std::mem::replace(&mut inner.failed, failed_after);

        Ok(old_stored.len() + old_failed.len())
    }

    async fn get_anchored_view(&self) -> Result<StoredView<BLOCK, STATE, N>> {
        let inner = self.inner.read().await;
        let last = inner
            .stored
            .values()
            .next_back()
            .ok_or(StorageError::NoGenesisView)?;
        Ok(last.clone())
    }
}

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;

    use super::*;
    use hotshot_types::data::{BlockHash, LeafHash, QuorumCertificate};
    #[allow(clippy::wildcard_imports)]
    use hotshot_types::traits::block_contents::dummy::*;

    fn random_stored_view(number: ViewNumber) -> StoredView<DummyBlock, DummyState, 32> {
        StoredView {
            append: ViewAppend::Block {
                block: DummyBlock::random(),
                rejected_transactions: BTreeSet::new(),
            },
            view_number: number,
            parent: LeafHash::random(),
            qc: QuorumCertificate {
                block_hash: BlockHash::random(),
                genesis: number == ViewNumber::genesis(),
                leaf_hash: LeafHash::random(),
                signatures: BTreeMap::new(),
                view_number: number,
            },
            state: DummyState::random(),
        }
    }

    #[async_std::test]
    async fn memory_storage() {
        let storage = MemoryStorage::default();
        let genesis = random_stored_view(ViewNumber::genesis());
        storage
            .insert_single_view(genesis.clone())
            .await
            .expect("Could not append block");
        assert_eq!(storage.get_anchored_view().await.unwrap(), genesis);
        storage
            .cleanup_storage_up_to_view(genesis.view_number)
            .await
            .unwrap();
        assert!(storage.get_anchored_view().await.is_err())
    }
}
