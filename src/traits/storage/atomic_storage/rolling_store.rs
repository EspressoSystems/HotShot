use async_std::sync::{RwLock, RwLockWriteGuard};
use atomic_store::{
    load_store::BincodeLoadStore, AtomicStoreLoader, RollingLog,
};
use serde::{Serialize, de::DeserializeOwned};
use std::{collections::HashMap};
use tracing::{warn, error};
use super::StoreContents;

pub struct RollingStore<T: StoreContents> {
    store: RwLock<RollingLog<BincodeLoadStore<Vec<T::Entry>>>>,
    data: RwLock<T>,
    new_data: RwLock<Vec<T::Entry>>,
}

impl<K, V> RollingStore<HashMap<K, V>> where HashMap<K, V>: StoreContents<Entry = (K, V)>,
K: Serialize + DeserializeOwned + Clone + Eq + std::hash::Hash,
V: Serialize + DeserializeOwned + Clone, {
    pub async fn commit(&self) -> atomic_store::Result<RollingStoreCommitLock<'_, HashMap<K, V>>> {
        // Take write ownership of this store so no new data is added while we commit
        let mut store = self.store.write().await;
        let data = self.data.write().await;
        let new_data_lock = self.new_data.write().await;

        let mut new_data = (&*data).clone();

        for (k, v) in new_data_lock.iter().cloned() {
            new_data.insert(k, v);
        }
        let new_data: Vec<_> = new_data.into_iter().collect();
        store.store_resource(&new_data)?;

        Ok(RollingStoreCommitLock {
            store,
            data,
            new_data_lock,
            new_data,
        })
    }
}

impl<T: StoreContents + Default> RollingStore<T> {
    pub fn load(loader: &mut AtomicStoreLoader, name: &str) -> atomic_store::Result<Self> {
        let store = RollingLog::load(loader, Default::default(), name, 1024)?;
        let data: Vec<T::Entry> = store.load_latest()?;
        let data = data.into_iter().collect();
        Ok(Self {
            store: RwLock::new(store),
            data: RwLock::new(data),
            new_data: RwLock::default(),
        })
    }
}

impl<K, V> RollingStore<HashMap<K, V>>
where
    HashMap<K, V>: StoreContents<Entry = (K, V)>,
    K: Eq + std::hash::Hash,
    V: Clone,
{
    pub async fn get(&self, hash: &K) -> Option<V> {
        if let Some(entry) = self.data.read().await.get(hash).cloned() {
            Some(entry)
        } else {
            self.new_data
                .read()
                .await
                .iter()
                .find(|(k, _)| k == hash)
                .map(|(_, v)| v.clone())
        }
    }

    pub async fn insert(&self, key: K, val: V) {
        let mut new_data = self.new_data.write().await;
        new_data.push((key, val));
    }
}

pub struct RollingStoreCommitLock<'a, T: StoreContents> {
    store: RwLockWriteGuard<'a, RollingLog<BincodeLoadStore<Vec<T::Entry>>>>,
    data: RwLockWriteGuard<'a, T>,
    new_data_lock: RwLockWriteGuard<'a, Vec<T::Entry>>,
    new_data: Vec<T::Entry>,
}

impl<'a, K, V> RollingStoreCommitLock<'a, HashMap<K, V>> 
where HashMap<K, V>: StoreContents<Entry = (K, V)> {
    pub fn apply(mut self) {
        self.new_data_lock.clear();
        *self.data = self.new_data.drain(..).collect();
    }
}

impl<'a, T> Drop for RollingStoreCommitLock<'a, T> where T: StoreContents {
    fn drop(&mut self) {
        if !self.new_data_lock.is_empty() {
            let type_name: &str = std::any::type_name::<RollingStore<T>>();
            warn!("{} did not apply properly, rolling back", type_name);
            if let Err(e) = self.store.revert_version() {
                error!(?e, "Could not rollback {}", type_name);
            }
        }
    }
}