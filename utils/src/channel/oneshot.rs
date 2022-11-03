/// inner module, used to group feature-specific imports
#[cfg(feature = "channel-tokio")]
mod inner {
    pub use tokio::sync::oneshot::{error::TryRecvError as OneShotTryRecvError, Receiver, Sender};

    // There is a `RecvError` in tokio but we don't use it because we can't create an instance of `RecvError` so we
    // can't turn a `TryRecvError` into a `RecvError`.

    /// Error that occurs when the [`OneShotReceiver`] could not receive a message.
    #[derive(Debug, PartialEq, Eq)]
    pub struct OneShotRecvError;

    impl From<tokio::sync::oneshot::error::RecvError> for OneShotRecvError {
        fn from(_: tokio::sync::oneshot::error::RecvError) -> Self {
            Self
        }
    }

    impl std::fmt::Display for OneShotRecvError {
        fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(fmt, stringify!(OneShotRecvError))
        }
    }

    impl std::error::Error for OneShotRecvError {}

    /// A single-use sender, created with [`oneshot`]
    pub struct OneShotSender<T>(pub(super) Sender<T>);
    /// A single-use sender, created with [`oneshot`]
    pub struct OneShotReceiver<T>(pub(super) Receiver<T>);

    /// Create a single-use channel. Under water this might be implemented as a `bounded(1)`, but more efficient implementations might exist.
    ///
    /// Both the sender and receiver take `self` instead of `&self` or `&mut self`, so they can only be used once.
    #[must_use]
    pub fn oneshot<T>() -> (OneShotSender<T>, OneShotReceiver<T>) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        (OneShotSender(sender), OneShotReceiver(receiver))
    }
}

/// inner module, used to group feature-specific imports
#[cfg(feature = "channel-flume")]
mod inner {
    use flume::{Receiver, Sender};
    pub use flume::{RecvError as OneShotRecvError, TryRecvError as OneShotTryRecvError};

    /// A single-use sender, created with [`oneshot`]
    pub struct OneShotSender<T>(pub(super) Sender<T>);
    /// A single-use sender, created with [`oneshot`]
    pub struct OneShotReceiver<T>(pub(super) Receiver<T>);

    /// Create a single-use channel. Under water this might be implemented as a `bounded(1)`, but more efficient implementations might exist.
    ///
    /// Both the sender and receiver take `self` instead of `&self` or `&mut self`, so they can only be used once.
    #[must_use]
    pub fn oneshot<T>() -> (OneShotSender<T>, OneShotReceiver<T>) {
        let (sender, receiver) = flume::bounded(1);
        (OneShotSender(sender), OneShotReceiver(receiver))
    }
}

/// inner module, used to group feature-specific imports
#[cfg(feature = "channel-async-std")]
mod inner {
    use async_std::channel::{Receiver, Sender};
    pub use async_std::channel::{
        RecvError as OneShotRecvError, TryRecvError as OneShotTryRecvError,
    };

    /// A single-use sender, created with [`oneshot`]
    pub struct OneShotSender<T>(pub(super) Sender<T>);
    /// A single-use sender, created with [`oneshot`]
    pub struct OneShotReceiver<T>(pub(super) Receiver<T>);

    /// Create a single-use channel. Under water this might be implemented as a `bounded(1)`, but more efficient implementations might exist.
    ///
    /// Both the sender and receiver take `self` instead of `&self` or `&mut self`, so they can only be used once.
    #[must_use]
    pub fn oneshot<T>() -> (OneShotSender<T>, OneShotReceiver<T>) {
        let (sender, receiver) = async_std::channel::bounded(1);
        (OneShotSender(sender), OneShotReceiver(receiver))
    }
}

pub use inner::*;

impl<T> OneShotSender<T> {
    /// Send a message on this sender.
    ///
    /// If this fails because the receiver is dropped, a warning will be printed.
    pub fn send(self, msg: T) {
        #[cfg(feature = "channel-async-std")]
        if self.0.try_send(msg).is_err() {
            tracing::warn!("Could not send msg on OneShotSender, did the receiver drop?");
        }
        #[cfg(not(feature = "channel-async-std"))]
        if self.0.send(msg).is_err() {
            tracing::warn!("Could not send msg on OneShotSender, did the receiver drop?");
        }
    }
}

impl<T> OneShotReceiver<T> {
    /// Receive a value from this oneshot receiver. If the sender is dropped, an error is returned.
    ///
    /// # Errors
    ///
    /// Will return an error if the sender channel is dropped
    pub async fn recv(self) -> Result<T, OneShotRecvError> {
        #[cfg(feature = "channel-tokio")]
        let result = self.0.await.map_err(Into::into);
        #[cfg(feature = "channel-flume")]
        let result = self.0.recv_async().await;
        #[cfg(feature = "channel-async-std")]
        let result = self.0.recv().await;

        result
    }
}

// Debug impl
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
