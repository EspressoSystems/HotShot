use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(feature = "channel-tokio")] {
        pub use tokio::sync::oneshot::{
            error::{TryRecvError as OneShotTryRecvError},
            Sender,
            Receiver
        };

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
        pub struct OneShotSender<T>(Sender<T>);
        /// A single-use sender, created with [`oneshot`]
        pub struct OneShotReceiver<T>(Receiver<T>);
    } else if #[cfg(feature = "channel-flume")] {
        pub use flume::{TryRecvError as OneShotTryRecvError, RecvError as OneShotRecvError};
        use flume::{Sender, Receiver};

        /// A single-use sender, created with [`oneshot`]
        pub struct OneShotSender<T>(Sender<T>);
        /// A single-use sender, created with [`oneshot`]
        pub struct OneShotReceiver<T>(Receiver<T>);
    } else if #[cfg(feature = "channel-async-std")] {
        pub use async_std::channel::{TryRecvError as OneShotTryRecvError, RecvError as OneShotRecvError};
        use async_std::channel::{Sender, Receiver};

        /// A single-use sender, created with [`oneshot`]
        pub struct OneShotSender<T>(Sender<T>);
        /// A single-use sender, created with [`oneshot`]
        pub struct OneShotReceiver<T>(Receiver<T>);
    }
}

/// Create a single-use channel. Under water this might be implemented as a `bounded(1)`, but more efficient implementations might exist.
///
/// Both the sender and receiver take `self` instead of `&self` or `&mut self`, so they can only be used once.
#[must_use]
pub fn oneshot<T>() -> (OneShotSender<T>, OneShotReceiver<T>) {
    cfg_if! {
        if #[cfg(feature = "channel-tokio")] {
            let (sender, receiver) = tokio::sync::oneshot::channel();
        } else if #[cfg(feature = "channel-flume")] {
            let (sender, receiver) = flume::bounded(1);
        } else if #[cfg(feature = "channel-async-std")] {
            let (sender, receiver) = async_std::channel::bounded(1);
        }
    }
    (OneShotSender(sender), OneShotReceiver(receiver))
}

impl<T> OneShotSender<T> {
    /// Send a message on this sender.
    ///
    /// If this fails because the receiver is dropped, a warning will be printed.
    pub fn send(self, msg: T) {
        cfg_if! {
            if #[cfg(feature = "channel-async-std")] {
                if self.0.try_send(msg).is_err() {
                    tracing::warn!("Could not send msg on OneShotSender, did the receiver drop?");
                }
            } else {
                if self.0.send(msg).is_err() {
                    tracing::warn!("Could not send msg on OneShotSender, did the receiver drop?");
                }
            }
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
        cfg_if! {
            if #[cfg(feature = "channel-tokio")] {
                self.0.await.map_err(Into::into)
            } else if #[cfg(feature = "channel-flume")] {
                self.0.recv_async().await
            } else {
                self.0.recv().await
            }
        }
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
