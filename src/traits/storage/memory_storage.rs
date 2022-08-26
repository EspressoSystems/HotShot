//! [`HashMap`](std::collections::HashMap) and [`Vec`] based implementation of the storage trait
//!
//! This module provides a non-persisting, dummy adapter for the [`Storage`] trait

use crate::{
    data::Leaf,
    traits::{BlockContents, StateContents},
    QuorumCertificate,
};
use async_std::sync::RwLock;
use async_trait::async_trait;
use commit::{Commitment, Committable};
use dashmap::DashMap;
use hotshot_types::{
    data::ViewNumber,
    traits::storage::{
        Result, Storage, StorageError, StorageState, StoredView, TestableStorage, ViewAppend,
        ViewEntry,
    },
};
use hotshot_utils::hack::nll_todo;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Internal state for a [`MemoryStorage`]
struct MemoryStorageInternal<STATE: StateContents> {
    /// The views that have been stored
    stored: BTreeMap<ViewNumber, StoredView<STATE>>,
    /// The views that have failed
    failed: BTreeSet<ViewNumber>,
}

/// In memory, ephemeral, storage for a [`HotShot`](crate::HotShot) instance
#[derive(Clone)]
pub struct MemoryStorage<STATE>
where
    STATE: StateContents + 'static,
{
    /// The inner state of this [`MemoryStorage`]
    inner: Arc<RwLock<MemoryStorageInternal<STATE>>>,
}

impl<STATE: StateContents> MemoryStorage<STATE> {
    /// Create a new instance of the memory storage with the given block and state
    pub fn new(block: <STATE as StateContents>::Block, state: STATE) -> Self {
        let mut inner = MemoryStorageInternal {
            stored: BTreeMap::new(),
            failed: BTreeSet::new(),
        };
        let qc = QuorumCertificate {
            block_commitment: block.commit(),
            genesis: true,
            leaf_commitment: Leaf {
                deltas: block.clone(),
                justify_qc: nll_todo(),
                parent: nll_todo(),
                state: state.clone(),
                view_number: ViewNumber::genesis(),
            }
            .commit(),
            view_number: ViewNumber::genesis(),
            signatures: BTreeMap::new(),
        };
        inner.stored.insert(
            ViewNumber::genesis(),
            StoredView {
                append: ViewAppend::Block { block },
                parent: nll_todo(),
                justify_qc: qc,
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
impl<STATE> TestableStorage<STATE> for MemoryStorage<STATE>
where
    STATE: StateContents + 'static,
{
    fn construct_tmp_storage(block: <STATE as StateContents>::Block, state: STATE) -> Result<Self> {
        Ok(Self::new(block, state))
    }

    async fn get_full_state(&self) -> StorageState<STATE> {
        let inner = self.inner.read().await;
        StorageState {
            stored: inner.stored.clone(),
            failed: inner.failed.clone(),
        }
    }
}

#[async_trait]
impl<STATE> Storage<STATE> for MemoryStorage<STATE>
where
    STATE: StateContents + 'static,
{
    async fn append(&self, views: Vec<ViewEntry<STATE>>) -> Result {
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

    async fn get_anchored_view(&self) -> Result<StoredView<STATE>> {
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
    use commit::Committable;
    use hotshot_types::data::QuorumCertificate;
    #[allow(clippy::wildcard_imports)]
    use hotshot_types::traits::block_contents::dummy::*;

    fn random_stored_view(number: ViewNumber) -> StoredView<DummyState> {
        StoredView::from_qc_block_and_state(
            QuorumCertificate {
                block_commitment: nll_todo(),
                genesis: number == ViewNumber::genesis(),
                leaf_commitment: nll_todo(),
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
            .append_single_view(genesis.clone())
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
