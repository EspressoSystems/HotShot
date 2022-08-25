//! [`HashMap`](std::collections::HashMap) and [`Vec`] based implementation of the storage trait
//!
//! This module provides a non-persisting, dummy adapter for the [`Storage`] trait

use crate::{
    data::{BlockHash, Leaf, LeafHash},
    traits::{BlockContents, StateContents},
    QuorumCertificate,
};
use async_std::sync::RwLock;
use async_trait::async_trait;
use commit::{Commitment, Committable};
use dashmap::DashMap;
use futures::Future;
use hotshot_types::{
    data::ViewNumber,
    traits::storage::{
        InconsistencySnafu, Storage, StorageResult, StorageState, StorageUpdater, TestableStorage,
    },
};
use std::sync::Arc;
use tracing::{instrument, trace};

/// Internal state for a [`MemoryStorage`]
struct MemoryStorageInternal<STATE: StateContents> {
    /// The Blocks stored by this [`MemoryStorage`]
    blocks: DashMap<Commitment<STATE::Block>, STATE::Block>,
    /// The [`QuorumCertificate`]s stored by this [`MemoryStorage`]
    ///
    /// In order to maintain the struct constraints, this list must be append only. Once a QC is
    /// inserted, it index _must not_ change
    qcs: RwLock<Vec<QuorumCertificate<STATE>>>,
    /// Index of the [`QuorumCertificate`]s by hash
    hash_to_qc: DashMap<Commitment<STATE::Block>, usize>,
    /// Index of the [`QuorumCertificate`]s by view number
    view_to_qc: DashMap<ViewNumber, usize>,
    /// The [`Leaf`s stored by this [`MemoryStorage`]
    ///
    /// In order to maintain the struct constraints, this list must be append only. Once a QC is
    /// inserted, it index _must not_ change
    leaves: RwLock<Vec<Leaf<STATE>>>,
    /// Index of the [`Leaf`]s by their hashes
    hash_to_leaf: DashMap<Commitment<Leaf<STATE>>, usize>,
    /// Index of the [`Leaf`]s by their block's hashes
    block_to_leaf: DashMap<Commitment<STATE::Block>, usize>,
    /// The store of states
    states: DashMap<Commitment<Leaf<STATE>>, STATE>,
}

/// In memory, ephemeral, storage for a [`HotShot`](crate::HotShot) instance
#[derive(Clone)]
pub struct MemoryStorage<State: StateContents> {
    /// The inner state of this [`MemoryStorage`]
    inner: Arc<MemoryStorageInternal<State>>,
}

impl<State: StateContents> Default for MemoryStorage<State> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: StateContents + 'static>
    TestableStorage<S> for MemoryStorage<S>
{
    fn construct_tmp_storage() -> StorageResult<Self> {
        Ok(Self::new())
    }
}

impl<State: StateContents> MemoryStorage<State> {
    /// Creates a new, empty [`MemoryStorage`]
    pub fn new() -> Self {
        let inner = MemoryStorageInternal {
            blocks: DashMap::new(),
            qcs: RwLock::new(Vec::new()),
            hash_to_qc: DashMap::new(),
            view_to_qc: DashMap::new(),
            leaves: RwLock::new(Vec::new()),
            hash_to_leaf: DashMap::new(),
            block_to_leaf: DashMap::new(),
            states: DashMap::new(),
        };
        MemoryStorage {
            inner: Arc::new(inner),
        }
    }
}

