//! A store based on [`AppendLog`]

use std::sync::Arc;

use async_std::sync::{RwLock, RwLockWriteGuard};
use atomic_store::{load_store::BincodeLoadStore, AppendLog, AtomicStoreLoader};
use serde::{de::DeserializeOwned, Serialize};
use tracing::{error, trace, warn};

/// A store with [`AppendLog`] as the storage system.
pub struct AppendStore<T: Serialize + DeserializeOwned> {
    /// The underlying atomic_store store
    store: Arc<RwLock<AppendLog<BincodeLoadStore<T>>>>,
    /// Data currently loaded in the store
    data: RwLock<Vec<T>>,
    /// New records that aren't committed to the store yet
    append: RwLock<Vec<T>>,
}

impl<T: Serialize + DeserializeOwned> AppendStore<T> {
    /// Load a `AppendStore` with the given loader and name.
    ///
    /// # Errors
    ///
    /// Returns any errors that [`AppendLog`]'s `load` returns.
    pub fn load(loader: &mut AtomicStoreLoader, name: &str) -> atomic_store::Result<Self> {
        let store = AppendLog::load(loader, BincodeLoadStore::default(), name, 1024)?;
        let data = store.iter().collect::<Result<_, _>>().unwrap_or_default();
        let append = Vec::new();
        Ok(Self {
            store: Arc::new(RwLock::new(store)),
            data: RwLock::new(data),
            append: RwLock::new(append),
        })
    }

    /// Commit this rolling store, returning a commit lock.
    ///
    /// Once all stores are committed, you need to call `apply` on this lock, or else the commit will be reverted once the lock goes out of scope.
    ///
    /// # Errors
    ///
    /// Returns any errors that [`AppendLog`]'s `store_resource` and `commit_version` returns.
    pub async fn commit(&self) -> atomic_store::Result<AppendStoreCommitLock<'_, T>> {
        // Take write ownership of this store so no new data is added while we commit
        let mut store = self.store.write().await;
        let data = self.data.write().await;
        let append = self.append.write().await;

        for entry in append.iter() {
            store.store_resource(entry)?;
        }
        store.commit_version()?;

        Ok(AppendStoreCommitLock {
            store: Arc::clone(&self.store),
            data,
            append,
        })
    }

    /// Get the amount of entries that are waiting to be committed.
    pub async fn uncommitted_change_count(&self) -> usize {
        self.append.read().await.len()
    }
}

impl<T> AppendStore<T>
where
    T: Serialize + DeserializeOwned + Clone + std::fmt::Debug,
{
    /// Get an entry in this store. Returning `Some(V)` if it was found.
    pub async fn get(&self, idx: usize) -> Option<T> {
        let idx = {
            // Attempt to get the entry from `self.data` first
            let data = self.data.read().await;
            if let Some(data) = data.get(idx).cloned() {
                return Some(data);
            }
            // `self.append` are entries after `self.data`, so make sure to subtract `data.len()`
            idx - data.len()
        };
        self.append.read().await.get(idx).cloned()
    }

    /// Insert a new entry into the store. This won't be committed untill `commit` is called.
    pub async fn append(&self, t: T) -> usize {
        // Get the len of `self.data` so we can correctly calculate the returned index
        let len = self.data.read().await.len();
        let mut append = self.append.write().await;
        // The index that the entry will be added at
        let index = append.len() + len;
        trace!(?t, ?index, "Inserting");
        append.push(t);
        index
    }
}

/// The commit lock returned from [`AppendStore`]'s `commit` function.
/// Make sure to call `.apply` once all stores are committed or else this commit will be rolled back once this goes out of scope.
pub struct AppendStoreCommitLock<'a, T: Serialize + DeserializeOwned> {
    /// The lock of `AppendStore::store`
    store: Arc<RwLock<AppendLog<BincodeLoadStore<T>>>>,
    /// The lock of `AppendStore::data`
    data: RwLockWriteGuard<'a, Vec<T>>,
    /// The lock of `AppendStore::append`
    append: RwLockWriteGuard<'a, Vec<T>>,
}

impl<'a, T: Serialize + DeserializeOwned> AppendStoreCommitLock<'a, T> {
    /// Apply the commit. Making sure it doesn't roll back.
    pub fn apply(mut self) {
        for item in std::mem::take(&mut *self.append) {
            self.data.push(item);
        }
    }
}

impl<'a, T: Serialize + DeserializeOwned> Drop for AppendStoreCommitLock<'a, T> {
    fn drop(&mut self) {
        if !self.append.is_empty() {
            let type_name: &str = std::any::type_name::<AppendStore<T>>();
            warn!("{} did not apply properly, rolling back", type_name);
            async_std::task::block_on(async move {
                if let Err(e) = self.store.write().await.revert_version() {
                    error!(?e, "Could not rollback {}", type_name);
                }
            });
        }
    }
}
