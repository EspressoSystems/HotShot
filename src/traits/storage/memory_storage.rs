//! [`HashMap`](std::collections::HashMap) and [`Vec`] based implementation of the storage trait
//!
//! This module provides a non-persisting, dummy adapter for the [`Storage`] trait

use crate::traits::{BlockContents, State};
use async_std::sync::RwLock;
use async_trait::async_trait;
use hotshot_types::{
    data::{Leaf, LeafHash, QuorumCertificate, ViewNumber},
    traits::storage::{
        Result, Storage, StorageError, StorageState, StoredView, TestableStorage, ViewAppend,
        ViewEntry,
    },
};
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
    /// The views that have been stored
    stored: BTreeMap<ViewNumber, StoredView<BLOCK, STATE, N>>,
    /// The views that have failed
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

impl<B, S, const N: usize> MemoryStorage<B, S, N>
where
    B: BlockContents<N> + 'static,
    S: State<N, Block = B> + 'static,
{
    /// Create a new instance of the memory storage with the given block and state
    pub fn new(block: B, state: S) -> Self {
        let mut inner = MemoryStorageInternal {
            stored: BTreeMap::new(),
            failed: BTreeSet::new(),
        };
        let qc = QuorumCertificate {
            block_hash: BlockContents::hash(&block),
            genesis: true,
            leaf_hash: Leaf {
                deltas: block.clone(),
                justify_qc: QuorumCertificate::default(),
                parent: LeafHash::default(),
                state: state.clone(),
                view_number: ViewNumber::genesis(),
            }
            .hash(),
            view_number: ViewNumber::genesis(),
            signatures: BTreeMap::new(),
        };
        inner.stored.insert(
            ViewNumber::genesis(),
            StoredView {
                append: ViewAppend::Block {
                    block,
                    rejected_transactions: BTreeSet::new(),
                },
                parent: LeafHash::default(),
                qc,
                state,
                view_number: ViewNumber::genesis(),
            },
        );
        Self {
            inner: Arc::new(RwLock::new(inner)),
        }
    }
}

#[async_trait]
impl<B, S, const N: usize> TestableStorage<B, S, N> for MemoryStorage<B, S, N>
where
    B: BlockContents<N> + 'static,
    S: State<N, Block = B> + 'static,
{
    fn construct_tmp_storage(block: B, state: S) -> Result<Self> {
        Ok(Self::new(block, state))
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
        let stored_after = inner.stored.split_off(&view);
        // .split_off will return the map we want to keep stored, so we need to swap them
        let old_stored = std::mem::replace(&mut inner.stored, stored_after);

        // same for the BTreeSet
        let failed_after = inner.failed.split_off(&view);
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

    async fn commit(&self) -> Result {
        Ok(()) // do nothing
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
        StoredView::from_qc_block_and_state(
            QuorumCertificate {
                block_hash: BlockHash::random(),
                genesis: number == ViewNumber::genesis(),
                leaf_hash: LeafHash::random(),
                signatures: BTreeMap::new(),
                view_number: number,
            },
            DummyBlock::random(),
            DummyState::random(),
        )
    }

    #[async_std::test]
    async fn memory_storage() {
        let storage =
            MemoryStorage::construct_tmp_storage(DummyBlock::random(), DummyState::random())
                .unwrap();
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
        assert_eq!(storage.get_anchored_view().await.unwrap(), genesis);
        storage
            .cleanup_storage_up_to_view(genesis.view_number + 1)
            .await
            .unwrap();
        assert!(storage.get_anchored_view().await.is_err());
    }
}