#[async_trait]
impl<
        STATE: StateContents,
    > Storage<STATE> for MemoryStorage<STATE>
{
    #[instrument(name = "MemoryStorage::get_block", skip_all)]
    async fn get_block(&self, hash: &Commitment<STATE::Block>) -> StorageResult<Option<STATE::Block>> {
        Ok(if let Some(r) = self.inner.blocks.get(hash) {
            trace!("Block found");
            let block = r.value().clone();
            Some(block)
        } else {
            trace!("Block not found");
            None
        })
    }

    #[instrument(name = "MemoryStorage::get_qc", skip_all)]
    async fn get_qc(&self, hash: &Commitment<STATE::Block>) -> StorageResult<Option<QuorumCertificate<STATE>>> {
        // Check to see if we have the qc
        let index = self.inner.hash_to_qc.get(hash);
        Ok(if let Some(index) = index {
            trace!("Found qc");
            let qcs = self.inner.qcs.read().await;
            let qc = qcs[*index.value()].clone();
            Some(qc)
        } else {
            trace!("Did not find qc");
            None
        })
    }

    #[instrument(name = "MemoryStorage::get_newest_qc", skip_all)]
    async fn get_newest_qc(&self) -> StorageResult<Option<QuorumCertificate<STATE>>> {
        let iter = self.inner.view_to_qc.iter();
        let idx = match iter.max_by_key(|pair| *pair.key()) {
            Some(pair) => *pair.value(),
            None => return Ok(None),
        };
        let qcs = self.inner.qcs.read().await;
        Ok(Some(qcs[idx].clone()))
    }

    #[instrument(name = "MemoryStorage::get_qc_for_view", skip_all)]
    async fn get_qc_for_view(
        &self,
        view: ViewNumber,
    ) -> StorageResult<Option<QuorumCertificate<STATE>>> {
        // Check to see if we have the qc
        let index = self.inner.view_to_qc.get(&view);
        Ok(if let Some(index) = index {
            trace!("Found qc");
            let qcs = self.inner.qcs.read().await;
            let qc = qcs[*index.value()].clone();
            Some(qc)
        } else {
            trace!("Did not find qc");
            None
        })
    }

    #[instrument(name = "MemoryStorage::get_leaf", skip_all)]
    async fn get_leaf(&self, hash: &Commitment<Leaf<STATE>>) -> StorageResult<Option<Leaf<STATE>>> {
        trace!(?self.inner.hash_to_leaf, ?hash);
        // Check to see if we have the leaf
        let index = self.inner.hash_to_leaf.get(hash);
        Ok(if let Some(index) = index {
            trace!("Found leaf");
            let leaves = self.inner.leaves.read().await;
            Some(leaves[*index.value()].clone())
        } else {
            trace!("Did not find leaf");
            None
        })
    }

    #[instrument(name = "MemoryStorage::get_by_block", skip_all)]
    async fn get_leaf_by_block(
        &self,
        hash: &Commitment<STATE::Block>,
    ) -> StorageResult<Option<Leaf<STATE>>> {
        // Check to see if we have the leaf
        let index = self.inner.block_to_leaf.get(hash);
        Ok(if let Some(index) = index {
            trace!("Found leaf");
            let leaves = self.inner.leaves.read().await;
            Some(leaves[*index.value()].clone())
        } else {
            trace!("Did not find leaf");
            None
        })
    }

    async fn get_state(&self, hash: &Commitment<Leaf<STATE>>) -> StorageResult<Option<STATE>> {
        let maybe_state = self.inner.states.get(hash);
        Ok(if let Some(state) = maybe_state {
            let state = state.value().clone();
            Some(state)
        } else {
            None
        })
    }

    async fn update<'a, F, FUT>(&'a self, update_fn: F) -> StorageResult
    where
        F: FnOnce(Box<dyn StorageUpdater<'a, STATE> + 'a>) -> FUT + Send + 'a,
        FUT: Future<Output = StorageResult> + Send + 'a,
    {
        let updater = Box::new(MemoryStorageUpdater { inner: &self.inner });
        update_fn(updater).await?;
        Ok(())
    }

    async fn get_internal_state(&self) -> StorageState<STATE> {
        let mut blocks: Vec<(Commitment<STATE::Block>, STATE::Block)> = self
            .inner
            .blocks
            .iter()
            .map(|pair| {
                let (hash, block) = pair.pair();
                (*hash, block.clone())
            })
            .collect();
        // TODO no Ord on commitments...
        // blocks.sort_by_key(|(hash, _)| *hash);
        let blocks = blocks.into_iter().map(|(_, block)| block).collect();

        let mut leafs: Vec<Leaf<STATE>> = self.inner.leaves.read().await.clone();
        // leafs.sort_by_cached_key(Leaf::hash);

        let mut quorum_certificates = self.inner.qcs.read().await.clone();
        // quorum_certificates.sort_by_key(|qc| qc.view_number);

        let mut states: Vec<(Commitment<Leaf<STATE>>, STATE)> = self
            .inner
            .states
            .iter()
            .map(|pair| {
                let (hash, state) = pair.pair();
                (*hash, state.clone())
            })
            .collect();
        // TODO no Ord on commitments
        // states.sort_by_key(|(hash, _)| *hash);
        let states = states.into_iter().map(|(_, state)| state).collect();

        StorageState {
            blocks,
            quorum_certificates,
            leafs,
            states,
        }
    }
}

