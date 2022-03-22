use async_std::sync::{Mutex, MutexGuard};
use flume::{bounded, Receiver, Sender};
use std::{fmt, time::Duration};

/// A mutex that can register subscribers to be notified. This works in the same way as [`Mutex`], but has some additional functions:
///
/// [`subscribe`] will return a [`Receiver`] which can be used to be notified of changes.
///
/// [`notify_change_subscribers`] will notify all `Receiver` that are registered with the `subscribe` function.
#[derive(Default)]
pub struct SubscribableMutex<T: ?Sized> {
    /// A list of subscribers of this mutex.
    subscribers: Mutex<Vec<Sender<()>>>,
    /// The inner mutex holding the value.
    /// Note that because of the `T: ?Sized` constraint, this must be the last field in this struct.
    mutex: Mutex<T>,
}

impl<T> SubscribableMutex<T> {
    /// Create a new mutex with the value T
    pub fn new(t: T) -> Self {
        Self {
            mutex: Mutex::new(t),
            subscribers: Mutex::default(),
        }
    }

    /// Acquires the mutex.
    ///
    /// Returns a guard that releases the mutex when dropped.
    ///
    /// Consider using one of the following functions instead:
    /// - `modify` to edit the inner value.
    /// - `set` to set the inner value.
    /// - `compare_and_set` compare the inner value with a given value, and if they match, update the value to the second value.
    /// - `copied` and `cloned` gets a copy or clone of the inner value
    #[deprecated(note = "Consider using a different function instead")]
    pub async fn lock(&self) -> MutexGuard<'_, T> {
        self.mutex.lock().await
    }

    /// Notify the subscribers that a change has occured. Subscribers can be registered by calling [`subscribe`].
    ///
    /// Subscribers cannot be removed as they have no unique identifying information. Instead this function will simply remove all senders that fail to deliver their message.
    pub async fn notify_change_subscribers(&self) {
        let mut lock = self.subscribers.lock().await;
        // We currently don't have a way to remove subscribers, so we'll remove them when they fail to deliver their message.
        let mut idx_to_remove = Vec::new();
        for (idx, sender) in lock.iter().enumerate() {
            if sender.send(()).is_err() {
                idx_to_remove.push(idx);
            }
        }
        // Make sure to reverse `idx_to_remove`, or else the first index to remove will make the other indexes invalid
        for idx in idx_to_remove.into_iter().rev() {
            lock.remove(idx);
        }
    }

    /// Create a [`Receiver`] that will be notified every time a thread calls [`notify_change_subscribers`]
    pub async fn subscribe(&self) -> Receiver<()> {
        let (sender, receiver) = bounded(10);
        self.subscribers.lock().await.push(sender);
        receiver
    }

    /// Modify the internal value, then notify all subscribers that the value is updated.
    pub async fn modify<F>(&self, cb: F)
    where
        F: FnOnce(&mut T),
    {
        let mut lock = self.mutex.lock().await;
        cb(&mut *lock);
        drop(lock);
        self.notify_change_subscribers().await;
    }

    /// Set the new inner value, discarding the old ones. This will also notify all subscribers.
    pub async fn set(&self, val: T) {
        let mut lock = self.mutex.lock().await;
        *lock = val;
        drop(lock);
        self.notify_change_subscribers().await;
    }

    /// Wait until `condition` returns `true`. Will block until then.
    pub async fn wait_until<F>(&self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        let receiver = self.subscribe().await;
        loop {
            receiver
                .recv_async()
                .await
                .expect("`SubscribableMutex::wait_until` was still running when it was dropped");
            let lock = self.mutex.lock().await;
            if f(&*lock) {
                return;
            }
        }
    }
    /// Wait `timeout` until `f` returns `true`. Will return `Ok(())` if the function returned `true` before the time elapsed.
    ///
    /// # Errors
    ///
    /// Returns an error when this function timed out.
    pub async fn wait_timeout_until<F>(
        &self,
        timeout: Duration,
        f: F,
    ) -> Result<(), async_std::future::TimeoutError>
    where
        F: FnMut(&T) -> bool,
    {
        async_std::future::timeout(timeout, self.wait_until(f)).await
    }
}

