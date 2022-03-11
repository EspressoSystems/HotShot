use async_std::sync::{RwLock, RwLockWriteGuard};
use atomic_store::{load_store::BincodeLoadStore, AppendLog, AtomicStoreLoader};
use serde::{de::DeserializeOwned, Serialize};
use tracing::{error, trace, warn};

pub struct AppendStore<T: Serialize + DeserializeOwned> {
    store: RwLock<AppendLog<BincodeLoadStore<T>>>,
    data: RwLock<Vec<T>>,
    append: RwLock<Vec<T>>,
}

impl<T: Serialize + DeserializeOwned> AppendStore<T> {
    pub fn load(loader: &mut AtomicStoreLoader, name: &str) -> atomic_store::Result<Self> {
        let store = AppendLog::load(loader, Default::default(), name, 1024)?;
        let data = store.iter().collect::<Result<_, _>>()?;
        let append = Vec::new();
        Ok(Self {
            store: RwLock::new(store),
            data: RwLock::new(data),
            append: RwLock::new(append),
        })
    }

    pub async fn commit(&self) -> atomic_store::Result<AppendStoreCommitLock<'_, T>> {
        // Take write ownership of this store so no new data is added while we commit
        let mut store = self.store.write().await;
        let data = self.data.write().await;
        let append = self.append.write().await;

        for entry in append.iter() {
            store.store_resource(entry)?;
        }
        Ok(AppendStoreCommitLock {
            store,
            data,
            append,
        })
    }
}

impl<T> AppendStore<T>
where
    T: Serialize + DeserializeOwned + Clone + std::fmt::Debug,
{
    pub async fn get(&self, idx: usize) -> Option<T> {
        let idx = {
            let data = self.data.read().await;
            if let Some(data) = data.get(idx).cloned() {
                return Some(data);
            }
            idx - data.len()
        };
        self.append.read().await.get(idx).cloned()
    }

    pub async fn append(&self, t: T) -> usize {
        let len = self.data.read().await.len();
        let mut append = self.append.write().await;
        let index = append.len() + len;
        trace!(?t, ?index, "Inserting");
        append.push(t);
        index
    }
}

pub struct AppendStoreCommitLock<'a, T: Serialize + DeserializeOwned> {
    store: RwLockWriteGuard<'a, AppendLog<BincodeLoadStore<T>>>,
    data: RwLockWriteGuard<'a, Vec<T>>,
    append: RwLockWriteGuard<'a, Vec<T>>,
}

impl<'a, T: Serialize + DeserializeOwned> AppendStoreCommitLock<'a, T> {
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
            if let Err(e) = self.store.revert_version() {
                error!(?e, "Could not rollback {}", type_name);
            }
        }
    }
}