/// An implementation of [`StorageUpdater`] for [`MemoryStorage`]
struct MemoryStorageUpdater<'a, STATE: StateContents> {
    /// Reference to the internals of the memory storage
    inner: &'a MemoryStorageInternal<STATE>,
}

#[async_trait]
impl<'a, STATE> StorageUpdater<'a, STATE>
    for MemoryStorageUpdater<'a, STATE>
where
    STATE: StateContents + 'static,
{
    #[instrument(name = "MemoryStorage::insert_block", skip_all)]
    async fn insert_block(&mut self, hash: Commitment<STATE::Block>, block: STATE::Block) -> StorageResult {
        trace!(?block, "inserting block");
        self.inner.blocks.insert(hash, block);
        Ok(())
    }

    #[instrument(name = "MemoryStorage::insert_qc", skip_all)]
    async fn insert_qc(&mut self, qc: QuorumCertificate<STATE>) -> StorageResult {
        // Insert the qc into the main vec and the add the references
        let view = qc.view_number;
        let hash = qc.block_hash;
        let mut qcs = self.inner.qcs.write().await;

        match (
            self.inner.view_to_qc.get(&view),
            self.inner.hash_to_qc.get(&hash),
        ) {
            (Some(view_idx), Some(hash_idx)) if view_idx.value() == hash_idx.value() => {
                let index: usize = *view_idx.value() as usize;
                trace!(?qc, ?index, "Updating qc");
                qcs[index] = qc;
                Ok(())
            }
            (Some(_), Some(_)) => InconsistencySnafu {
                description: String::from("the view_number and block_hash already exists"),
            }
            .fail(),
            (Some(_), None) => InconsistencySnafu {
                description: String::from("the view_number already exists"),
            }
            .fail(),
            (None, Some(_)) => InconsistencySnafu {
                description: String::from("the block_hash already exists"),
            }
            .fail(),
            (None, None) => {
                let index = qcs.len();
                trace!(?qc, ?index, "Inserting qc");
                qcs.push(qc);
                self.inner.view_to_qc.insert(view, index);
                self.inner.hash_to_qc.insert(hash, index);
                Ok(())
            }
        }
    }

    #[instrument(name = "MemoryStorage::insert_leaf", skip_all)]
    async fn insert_leaf(&mut self, leaf: Leaf<STATE>) -> StorageResult {
        let hash = leaf.hash();
        trace!(?leaf, ?hash, "Inserting");
        let block_hash = <STATE::Block as Committable>::commit(&leaf.deltas);
        let mut leaves = self.inner.leaves.write().await;
        let index = leaves.len();
        trace!(?leaf, ?index, "Inserting leaf");
        leaves.push(leaf);
        self.inner.hash_to_leaf.insert(hash, index);
        self.inner.block_to_leaf.insert(block_hash, index);
        Ok(())
    }

    #[instrument(name = "MemoryStorage::insert_state", skip_all)]
    async fn insert_state(&mut self, state: STATE, hash: Commitment<Leaf<STATE>>) -> StorageResult {
        trace!(?hash, "Inserting state");
        self.inner.states.insert(hash, state);
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;

    use super::*;
    use crate::demos::dentry::random_quorom_certificate;
    use commit::Committable;
    #[allow(clippy::wildcard_imports)]
    use hotshot_types::traits::block_contents::dummy::*;
    use hotshot_utils::test_util::setup_logging;
    use tracing::instrument;

    fn dummy_qc(
        hash_block: Commitment<DummyBlock>,
        hash_leaf: Commitment<Leaf<DummyState>>,
        view: ViewNumber,
    ) -> QuorumCertificate<DummyState> {
        QuorumCertificate {
            block_hash: hash_block,
            leaf_hash: hash_leaf,
            view_number: view,
            signatures: BTreeMap::new(),
            genesis: true,
        }
    }

    #[async_std::test]
    #[instrument]
    async fn blocks() {
        // setup_logging();
        // // Get our storage and dummy block
        // let storage = MemoryStorage::</* DummyState */>::default();
        // let test_block_1 = DummyBlock::random();
        // let hash_1 = <DummyBlock as Committable>::commit(&test_block_1);
        // let test_block_2 = DummyBlock::random();
        // let hash_2 = <DummyBlock as Committable>::commit(&test_block_2);
        // // Attempt to insert the blocks
        // let res = storage
        //     .update(|mut m| {
        //         let test_block_1 = test_block_1.clone();
        //         async move { m.insert_block(hash_1, test_block_1).await }
        //     })
        //     .await;
        // assert!(res.is_ok());
        // let res = storage
        //     .update(|mut m| {
        //         let test_block_2 = test_block_2.clone();
        //         async move {
        //             m.insert_block(hash_2, test_block_2).await?;
        //             Ok(())
        //         }
        //     })
        //     .await;
        // assert!(res.is_ok());
        // // Then attempt to get the blocks
        // let block_1 = storage.get_block(&hash_1).await.unwrap().unwrap();
        // let block_2 = storage.get_block(&hash_2).await.unwrap().unwrap();
        // // Make sure we got the right blocks
        // assert_eq!(block_1, test_block_1);
        // assert_eq!(block_2, test_block_2);
        // // Try to get an invalid block and make sure it is nothing
        // let bad_hash = BlockHash::<32>::random();
        // let res = storage.get_block(&bad_hash).await.unwrap();
        // assert!(res.is_none());
    }

    #[async_std::test]
    #[instrument]
    async fn qcs() {
        // setup_logging();
        // let storage = MemoryStorage::<DummyState>::default();
        // let view_1 = ViewNumber::new(1);
        // let view_2 = ViewNumber::new(2);
        // let view_3 = ViewNumber::new(3);
        // // Create a few dummy qcs
        // let qc_1_hash_block = BlockHash::<32>::random();
        // let qc_1_hash_leaf = LeafHash::<32>::random();
        // let qc_1 = dummy_qc(qc_1_hash_block, qc_1_hash_leaf, view_1);
        // let qc_2_hash_block = BlockHash::<32>::random();
        // let qc_2_hash_leaf = LeafHash::<32>::random();
        // let qc_2 = dummy_qc(qc_2_hash_block, qc_2_hash_leaf, view_2);
        // // Attempt to insert them
        // storage
        //     .update(|mut m| {
        //         let qc_1 = qc_1.clone();
        //         let qc_2 = qc_2.clone();
        //         async move {
        //             m.insert_qc(qc_1).await?;
        //             m.insert_qc(qc_2).await?;
        //             Ok(())
        //         }
        //     })
        //     .await
        //     .unwrap();
        // // Attempt to get them back by hash
        // let h_qc_1 = storage.get_qc(&qc_1_hash_block).await.unwrap().unwrap();
        // let h_qc_2 = storage.get_qc(&qc_2_hash_block).await.unwrap().unwrap();
        // // Check to make sure we got the right QCs back
        // assert_eq!(h_qc_1, qc_1);
        // assert_eq!(h_qc_2, qc_2);
        // // Attempt to get them back by view number
        // let v_qc_1 = storage.get_qc_for_view(view_1).await.unwrap().unwrap();
        // let v_qc_2 = storage.get_qc_for_view(view_2).await.unwrap().unwrap();
        // // Check to make sure we got the right QCs back
        // assert_eq!(v_qc_1, qc_1);
        // assert_eq!(v_qc_2, qc_2);
        // // Make sure trying to get bunk QCs fails
        // let bunk_hash = BlockHash::<32>::random();
        // assert!(storage.get_qc(&bunk_hash).await.unwrap().is_none());
        // assert!(storage.get_qc_for_view(view_3).await.unwrap().is_none());
        // // Make sure inserting a bunk QC fails
        // //let bad_qc = dummy_qc(bunk_hash, 3, false);
        // //assert!(!storage.insert_qc(bad_qc).await.is_some());
    }

    #[async_std::test]
    #[instrument]
    async fn leaves() {
        // setup_logging();
        // let storage = MemoryStorage::<DummyBlock, DummyState, 32>::default();
        // // Create a few dummy leaves
        // let block_1 = DummyBlock::random();
        // let block_2 = DummyBlock::random();
        // let parent_1 = LeafHash::</* 32 */>::random();
        // let parent_2 = LeafHash::<32>::random();
        // let state_1 = DummyState::random();
        // let state_2 = DummyState::random();
        // let qc_1 = random_quorom_certificate();
        // let qc_2 = random_quorom_certificate();
        // let leaf_1 = Leaf {
        //     parent: parent_1,
        //     deltas: block_1.clone(),
        //     view_number: qc_1.view_number,
        //     justify_qc: qc_1,
        //     state: state_1,
        // };
        // let hash_1 = leaf_1.hash();
        // let leaf_2 = Leaf {
        //     parent: parent_2,
        //     deltas: block_2.clone(),
        //     view_number: qc_2.view_number,
        //     justify_qc: qc_2,
        //     state: state_2,
        // };
        // let hash_2 = leaf_2.hash();
        // // Attempt to insert them
        // storage
        //     .update(|mut m| {
        //         let leaf_1 = leaf_1.clone();
        //         let leaf_2 = leaf_2.clone();
        //         async move {
        //             m.insert_leaf(leaf_1).await?;
        //             m.insert_leaf(leaf_2).await?;
        //             Ok(())
        //         }
        //     })
        //     .await
        //     .unwrap();
        // // Attempt to get them back by hash
        // let h_leaf_1 = storage.get_leaf(&hash_1).await.unwrap().unwrap();
        // let h_leaf_2 = storage.get_leaf(&hash_2).await.unwrap().unwrap();
        // // Make sure they are the right leaves
        // assert_eq!(h_leaf_1.parent, leaf_1.parent);
        // assert_eq!(h_leaf_1.deltas, leaf_1.deltas);
        // assert_eq!(h_leaf_2.parent, leaf_2.parent);
        // assert_eq!(h_leaf_2.deltas, leaf_2.deltas);
        // // Attempt to get them back by block hash
        // let b_leaf_1 = storage
        //     .get_leaf_by_block(&<DummyBlock as BlockContents<32>>::hash(&block_1))
        //     .await
        //     .unwrap()
        //     .unwrap();
        // let b_leaf_2 = storage
        //     .get_leaf_by_block(&<DummyBlock as BlockContents<32>>::hash(&block_2))
        //     .await
        //     .unwrap()
        //     .unwrap();
        // // Make sure they are the right leaves
        // assert_eq!(b_leaf_1.parent, leaf_1.parent);
        // assert_eq!(b_leaf_1.deltas, leaf_1.deltas);
        // assert_eq!(b_leaf_2.parent, leaf_2.parent);
        // assert_eq!(b_leaf_2.deltas, leaf_2.deltas);
        // // Getting a bunk leaf by hash fails
        // assert!(storage
        //     .get_leaf(&LeafHash::<32>::random())
        //     .await
        //     .unwrap()
        //     .is_none());
        // // Getting a bunk leaf by block hash fails
        // assert!(storage
        //     .get_leaf_by_block(&BlockHash::<32>::random())
        //     .await
        //     .unwrap()
        //     .is_none());
    }
}
