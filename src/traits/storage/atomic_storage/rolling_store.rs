//! A store based on [`RollingLog`]

use async_std::sync::{RwLock, RwLockWriteGuard};
use atomic_store::{load_store::BincodeLoadStore, AtomicStoreLoader, RollingLog};
use serde::{de::DeserializeOwned, Serialize};
use std::{collections::HashMap, sync::Arc};
use tracing::{error, warn};

/// A store with [`RollingLog`] as the storage system.
pub struct RollingStore<K, V>
where
    K: Serialize + DeserializeOwned,
    V: Serialize + DeserializeOwned,
{
    /// The underlying atomic_store store
    store: Arc<RwLock<RollingLog<BincodeLoadStore<Vec<(K, V)>>>>>,
    /// Data currently loaded in the store
    data: RwLock<HashMap<K, V>>,
    /// New records that aren't committed to the store yet
    append: RwLock<HashMap<K, V>>,
}

impl<K, V> RollingStore<K, V>
where
    K: Serialize + DeserializeOwned + Clone + Eq + std::hash::Hash,
    V: Serialize + DeserializeOwned + Clone,
{
    /// Commit this rolling store, returning a commit lock.
    ///
    /// Once all stores are committed, you need to call `apply` on this lock, or else the commit will be reverted once the lock goes out of scope.
    ///
    /// # Errors
    ///
    /// Returns any errors that [`RollingLog`]'s `store_resource` and `commit_version` returns.
    pub async fn commit(&self) -> atomic_store::Result<RollingStoreCommitLock<'_, K, V>> {
        // Take write ownership of this store so no new data is added while we commit
        let mut store = self.store.write().await;
        let data = self.data.write().await;
        let append = self.append.write().await;

        let mut new_data = (&*data).clone();

        for (k, v) in append.iter() {
            new_data.insert(k.clone(), v.clone());
        }
        {
            let new_data: Vec<(K, V)> = new_data
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            store.store_resource(&new_data)?;
        }
        store.commit_version()?;

        Ok(RollingStoreCommitLock {
            store: Arc::clone(&self.store),
            data,
            append,
            new_data,
        })
    }

    /// Load a `RollingStore` with the given loader and name.
    ///
    /// # Errors
    ///
    /// Returns any errors that [`RollingLog`]'s `load` returns.
    pub fn load(loader: &mut AtomicStoreLoader, name: &str) -> atomic_store::Result<Self> {
        let store = RollingLog::load(loader, BincodeLoadStore::default(), name, 1024)?;
        let data: Vec<(K, V)> = store.load_latest().unwrap_or_default();
        let data = data.into_iter().collect();
        Ok(Self {
            store: Arc::new(RwLock::new(store)),
            data: RwLock::new(data),
            append: RwLock::default(),
        })
    }

    /// Get the amount of entries that are waiting to be committed.
    pub async fn uncommitted_change_count(&self) -> usize {
        self.append.read().await.len()
    }
}

impl<K, V> RollingStore<K, V>
where
    K: Serialize + DeserializeOwned + Clone + Eq + std::hash::Hash,
    V: Serialize + DeserializeOwned + Clone,
{
    /// Get an entry in this store. Returning `Some(V)` if it was found.
    pub async fn get(&self, hash: &K) -> Option<V> {
        // make sure to read from `append` first, as this contains new data
        if let Some(entry) = self.append.read().await.get(hash).cloned() {
            Some(entry)
        } else {
            self.data.read().await.get(hash).cloned()
        }
    }

    /// Insert a new key-value entry into the store. This won't be committed untill `commit` is called.
    pub async fn insert(&self, key: K, val: V) {
        let mut new_data = self.append.write().await;
        new_data.insert(key, val);
    }
}

/// The commit lock returned from [`RollingStore`]'s `commit` function.
/// Make sure to call `.apply` once all stores are committed or else this commit will be rolled back once this goes out of scope.
pub struct RollingStoreCommitLock<'a, K, V>
where
    K: Serialize + DeserializeOwned,
    V: Serialize + DeserializeOwned,
{
    /// The lock of `RollingStore::store`
    store: Arc<RwLock<RollingLog<BincodeLoadStore<Vec<(K, V)>>>>>,
    /// The lock of `RollingStore::data`
    data: RwLockWriteGuard<'a, HashMap<K, V>>,
    /// The lock of `RollingStore::append`
    append: RwLockWriteGuard<'a, HashMap<K, V>>,
    /// The new data to be used once this lock is applied
    new_data: HashMap<K, V>,
}

impl<'a, K, V> RollingStoreCommitLock<'a, K, V>
where
    K: Serialize + DeserializeOwned,
    V: Serialize + DeserializeOwned,
{
    /// Apply the commit. Making sure it doesn't roll back.
    pub fn apply(mut self) {
        self.append.clear();
        *self.data = std::mem::take(&mut self.new_data);
    }
}

impl<'a, K, V> Drop for RollingStoreCommitLock<'a, K, V>
where
    K: Serialize + DeserializeOwned,
    V: Serialize + DeserializeOwned,
{
    fn drop(&mut self) {
        if !self.append.is_empty() {
            let type_name: &str = std::any::type_name::<RollingStore<K, V>>();
            warn!("{} did not apply properly, rolling back", type_name);
            async_std::task::block_on(async move {
                if let Err(e) = self.store.write().await.revert_version() {
                    error!(?e, "Could not rollback {}", type_name);
                }
            });
        }
    }
}
