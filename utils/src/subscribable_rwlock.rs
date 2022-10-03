use crate::channel::{unbounded, UnboundedReceiver, UnboundedSender};
use async_lock::{Mutex, RwLock};
use async_trait::async_trait;
use std::fmt;

/// read only view of [`SubscribableRwLock`]
#[async_trait]
pub trait ReadView<T: Clone> {
    /// subscribe to state changes. Receive
    /// the updated state upon state change
    async fn subscribe(&self) -> UnboundedReceiver<T>;
    /// async clone the internal state and return it
    async fn cloned(&self) -> T;
}

/// read view with requirements on being threadsafe
pub trait ThreadedReadView<T: Clone + Sync + Send>:
    Send + Sync + ReadView<T> + std::fmt::Debug
{
}

/// A [`RwLock`] that can register subscribers to be notified upon state change.
#[derive(Default)]
pub struct SubscribableRwLock<T: Clone> {
    /// A list of subscribers to the rwlock
    subscribers: Mutex<Vec<UnboundedSender<T>>>,
    /// The lock holding the state
    rw_lock: RwLock<T>,
}

impl<T: Clone + Sync + Send + std::fmt::Debug> ThreadedReadView<T> for SubscribableRwLock<T> {}

#[async_trait]
impl<T: Clone + Send + Sync> ReadView<T> for SubscribableRwLock<T> {
    async fn subscribe(&self) -> UnboundedReceiver<T> {
        let (sender, receiver) = unbounded();
        self.subscribers.lock().await.push(sender);
        receiver
    }

    async fn cloned(&self) -> T {
        self.rw_lock.read().await.clone()
    }
}

impl<T: Clone> SubscribableRwLock<T> {
    /// create a new [`SubscribableRwLock`]
    pub fn new(t: T) -> Self {
        Self {
            subscribers: Mutex::new(Vec::new()),
            rw_lock: RwLock::new(t),
        }
    }

    /// subscribe to state changes. Receive
    /// the updated state upon state change
    pub async fn modify<F>(&self, cb: F)
    where
        F: FnOnce(&mut T),
    {
        let mut lock = self.rw_lock.write().await;
        cb(&mut *lock);
        let result = lock.clone();
        drop(lock);
        self.notify_change_subscribers(result).await;
    }

    /// send subscribers the updated state
    async fn notify_change_subscribers(&self, t: T) {
        let mut lock = self.subscribers.lock().await;
        let mut idx_to_remove = Vec::new();
        for (idx, sender) in lock.iter().enumerate() {
            if sender.send(t.clone()).await.is_err() {
                idx_to_remove.push(idx);
            }
        }
        for idx in idx_to_remove.into_iter().rev() {
            lock.remove(idx);
        }
    }
}

impl<T: Copy> SubscribableRwLock<T> {
    /// Return a copy of the current value of `T`
    pub async fn copied(&self) -> T {
        *self.rw_lock.read().await
    }
}

impl<T: fmt::Debug + Clone> fmt::Debug for SubscribableRwLock<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        /// Helper struct to be shown when the inner mutex is locked.
        struct Locked;
        impl fmt::Debug for Locked {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("<locked>")
            }
        }

        match self.rw_lock.try_read() {
            None => f
                .debug_struct("SubscribableRwLock")
                .field("data", &Locked)
                .finish(),
            Some(guard) => f
                .debug_struct("SubscribableRwLock")
                .field("data", &&*guard)
                .finish(),
        }
    }
}
