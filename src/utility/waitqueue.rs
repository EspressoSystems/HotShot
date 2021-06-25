use std::{fmt::Debug, marker::PhantomData};

use async_std::sync::{Condvar, Mutex};
use tracing::{instrument, trace};

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

impl<T: Debug> WaitQueue<T> {
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

    /// Waits with a filter, discarding elements that don't meet the requirements
    #[instrument(skip(self, f))]
    pub async fn wait_for(&self, f: impl Fn(&T) -> bool) -> Vec<T> {
        trace!("Waiting for lock");
        let mut guard = self
            .condvar
            .wait_until(self.queue.lock().await, |queue| {
                queue.iter().filter(|x| f(x)).count() >= self.wait_limit
            })
            .await;
        trace!("Acquired lock");
        let mut replacement = Vec::new();
        std::mem::swap(&mut replacement, &mut *guard);
        replacement
    }

    /// Insert a value into the queue
    #[instrument(skip(self))]
    pub async fn push(&self, value: T) {
        trace!("Waiting for lock");
        let mut guard = self.queue.lock().await;
        trace!("lock aquired");
        guard.push(value);
        self.condvar.notify_all();
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

impl<T: Debug> WaitOnce<T> {
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
    #[instrument(skip(self, closure))]
    pub async fn wait_for(&self, closure: impl Fn(&T) -> bool) -> T {
        trace!("Waiting for lock");
        let mut guard = self
            .condvar
            .wait_until(self.item.lock().await, |item| {
                trace!("Inside condvar");
                match item {
                    Some(i) => {
                        if closure(i) {
                            trace!(?i, "Item passed filter");
                            true
                        } else {
                            trace!(?i, "Item failed filter, resetting");
                            *item = None;
                            false
                        }
                    }
                    None => {
                        trace!("No item");
                        false
                    }
                }
            })
            .await;
        let mut replacement = None;
        std::mem::swap(&mut replacement, &mut *guard);
        replacement.unwrap()
    }

    /// Waits for the `WaitOnce` to have any contents, then removes and returns them
    pub async fn wait(&self) -> T {
        self.wait_for(|_| true).await
    }

    /// If the `WaitOnce` has any contents, replace them, otherwise set the contents
    #[instrument(skip(self))]
    pub async fn put(&self, item: T) {
        trace!("Waiting on lock");
        let mut i = self.item.lock().await;
        trace!("Lock aquired");
        *i = Some(item);
        self.condvar.notify_all();
        trace!("Condvar notified");
    }
}

impl<T: Debug> std::default::Default for WaitOnce<T> {
    fn default() -> Self {
        Self::new()
    }
}
