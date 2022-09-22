//! Abstraction over channels. This will expose the following types:
//!
//! - [`unbounded`] returning an [`UnboundedSender`] and [`UnboundedReceiver`]
//! - [`bounded`] returning an [`BoundedSender`] and [`BoundedReceiver`]
//! - [`oneshot`] returning an [`OneShotSender`] and [`OneShotReceiver`]
//! - [`RecvError`] and [`SendError`]
//!
//! Which channel is selected depends on these feature flags:
//! - `channel-tokio` enables [tokio](https://docs.rs/tokio)
//! - `channel-async-std` enables [async-std](https://docs.rs/async-std)
//! - `channel-flume` enables [flume](https://docs.rs/flume)
//!
//! Note that exactly 1 of these features must be enabled. If 0 or 2+ are selected, you'll get compiler errors.
//!
//! Some of these implementations may not exist under the crate selected, in those cases they will be shimmed to another channel. e.g. `oneshot` might be implemented as `bounded(1)`.

#![allow(missing_docs)] // VKO: If you see this in the PR I have forgotten to remove it

use cfg_if::cfg_if;
use futures::{future::FusedFuture, FutureExt, Stream};
pub use inner::{RecvError, SendError, TryRecvError, TrySendError};

pub fn unbounded<T>() -> (UnboundedSender<T>, UnboundedReceiver<T>) {
    let (sender, receiver) = inner::unbounded();
    (UnboundedSender(sender), UnboundedReceiver(receiver))
}

pub struct UnboundedSender<T>(inner::UnboundedSender<T>);
impl<T> UnboundedSender<T> {
    pub async fn send(&self, msg: T) -> Result<(), SendError<T>> {
        self.0.send(msg).await
    }
    pub fn try_send(&self, msg: T) -> Result<(), TrySendError<T>> {
        self.0.try_send(msg)
    }
}

pub struct UnboundedReceiver<T>(inner::UnboundedReceiver<T>);
impl<T> UnboundedReceiver<T> {
    pub async fn recv(&self) -> Result<T, RecvError> {
        self.0.recv().await
    }
    pub fn recv_fuse(&self) -> impl FusedFuture<Output = Result<T, RecvError>> + '_ {
        self.recv().fuse()
    }
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.0.try_recv()
    }
    /// Drains the receiver from all messages in the queue, but will not poll for more messages
    pub fn drain(&self) -> Result<Vec<T>, RecvError> {
        let mut result = Vec::new();
        loop {
            match self.try_recv() {
                Ok(t) => result.push(t),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Closed) => return Err(RecvError),
            }
        }
        Ok(result)
    }
    pub async fn drain_at_least_one(&self) -> Result<Vec<T>, RecvError> {
        // Wait for the first message to come up
        let first = self.recv().await?;
        let mut ret = vec![first];
        loop {
            match self.try_recv() {
                Ok(x) => ret.push(x),
                Err(TryRecvError::Empty) => break,
                Err(_) => {
                    tracing::error!(
                        "Tried to empty {:?} queue but it disconnected while we were emptying it ({} items are being dropped)",
                        std::any::type_name::<Self>(),
                        ret.len()
                    );
                    return Err(RecvError);
                }
            }
        }
        Ok(ret)
    }
    pub fn len(&self) -> Option<usize> {
        cfg_if::cfg_if! {
            if #[cfg(feature = "channel-tokio")] {
                None
            } else {
                Some(self.0.len())
            }
        }
    }
}

pub fn bounded<T>(len: usize) -> (BoundedSender<T>, BoundedReceiver<T>) {
    let (sender, receiver) = inner::bounded(len);
    (BoundedSender(sender), BoundedReceiver(receiver))
}

pub struct BoundedSender<T>(inner::BoundedSender<T>);
impl<T> BoundedSender<T> {
    pub async fn send(&self, msg: T) -> Result<(), SendError<T>> {
        self.0.send(msg).await
    }
}

pub struct BoundedReceiver<T>(inner::BoundedReceiver<T>);
impl<T> BoundedReceiver<T> {
    pub async fn recv(&self) -> Result<T, RecvError> {
        self.0.recv().await
    }
    pub fn into_stream(self) -> impl Stream<Item = T> {
        cfg_if! {
            if #[cfg(feature = "channel-async-std")] {
                self.0
            } else if #[cfg(feature = "channel-tokio")] {
                tokio_stream::wrappers::ReceiverStream::new(self.0)
            } else if #[cfg(feature = "channel-flume")] {
                self.0.into_stream()
            }
        }
    }
    pub fn recv_fuse(&self) -> impl FusedFuture<Output = Result<T, RecvError>> + '_ {
        self.recv().fuse()
    }
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.0.try_recv()
    }
    pub async fn drain_at_least_one(&self) -> Result<Vec<T>, RecvError> {
        // Wait for the first message to come up
        let first = self.recv().await?;
        let mut ret = vec![first];
        loop {
            match self.try_recv() {
                Ok(x) => ret.push(x),
                Err(TryRecvError::Empty) => break,
                Err(_) => {
                    tracing::error!(
                        "Tried to empty {:?} queue but it disconnected while we were emptying it ({} items are being dropped)",
                        std::any::type_name::<Self>(),
                        ret.len()
                    );
                    return Err(RecvError);
                }
            }
        }
        Ok(ret)
    }
    /// Drains the receiver from all messages in the queue, but will not poll for more messages
    pub fn drain(&self) -> Result<Vec<T>, RecvError> {
        let mut result = Vec::new();
        loop {
            match self.try_recv() {
                Ok(t) => result.push(t),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Closed) => return Err(RecvError),
            }
        }
        Ok(result)
    }
}

