use async_std::{sync::RwLock, task::block_on};
use flume::{Receiver, RecvError, SendError, Sender};

use std::{
    collections::HashMap,
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
    outputs: RwLock<HashMap<usize, Sender<T>>>,
}

/// Public interface for a broadcast queue sender
#[derive(Clone)]
pub struct BroadcastSender<T> {
    /// Underlying shared implementation details
    inner: Arc<BroadcastSenderInner<T>>,
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
            sender.send_async(item.clone()).await?;
        }
        Ok(())
    }

    /// Synchronously sends a value to all connected receivers
    ///
    /// # Errors
    ///
    /// Will return `Err` if one of the downstream receivers was disconnected without being properly
    /// dropped.
    pub async fn send(&self, item: T) -> Result<(), SendError<T>> {
        let map = async_std::task::block_on(self.inner.outputs.read());
        for sender in map.values() {
            sender.send(item.clone())?;
        }
        Ok(())
    }

    /// Asynchronously creates a new handle
    pub async fn handle_async(&self) -> BroadcastReceiver<T> {
        let id = self.inner.count.fetch_add(1, Ordering::SeqCst);
        let (send, recv) = flume::unbounded();
        let mut map = self.inner.outputs.write().await;
        map.insert(id, send);
        BroadcastReceiver {
            id,
            output: recv,
            handle: self.clone(),
        }
    }
}

/// Broadcast queue receiver
pub struct BroadcastReceiver<T> {
    /// ID for this receiver
    id: usize,
    /// Queue output
    output: Receiver<T>,
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
    pub async fn recv_async(&self) -> Result<T, RecvError> {
        self.output.recv_async().await
    }

    /// Synchronously receives a value
    ///
    /// # Errors
    ///
    /// Will return `Err` if the upstream sender has been disconnected.
    pub fn recv(&self) -> Result<T, RecvError> {
        self.output.recv()
    }

    /// Asynchronously clones this handle
    pub async fn clone_async(&self) -> Self {
        self.handle.handle_async().await
    }
}

impl<T> Drop for BroadcastReceiver<T> {
    /// Remove self from sender's map
    fn drop(&mut self) {
        let mut map = block_on(self.handle.inner.outputs.write());
        map.remove(&self.id);
    }
}

impl<T> Clone for BroadcastReceiver<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        block_on(self.clone_async())
    }
}
