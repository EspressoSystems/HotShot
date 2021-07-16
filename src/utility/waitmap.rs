use async_std::sync::{Condvar, Mutex};
use dashmap::DashMap;
use futures::future::{BoxFuture, FutureExt};
use tracing::{info_span, instrument, trace, Instrument};

use std::{
    future::Future,
    hash::Hash,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

/// The internal abstraction for waiting
struct Waiter<T> {
    /// A spot to put the value being waited upon
    value: Mutex<Option<T>>,
    /// The condvar used for notification
    condvar: Condvar,
    /// The number of `Waiter`s
    ///
    /// This allows the value to be dropped from the map after the last `Waiter` either drops or
    /// returns
    count: AtomicUsize,
}

/// Custom [`future`] implementation returned from the `get` method on [`WaitQueue`]
struct WaiterFuture<K, V>
where
    K: Eq + Hash,
{
    /// Reference to the internal [`Waiter`]
    waiter: Arc<Waiter<V>>,
    /// Copy of the key used in the `get` method that created this `WaiterFuture`
    key: K,
    /// Reference to the internal `DashMap`, so that we can remove the value from the map when the
    /// last `WaiterFuture` is dropped
    map: Arc<DashMap<K, Arc<Waiter<V>>>>,
    /// Place to put the future we generate
    future: Option<BoxFuture<'static, V>>,
}

impl<K, V> WaiterFuture<K, V>
where
    K: Eq + Hash + Clone + std::fmt::Debug,
    V: Clone + Send + 'static,
{
    /// Create a new `WaiterFuture`, incrementing the counter on the internal `Waiter`
    #[instrument(skip(waiter, map))]
    fn new(waiter: Arc<Waiter<V>>, key: &K, map: Arc<DashMap<K, Arc<Waiter<V>>>>) -> Self {
        trace!("Incrementing counter");
        let count = waiter.count.fetch_add(1, Ordering::SeqCst);
        trace!(count = ?(count + 1), "Counter incremented, returning new future");
        Self {
            waiter,
            key: key.clone(),
            map,
            future: None,
        }
    }

    /// Creates a future that waits for the item to be present
    fn gen_future(&mut self) -> BoxFuture<'static, V> {
        let waiter = self.waiter.clone();
        async move {
            trace!("Waiting for value to arrive");
            let item = waiter
                .condvar
                .wait_until(waiter.value.lock().await, |x| x.is_some())
                .await;
            trace!("Value has arrived, returning");
            item.clone().unwrap()
        }
        .instrument(info_span!("WaiterFuture::gen_future", ?self.key))
        .boxed()
    }
}

impl<K, V> Drop for WaiterFuture<K, V>
where
    K: Eq + Hash,
{
    #[instrument(skip(self))]
    fn drop(&mut self) {
        // decrement the waiter counter
        let x = self.waiter.count.fetch_sub(1, Ordering::SeqCst);
        trace!(x = ?(x - 1), "remaining waiters");
        // Check to see if we were the last `WaiterFuture`
        if x == 1 {
            trace!("We were the last waiter, dropping value from map");
            // Remove our value from the `DashMap`
            self.map.remove(&self.key);
        }
    }
}

impl<K, V> Future for WaiterFuture<K, V>
where
    K: Eq + Hash + Unpin + Clone + std::fmt::Debug,
    V: Unpin + Clone + Send + 'static,
{
    type Output = V;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut fut: BoxFuture<'static, V> = if let Some(x) = self.future.take() {
            x
        } else {
            self.gen_future()
        };
        let res = fut.as_mut().poll(cx);
        self.future.replace(fut);
        res
    }
}

/// A [`HashMap`](std::collections::HashMap) like type that allows you to await values
#[derive(Clone)]
pub struct WaitMap<K, V> {
    /// The internal DashMap
    map: Arc<DashMap<K, Arc<Waiter<V>>>>,
}

