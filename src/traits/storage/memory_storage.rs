//! [`HashMap`](std::collections::HashMap) and [`Vec`] based implementation of the storage trait
//!
//! This module provides a non-persisting, dummy adapter for the [`Storage`] trait

use async_lock::RwLock;
use async_trait::async_trait;
use hotshot_types::traits::{
    node_implementation::NodeTypes,
    storage::{
        Result, Storage, StorageError, StorageState, StoredView, TestableStorage, ViewEntry,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Internal state for a [`MemoryStorage`]
struct MemoryStorageInternal<TYPES: NodeTypes> {
    /// The views that have been stored
    stored: BTreeMap<TYPES::Time, StoredView<TYPES>>,
    /// The views that have failed
    failed: BTreeSet<TYPES::Time>,
}

/// In memory, ephemeral, storage for a [`HotShot`](crate::HotShot) instance
#[derive(Clone)]
pub struct MemoryStorage<TYPES: NodeTypes> {
    /// The inner state of this [`MemoryStorage`]
    inner: Arc<RwLock<MemoryStorageInternal<TYPES>>>,
}

#[allow(clippy::new_without_default)]
impl<TYPES: NodeTypes> MemoryStorage<TYPES> {
    /// Create a new instance of the memory storage with the given block and state
    /// NOTE: left as `new` because this API is not stable
    /// we may add arguments to new in the future
    pub fn new() -> Self {
        let inner = MemoryStorageInternal {
            stored: BTreeMap::new(),
            failed: BTreeSet::new(),
        };
        Self {
            inner: Arc::new(RwLock::new(inner)),
        }
    }
}

#[async_trait]
impl<TYPES: NodeTypes> TestableStorage<TYPES> for MemoryStorage<TYPES> {
    fn construct_tmp_storage() -> Result<Self> {
        Ok(Self::new())
    }

    async fn get_full_state(&self) -> StorageState<TYPES> {
        let inner = self.inner.read().await;
        StorageState {
            stored: inner.stored.clone(),
            failed: inner.failed.clone(),
        }
    }
}

#[async_trait]
impl<TYPES: NodeTypes> Storage<TYPES> for MemoryStorage<TYPES> {
    async fn append(&self, views: Vec<ViewEntry<TYPES>>) -> Result {
        let mut inner = self.inner.write().await;
        for view in views {
            match view {
                ViewEntry::Failed(num) => {
                    inner.failed.insert(num);
                }
                ViewEntry::Success(view) => {
                    inner.stored.insert(view.time, view);
                }
            }
        }
        Ok(())
    }

    async fn cleanup_storage_up_to_view(&self, view: TYPES::Time) -> Result<usize> {
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

    async fn get_anchored_view(&self) -> Result<StoredView<TYPES>> {
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
    use crate::traits::election::static_committee::StaticElectionConfig;
    use crate::traits::election::static_committee::StaticVoteToken;

    use super::*;
    use hotshot_types::constants::genesis_proposer_id;
    use hotshot_types::data::fake_commitment;
    use hotshot_types::data::Leaf;
    use hotshot_types::data::QuorumCertificate;
    use hotshot_types::data::ViewNumber;
    #[allow(clippy::wildcard_imports)]
    use hotshot_types::traits::block_contents::dummy::*;
    use hotshot_types::traits::node_implementation::NodeTypes;
    use hotshot_types::traits::signature_key::ed25519::Ed25519Pub;
    use hotshot_types::traits::state::ConsensusTime;
    use hotshot_types::traits::Block;
    use std::collections::BTreeMap;
    use tracing::instrument;

    #[derive(
        Copy,
        Clone,
        Debug,
        Default,
        Hash,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        serde::Serialize,
        serde::Deserialize,
    )]
    struct DummyTypes;

    impl NodeTypes for DummyTypes {
        type Time = ViewNumber;
        type BlockType = DummyBlock;
        type SignatureKey = Ed25519Pub;
        type VoteTokenType = StaticVoteToken;
        type Transaction = <DummyBlock as Block>::Transaction;
        type ElectionConfigType = StaticElectionConfig;
        type StateType = DummyState;
    }

    #[instrument]
    fn random_stored_view(time: ViewNumber) -> StoredView<DummyTypes> {
        // TODO is it okay to be using genesis here?
        let dummy_block_commit = fake_commitment::<DummyBlock>();
        let dummy_leaf_commit = fake_commitment::<Leaf<DummyTypes>>();
        StoredView::from_qc_block_and_state(
            QuorumCertificate {
                block_commitment: dummy_block_commit,
                genesis: time == ViewNumber::genesis(),
                leaf_commitment: dummy_leaf_commit,
                signatures: BTreeMap::new(),
                time,
            },
            DummyBlock::random(),
            DummyState::random(),
            dummy_leaf_commit,
            Vec::new(),
            genesis_proposer_id(),
        )
    }

    #[cfg_attr(
        feature = "tokio-executor",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(feature = "async-std-executor", async_std::test)]
    #[instrument]
    async fn memory_storage() {
        let storage = MemoryStorage::construct_tmp_storage().unwrap();
        let genesis = random_stored_view(ViewNumber::genesis());
        storage
            .append_single_view(genesis.clone())
            .await
            .expect("Could not append block");
        assert_eq!(storage.get_anchored_view().await.unwrap(), genesis);
        storage
            .cleanup_storage_up_to_view(genesis.time)
            .await
            .unwrap();
        assert_eq!(storage.get_anchored_view().await.unwrap(), genesis);
        storage
            .cleanup_storage_up_to_view(genesis.time + 1)
            .await
            .unwrap();
        assert!(storage.get_anchored_view().await.is_err());
    }
}
