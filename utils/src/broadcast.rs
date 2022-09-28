#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]
use crate::art::async_block_on;
use crate::channel::{SendError, UnboundedReceiver, UnboundedRecvError, UnboundedSender};
use async_lock::RwLock;

use std::{
    collections::HashMap,
    fmt::Debug,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

/// Internals for a broadcast queue sender
struct BroadcastSenderInner<T> {
    /// Atomic int used for assigning ids
    count: AtomicUsize,
    /// Map of IDs to channels
    outputs: RwLock<HashMap<usize, UnboundedSender<T>>>,
}

/// Public interface for a broadcast queue sender
#[derive(Clone)]
pub struct BroadcastSender<T> {
    /// Underlying shared implementation details
    inner: Arc<BroadcastSenderInner<T>>,
}

impl<T> Debug for BroadcastSender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BroadcastSender")
            .field("inner", &"inner")
            .finish()
    }
}

impl<T> BroadcastSender<T>
where
    T: Clone,
{
    /// Asynchronously sends a value to all connected receivers
    ///
    /// # Errors
    ///
    /// Will return `Err` if one of the downstream receivers was disconnected without being properly
    /// dropped.
    pub async fn send_async(&self, item: T) -> Result<(), SendError<T>> {
        let map = self.inner.outputs.read().await;
        for sender in map.values() {
            sender.send(item.clone()).await?;
        }
        Ok(())
    }

    /// Asynchronously creates a new handle
    pub async fn handle_async(&self) -> BroadcastReceiver<T> {
        let id = self.inner.count.fetch_add(1, Ordering::SeqCst);
        let (send, recv) = crate::channel::unbounded();
        let mut map = self.inner.outputs.write().await;
        map.insert(id, send);
        BroadcastReceiver {
            id,
            output: recv,
            handle: self.clone(),
        }
    }

    /// Synchronously creates a new handle
    pub fn handle_sync(&self) -> BroadcastReceiver<T> {
        async_block_on(self.handle_async())
    }
}

/// Broadcast queue receiver
pub struct BroadcastReceiver<T> {
    /// ID for this receiver
    id: usize,
    /// Queue output
    output: UnboundedReceiver<T>,
    /// Handle to the sender internals
    handle: BroadcastSender<T>,
}

impl<T> BroadcastReceiver<T>
where
    T: Clone,
{
    /// Asynchronously receives a value
    ///
    /// # Errors
    ///
    /// Will return `Err` if the upstream sender has been disconnected.
    pub async fn recv_async(&mut self) -> Result<T, UnboundedRecvError> {
        self.output.recv().await
    }

    /// Returns a value, if one is available
    pub fn try_recv(&mut self) -> Option<T> {
        self.output.try_recv().ok()
    }

    /// Asynchronously clones this handle
    pub async fn clone_async(&self) -> Self {
        self.handle.handle_async().await
    }
}

impl<T> Drop for BroadcastReceiver<T> {
    /// Remove self from sender's map
    fn drop(&mut self) {
        let mut map = async_block_on(self.handle.inner.outputs.write());
        map.remove(&self.id);
    }
}

impl<T> Clone for BroadcastReceiver<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        async_block_on(self.clone_async())
    }
}

/// Creates a sender, receiver pair
pub fn channel<T: Clone>() -> (BroadcastSender<T>, BroadcastReceiver<T>) {
    let inner = BroadcastSenderInner {
        count: AtomicUsize::from(0),
        outputs: RwLock::new(HashMap::new()),
    };
    let input = BroadcastSender {
        inner: Arc::new(inner),
    };
    let output = input.handle_sync();
    (input, output)
}