impl<K, V> WaitMap<K, V>
where
    K: Eq + Hash + Unpin + Clone + std::fmt::Debug,
    V: Unpin + Clone + Send + 'static,
{
    /// Creates a new, empty, `WaitMap`
    pub fn new() -> WaitMap<K, V> {
        Self {
            map: Arc::new(DashMap::new()),
        }
    }

    /// Inserts a value into the `WaitMap`, silently ignoring it if there are no waiters
    #[instrument(skip(self, value))]
    pub async fn insert(&self, key: &K, value: V) {
        if let Some(x) = self.map.get(key) {
            trace!("Found waiter for key");
            let waiter = x.value();
            *waiter.value.lock().await = Some(value);
            waiter.condvar.notify_all();
        } else {
            trace!("No waiters for provided key, dropping value");
        }
    }

    /// Returns a future that will complete when a value is inserted into the given key in the
    /// `WaitMap`
    #[instrument(skip(self))]
    pub fn get(&self, key: &K) -> impl Future<Output = V> {
        // get or create the associated waiter
        let waiter_ref = self.map.entry(key.clone()).or_insert_with(|| {
            Arc::new(Waiter {
                value: Mutex::new(None),
                condvar: Condvar::new(),
                count: AtomicUsize::from(0),
            })
        });
        let waiter = waiter_ref.value().clone();
        WaiterFuture::new(waiter, key, self.map.clone())
    }

    /// Returns the number of values currently being waited upon
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns true if the `WaitMap` does not currently have values being waited upon
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl<K, V> Default for WaitMap<K, V>
where
    K: Eq + Hash + Unpin + Clone + std::fmt::Debug,
    V: Unpin + Clone + Send + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utility::test_util::setup_logging;
    use async_std::task::{sleep, spawn};
    use std::time::Duration;

    #[async_std::test]
    #[instrument]
    async fn smoke() {
        setup_logging();
        // Create a new WaitMap
        let wait_map: WaitMap<usize, u64> = WaitMap::new();
        // It should't have any waiters
        assert_eq!(wait_map.len(), 0);
        // Create a background task waiting on a value
        let handle = spawn({
            let wm = wait_map.clone();
            async move { wm.get(&1).await }
        });
        // Wait a bit to allow the task to do its thing
        sleep(Duration::from_millis(10)).await;
        // We should now have one waiter
        assert_eq!(wait_map.len(), 1);
        // Insert a bunk value into the map, this should do nothing
        wait_map.insert(&2, 53).await;
        // We should still have one waiter
        assert_eq!(wait_map.len(), 1);
        // Insert a value into the map that is currently being waited on
        wait_map.insert(&1, 42).await;
        // Await our task
        let x = handle.await;
        // We should get the correct value
        assert_eq!(x, 42);
        // We should also now have zero waiters
        assert_eq!(wait_map.len(), 0);
    }

    #[async_std::test]
    #[instrument]
    async fn two_waiters() {
        setup_logging();
        // Create a new WaitMap
        let wait_map: WaitMap<usize, u64> = WaitMap::new();
        // Spawn two tasks waiting on the same value
        let handle_1 = spawn({
            let wm = wait_map.clone();
            async move { wm.get(&1).await }
        });
        let handle_2 = spawn({
            let wm = wait_map.clone();
            async move { wm.get(&1).await }
        });
        // Wait a bit to allow the tasks to do their thing
        sleep(Duration::from_millis(10)).await;
        // We should have one waiter in the map
        assert_eq!(wait_map.len(), 1);
        // Insert the value in the map
        wait_map.insert(&1, 42).await;
        // await our tasks
        let x_1 = handle_1.await;
        let x_2 = handle_2.await;
        // Make sure we got the right values
        assert_eq!(x_1, 42);
        assert_eq!(x_2, 42);
        // We should also now have zero waiters
        assert_eq!(wait_map.len(), 0);
    }

    #[async_std::test]
    #[instrument]
    async fn two_waiters_drop() {
        setup_logging();
        // Create a new WaitMap
        let wait_map: WaitMap<usize, u64> = WaitMap::new();
        // Spawn two tasks waiting on the same value
        let handle_1 = spawn({
            let wm = wait_map.clone();
            async move { wm.get(&1).await }
        });
        let handle_2 = spawn({
            let wm = wait_map.clone();
            async move { wm.get(&1).await }
        });
        // Wait a bit to allow the tasks to do their thing
        sleep(Duration::from_millis(10)).await;
        // We should have one waiter in the map
        assert_eq!(wait_map.len(), 1);
        // Cancel the second task
        handle_2.cancel().await;
        // Wait a bit to allow the tasks to do their thing
        sleep(Duration::from_millis(10)).await;
        // We should still have one waiter in the map
        assert_eq!(wait_map.len(), 1);
        // Insert the value in the map
        wait_map.insert(&1, 42).await;
        // await our remaining task
        let x_1 = handle_1.await;
        // Make sure we got the right values
        assert_eq!(x_1, 42);
        // We should also now have zero waiters
        assert_eq!(wait_map.len(), 0);
    }
}
