//! A store that operates on a value with 2 different keys.
//!
//! Implementations should implement [`DualKeyValue`] before they can use [`DualKeyValueStore`].

use async_std::sync::RwLock;
use atomic_store::{load_store::BincodeLoadStore, AppendLog, AtomicStoreLoader};
use phaselock_types::{
    data::{BlockHash, Leaf, LeafHash, QuorumCertificate},
    traits::{
        storage::{AtomicStoreSnafu, InconsistencySnafu, StorageError},
        BlockContents,
    },
};
use serde::{de::DeserializeOwned, Serialize};
use snafu::ResultExt;
use std::{collections::HashMap, hash::Hash};

/// A store that allows lookup of a value by 2 different keys.
pub struct DualKeyValueStore<K: DualKeyValue> {
    /// inner value
    inner: RwLock<Inner<K>>,
}

/// The inner struct of the [`DualKeyValueStore`]
struct Inner<K: DualKeyValue> {
    /// The underlying store
    store: AppendLog<BincodeLoadStore<K>>,

    /// Key 1 to index
    key_1: HashMap<K::Key1, usize>,

    /// Key 2 to index
    key_2: HashMap<K::Key2, usize>,

    /// Actual values. This list should be append-only
    values: Vec<K>,
}

impl<K: DualKeyValue> DualKeyValueStore<K> {
    /// Open the [`DualKeyValueStore`] with the given loader and name.
    ///
    /// # Errors
    ///
    /// Returns any errors that [`AppendLog`]'s `load` returns.
    pub fn open(
        loader: &mut AtomicStoreLoader,
        name: &str,
    ) -> Result<Self, atomic_store::PersistenceError> {
        let store = AppendLog::load(loader, BincodeLoadStore::default(), name, 1024)?;
        let values = store
            .iter()
            .collect::<Result<Vec<K>, atomic_store::PersistenceError>>()
            .unwrap_or_default();
        let key_1 = values
            .iter()
            .enumerate()
            .map(|(idx, v)| (v.key_1(), idx))
            .collect();
        let key_2 = values
            .iter()
            .enumerate()
            .map(|(idx, v)| (v.key_2(), idx))
            .collect();
        Ok(Self {
            inner: RwLock::new(Inner {
                store,
                key_1,
                key_2,
                values,
            }),
        })
    }

    /// Load the `K` value based on the 1st key.
    pub async fn load_by_key_1_ref(&self, k: &K::Key1) -> Option<K> {
        let read = self.inner.read().await;
        let idx = read.key_1.get(k).copied()?;
        Some(read.values[idx].clone())
    }

    /// Load the `K` value based on a reference of the 2nd key.
    pub async fn load_by_key_2_ref(&self, k: &K::Key2) -> Option<K> {
        let read = self.inner.read().await;
        let idx = read.key_2.get(k).copied()?;
        Some(read.values[idx].clone())
    }

    /// Load the `K` value based on the 2nd key.
    pub async fn load_by_key_2(&self, k: K::Key2) -> Option<K> {
        self.load_by_key_2_ref(&k).await
    }

    /// Load the latest inserted entry in this [`DualKeyValueStore`]
    pub async fn load_latest<F, V>(&self, cb: F) -> Option<K>
    where
        F: FnMut(&&K) -> V,
        V: std::cmp::Ord,
    {
        let read = self.inner.read().await;
        read.values.iter().max_by_key::<V, F>(cb).cloned()
    }

    /// Load all entries in this [`DualKeyValueStore`]
    pub async fn load_all(&self) -> Vec<K> {
        self.inner.read().await.values.clone()
    }

    /// Insert a value into this [`DualKeyValueStore`]
    ///
    /// # Errors
    ///
    /// Returns any errors that [`AppendLog`]'s `store_resource` returns.
    pub async fn insert(&self, val: K) -> Result<(), StorageError> {
        let mut lock = self.inner.write().await;

        match (lock.key_1.get(&val.key_1()), lock.key_2.get(&val.key_2())) {
            (Some(idx), Some(key_2_idx)) if idx == key_2_idx => {
                // updating
                let idx = *idx;

                // TODO: This still adds a duplicate `K` in the storage
                // ideally we'd update this record instead
                lock.store.store_resource(&val).context(AtomicStoreSnafu)?;
                lock.values[idx] = val;
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
                // inserting
                lock.store.store_resource(&val).context(AtomicStoreSnafu)?;

                let idx = lock.values.len();
                lock.key_1.insert(val.key_1(), idx);
                lock.key_2.insert(val.key_2(), idx);
                lock.values.push(val);

                Ok(())
            }
        }
    }

    /// Commit this [`DualKeyValueStore`].
    ///
    /// # Errors
    ///
    /// Returns any errors that [`AppendLog`]'s `commit_version` returns.
    pub async fn commit_version(&self) -> atomic_store::Result<()> {
        let mut lock = self.inner.write().await;
        lock.store.commit_version()?;
        Ok(())
    }
}

/// A dual key value. Used for [`DualKeyValueStore`]
pub trait DualKeyValue: Serialize + DeserializeOwned + Clone {
    /// The first key type
    type Key1: Serialize + DeserializeOwned + Hash + Eq;
    /// The second key type
    type Key2: Serialize + DeserializeOwned + Hash + Eq;

    /// Get a copy of the first key
    fn key_1(&self) -> Self::Key1;
    /// Get a clone of the second key
    fn key_2(&self) -> Self::Key2;
}

impl<const N: usize> DualKeyValue for QuorumCertificate<N> {
    type Key1 = BlockHash<N>;
    type Key2 = u64;
    fn key_1(&self) -> Self::Key1 {
        self.block_hash
    }
    fn key_2(&self) -> Self::Key2 {
        self.view_number
    }
}

impl<Block, const N: usize> DualKeyValue for Leaf<Block, N>
where
    Block: Clone + Serialize + DeserializeOwned + BlockContents<N>,
{
    type Key1 = LeafHash<N>;
    type Key2 = BlockHash<N>;

    fn key_1(&self) -> Self::Key1 {
        self.hash()
    }

    fn key_2(&self) -> Self::Key2 {
        BlockContents::hash(&self.item)
    }
}
