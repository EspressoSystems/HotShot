//! Internal utility functions and traits

use async_trait::async_trait;

/// Utility functions implemented on `Receiver` types used in the codebase.
///
/// Currently this is only implemented on [`flume::Receiver`] but this should be usable for any `Receiver` type.
#[async_trait]
pub trait ReceiverExt<T> {
    /// Asynchronously wait for the first message in this receiver, then try to pop as many messages as possible from the receiver until it would block.
    ///
    /// If the first message could not be waited on this will return `None`.
    async fn recv_async_drain(&self) -> Option<Vec<T>>;
}

#[async_trait]
impl<T: Send> ReceiverExt<T> for flume::Receiver<T> {
    async fn recv_async_drain(&self) -> Option<Vec<T>> {
        // Wait for the first message to come up
        let first = self.recv_async().await.ok()?;
        let mut ret = vec![first];
        loop {
            match self.try_recv() {
                Ok(x) => ret.push(x),
                Err(flume::TryRecvError::Empty) => break,
                Err(flume::TryRecvError::Disconnected) => {
                    tracing::error!(
                        "Tried to empty {:?} queue but it disconnected while we were emptying it ({} items are being dropped)",
                        std::any::type_name::<Self>(),
                        ret.len()
                    );
                    return None;
                }
            }
        }
        Some(ret)
    }
}
