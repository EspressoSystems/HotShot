use std::marker::PhantomData;

use async_std::sync::{Condvar, Mutex};

/// Allows the consumer to wait until the queue is full enough before dumping it
pub struct WaitQueue<T> {
    /// Size queue needs to be before it's grabbeable
    wait_limit: usize,
    /// The queue itself
    queue: Mutex<Vec<T>>,
    /// Condvar to manage the queue
    condvar: Condvar,
    /// Phantom
    _phantom: PhantomData<T>,
}

impl<T> WaitQueue<T> {
    /// Creates a new `WaitQueue`
    pub fn new(wait_limit: usize) -> Self {
        WaitQueue {
            wait_limit,
            queue: Mutex::new(Vec::new()),
            condvar: Condvar::new(),
            _phantom: PhantomData,
        }
    }

    /// Waits for the queue to be ready, then returns it
    pub async fn wait(&self) -> Vec<T> {
        let mut guard = self
            .condvar
            .wait_until(self.queue.lock().await, |queue| {
                queue.len() >= self.wait_limit
            })
            .await;
        let mut replacement = Vec::new();
        std::mem::swap(&mut replacement, &mut *guard);
        replacement
    }

    /// Insert a value into the queue
    pub async fn push(&self, value: T) {
        let mut guard = self.queue.lock().await;
        guard.push(value);
        self.condvar.notify_one();
    }
}

/// A type that allows waiting on a single value that satisfies a given predicate to show up
pub struct WaitOnce<T> {
    /// Stores the item, if there is one
    item: Mutex<Option<T>>,
    /// Condvar used for synchronization
    condvar: Condvar,
    /// Phantom
    _phantom: PhantomData<T>,
}

impl<T> WaitOnce<T> {
    /// Creates a new, empty `WaitOnce`
    pub fn new() -> Self {
        WaitOnce {
            item: Mutex::new(None),
            condvar: Condvar::new(),
            _phantom: PhantomData,
        }
    }
    // Note: This function can't actually panic as the unwrap is 'safe'
    #[allow(clippy::missing_panics_doc)]
    /// Waits for the `WaitOnce` to have any contents, then applies a predicate to them. If the
    /// contents satisfy the predicate, then remove and return them, otherwise removes them and
    /// loops until contents satisfying the predicate are found
    pub async fn wait_for(&self, closure: impl Fn(&T) -> bool) -> T {
        let mut x = None;
        while x.is_none() {
            let mut guard = self.condvar.wait(self.item.lock().await).await;
            if guard.is_some() && closure(guard.as_mut().unwrap()) {
                std::mem::swap(&mut x, &mut *guard);
            } else {
                std::mem::drop(std::mem::replace(&mut *guard, None));
            }
        }
        x.unwrap()
    }

    /// Waits for the `WaitOnce` to have any contents, then removes and returns them
    pub async fn wait(&self) -> T {
        self.wait_for(|_| true).await
    }

    /// If the `WaitOnce` has any contents, replace them, otherwise set the contents
    pub async fn put(&self, item: T) {
        *self.item.lock().await = Some(item);
        self.condvar.notify_one();
    }
}

impl<T> std::default::Default for WaitOnce<T> {
    fn default() -> Self {
        Self::new()
    }
}