impl<T: PartialEq> SubscribableMutex<T> {
    /// Compare the value of this mutex. If the value is equal to `compare`, it will be set to `set` and all subscribers will be notified
    pub async fn compare_and_set(&self, compare: T, set: T) {
        let mut lock = self.mutex.lock().await;
        if *lock == compare {
            *lock = set;
            drop(lock);
            self.notify_change_subscribers().await;
        }
    }
}

impl<T: Clone> SubscribableMutex<T> {
    /// Return a clone of the current value of `T`
    pub async fn cloned(&self) -> T {
        self.mutex.lock().await.clone()
    }
}

impl<T: Copy> SubscribableMutex<T> {
    /// Return a copy of the current value of `T`
    pub async fn copied(&self) -> T {
        *self.mutex.lock().await
    }
}

impl<T: fmt::Debug> fmt::Debug for SubscribableMutex<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        /// Helper struct to be shown when the inner mutex is locked.
        struct Locked;
        impl fmt::Debug for Locked {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("<locked>")
            }
        }

        match self.mutex.try_lock() {
            None => f
                .debug_struct("SubscribableMutex")
                .field("data", &Locked)
                .finish(),
            Some(guard) => f
                .debug_struct("SubscribableMutex")
                .field("data", &&*guard)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SubscribableMutex;
    use std::{sync::Arc, time::Duration};

    #[async_std::test]
    async fn test_wait_timeout_until() {
        let mutex: Arc<SubscribableMutex<usize>> = Arc::default();
        {
            // inner loop finishes in 1.1s
            let mutex = Arc::clone(&mutex);
            async_std::task::spawn(async move {
                for i in 0..=10 {
                    async_std::task::sleep(Duration::from_millis(100)).await;
                    mutex.set(i).await;
                }
            });
        }
        // wait for 2 seconds
        let result = mutex
            .wait_timeout_until(Duration::from_secs(2), |s| *s == 10)
            .await;
        assert_eq!(result, Ok(()));
        assert_eq!(mutex.copied().await, 10);
    }

    #[async_std::test]
    async fn test_wait_timeout_until_fail() {
        let mutex: Arc<SubscribableMutex<usize>> = Arc::default();
        {
            let mutex = Arc::clone(&mutex);
            async_std::task::spawn(async move {
                // Never gets to 10
                for i in 0..10 {
                    async_std::task::sleep(Duration::from_millis(100)).await;
                    mutex.set(i).await;
                }
            });
        }
        let result = mutex
            .wait_timeout_until(Duration::from_secs(2), |s| *s == 10)
            .await;
        assert!(result.is_err());
        assert_eq!(mutex.copied().await, 9);
    }

    #[async_std::test]
    async fn test_compare_and_set() {
        let mutex = SubscribableMutex::new(5usize);
        let subscriber = mutex.subscribe().await;

        assert_eq!(mutex.copied().await, 5);

        // Update
        mutex.compare_and_set(5, 10).await;
        assert_eq!(mutex.copied().await, 10);
        assert!(subscriber.try_recv().is_ok());

        // No update
        mutex.compare_and_set(5, 20).await;
        assert_eq!(mutex.copied().await, 10);
        assert!(subscriber.try_recv().is_err());
    }

    #[async_std::test]
    async fn test_subscriber() {
        let mutex = SubscribableMutex::new(5usize);
        let subscriber = mutex.subscribe().await;

        // No messages
        assert!(subscriber.try_recv().is_err());

        // sync message
        mutex.set(10).await;
        assert_eq!(subscriber.try_recv(), Ok(()));

        // async message
        mutex.set(20).await;
        assert_eq!(
            async_std::future::timeout(Duration::from_millis(10), subscriber.recv_async()).await,
            Ok(Ok(()))
        );

        // Validate we have 1 subscriber
        assert_eq!(mutex.subscribers.lock().await.len(), 1);

        // Validate that if we drop the subscriber, and notify, it'll be removed
        drop(subscriber);
        mutex.notify_change_subscribers().await;
        assert_eq!(mutex.subscribers.lock().await.len(), 0);
    }
}
