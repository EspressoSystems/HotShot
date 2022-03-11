use async_std::sync::{RwLock, RwLockWriteGuard};
use atomic_store::{load_store::BincodeLoadStore, AtomicStoreLoader, RollingLog};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use tracing::{error, warn};

pub struct RollingStore<K, V>
where
    K: Serialize + DeserializeOwned,
    V: Serialize + DeserializeOwned,
{
    store: RwLock<RollingLog<BincodeLoadStore<Vec<(K, V)>>>>,
    data: RwLock<HashMap<K, V>>,
    new_data: RwLock<HashMap<K, V>>,
}

impl<K, V> RollingStore<K, V>
where
    K: Serialize + DeserializeOwned + Clone + Eq + std::hash::Hash,
    V: Serialize + DeserializeOwned + Clone,
{
    pub async fn commit(&self) -> atomic_store::Result<RollingStoreCommitLock<'_, K, V>> {
        // Take write ownership of this store so no new data is added while we commit
        let mut store = self.store.write().await;
        let data = self.data.write().await;
        let new_data_lock = self.new_data.write().await;

        let mut new_data = (&*data).clone();

        for (k, v) in new_data_lock.iter() {
            new_data.insert(k.clone(), v.clone());
        }
        {
            let new_data: Vec<(K, V)> = new_data
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            store.store_resource(&new_data)?;
        }

        Ok(RollingStoreCommitLock {
            store,
            data,
            new_data_lock,
            new_data,
        })
    }

    pub fn load(loader: &mut AtomicStoreLoader, name: &str) -> atomic_store::Result<Self> {
        let store = RollingLog::load(loader, Default::default(), name, 1024)?;
        let data: Vec<(K, V)> = store.load_latest()?;
        let data = data.into_iter().collect();
        Ok(Self {
            store: RwLock::new(store),
            data: RwLock::new(data),
            new_data: RwLock::default(),
        })
    }
}

impl<K, V> RollingStore<K, V>
where
    K: Serialize + DeserializeOwned + Clone + Eq + std::hash::Hash,
    V: Serialize + DeserializeOwned + Clone,
{
    pub async fn get(&self, hash: &K) -> Option<V> {
        if let Some(entry) = self.data.read().await.get(hash).cloned() {
            Some(entry)
        } else {
            self.new_data
                .read()
                .await
                .iter()
                .find(|(k, _)| *k == hash)
                .map(|(_, v)| v.clone())
        }
    }

    pub async fn insert(&self, key: K, val: V) {
        let mut new_data = self.new_data.write().await;
        new_data.insert(key, val);
    }
}

pub struct RollingStoreCommitLock<'a, K, V>
where
    K: Serialize + DeserializeOwned,
    V: Serialize + DeserializeOwned,
{
    store: RwLockWriteGuard<'a, RollingLog<BincodeLoadStore<Vec<(K, V)>>>>,
    data: RwLockWriteGuard<'a, HashMap<K, V>>,
    new_data_lock: RwLockWriteGuard<'a, HashMap<K, V>>,
    new_data: HashMap<K, V>,
}

impl<'a, K, V> RollingStoreCommitLock<'a, K, V>
where
    K: Serialize + DeserializeOwned,
    V: Serialize + DeserializeOwned,
{
    pub fn apply(mut self) {
        self.new_data_lock.clear();
        *self.data = std::mem::take(&mut self.new_data);
    }
}

impl<'a, K, V> Drop for RollingStoreCommitLock<'a, K, V>
where
    K: Serialize + DeserializeOwned,
    V: Serialize + DeserializeOwned,
{
    fn drop(&mut self) {
        if !self.new_data_lock.is_empty() {
            let type_name: &str = std::any::type_name::<RollingStore<K, V>>();
            warn!("{} did not apply properly, rolling back", type_name);
            if let Err(e) = self.store.revert_version() {
                error!(?e, "Could not rollback {}", type_name);
            }
        }
    }
}