pub fn oneshot<T>() -> (OneShotSender<T>, OneShotReceiver<T>) {
    let (sender, receiver) = inner::oneshot();
    (OneShotSender(sender), OneShotReceiver(receiver))
}

pub struct OneShotSender<T>(inner::OneShotSender<T>);
impl<T> OneShotSender<T> {
    pub fn send(self, msg: T) {
        self.0.send(msg);
    }
}
pub struct OneShotReceiver<T>(inner::OneShotReceiver<T>);
impl<T> OneShotReceiver<T> {
    pub async fn recv(self) -> Result<T, RecvError> {
        self.0.recv().await
    }
    pub fn recv_fuse(&self) -> impl FusedFuture<Output = Result<T, RecvError>> + '_ {
        self.0.recv().fuse()
    }
}

mod inner {
    cfg_if::cfg_if! {
        if #[cfg(feature = "channel-tokio")] {
            pub use tokio::sync::mpsc::{TryRecvError, TrySendError, RecvError, SendError};

            pub use tokio::sync::mpsc::{
                channel as bounded,
                Sender as BoundedSender,
                Receiver as BoundedReceiver,
            };

            pub use tokio::sync::mpsc::{
                unbounded_channel as unbounded,
                UnboundedSender,
                UnboundedReceiver,
            };

            pub use tokio::sync::oneshot::{
                channel as oneshot,
                Sender as OneShotSender,
                Receiver as OneShotReceiver,
            };
        } else if #[cfg(feature = "channel-async-std")] {
            pub use async_std::channel::{TryRecvError, TrySendError, RecvError, SendError};

            pub use async_std::channel::{
                bounded,
                Sender as BoundedSender,
                Receiver as BoundedReceiver,
            };

            pub use async_std::channel::{
                unbounded,
                Sender as UnboundedSender,
                Receiver as UnboundedReceiver,
            };

            /// Create a oneshot channel, which will allow sending a message exactly once
            pub fn oneshot<T>() -> (OneShotSender<T>, OneShotReceiver<T>) {
                let (sender, receiver) = bounded(1);
                (OneShotSender(sender), receiver)
            }
            pub struct OneShotSender<T>(BoundedSender<T>);
            impl<T> OneShotSender<T> {
                pub fn send(self, msg: T) {
                    if let Err(e) = crate::art::async_block_on(self.0.send(msg)) {
                        tracing::warn!("OneShotReceiver dropped {e:?}");
                    }
                }
            }
            pub use BoundedReceiver as OneShotReceiver;
        } else if #[cfg(feature = "channel-flume")] {
            pub use flume::{TryRecvError, RecvError, TrySendError, SendError};

            pub use flume::{
                bounded,
                Sender as BoundedSender,
                Receiver as BoundedReceiver,
            };
            pub use flume::{
                unbounded,
                Sender as UnboundedSender,
                Receiver as UnboundedReceiver,
            };

            /// Create a oneshot channel, which will allow sending a message exactly once
            pub fn oneshot<T>() -> (OneShotSender<T>, OneShotReceiver<T>) {
                bounded(1)
            }
            pub struct OneShotSender<T>(BoundedSender<T>);
            pub struct OneShotReceiver<T>(BoundedReceiver<T>);
        }
    }
}

// Clone impl
impl<T> Clone for UnboundedSender<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Clone for UnboundedReceiver<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Clone for BoundedSender<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Clone for BoundedReceiver<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

// Debug impl
impl<T> std::fmt::Debug for UnboundedSender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnboundedSender").finish()
    }
}
impl<T> std::fmt::Debug for UnboundedReceiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnboundedReceiver").finish()
    }
}
impl<T> std::fmt::Debug for BoundedSender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoundedSender").finish()
    }
}
impl<T> std::fmt::Debug for BoundedReceiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoundedReceiver").finish()
    }
}

impl<T> std::fmt::Debug for OneShotSender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OneShotSender").finish()
    }
}
impl<T> std::fmt::Debug for OneShotReceiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OneShotReceiver").finish()
    }
}
