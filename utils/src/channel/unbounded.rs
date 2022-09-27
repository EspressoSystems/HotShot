#![allow(clippy::module_name_repetitions)]

use cfg_if::cfg_if;
use futures::{future::FusedFuture, FutureExt, Stream};

cfg_if! {
    if #[cfg(feature = "channel-tokio")] {
        pub use tokio::sync::mpsc::error::{SendError as UnboundedSendError, TryRecvError as UnboundedTryRecvError};
        use tokio::sync::mpsc::{
            UnboundedSender as InnerSender,
            UnboundedReceiver as InnerReceiver
        };

        #[derive(Debug, PartialEq, Eq, Clone)]
        pub struct UnboundedRecvError;

        impl std::fmt::Display for UnboundedRecvError {
            fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(fmt, stringify!(UnboundedRecvError))
            }
        }

        impl std::error::Error for UnboundedRecvError {}

        use tokio::sync::Mutex;
        use std::sync::Arc;

        pub struct UnboundedSender<T>(InnerSender<T>);
        pub struct UnboundedReceiver<T>(Arc<Mutex<InnerReceiver<T>>>);

        /// Turn a `TryRecvError` into a `RecvError` if it's not `Empty`
        fn try_recv_error_to_recv_error(e: UnboundedTryRecvError) -> Option<UnboundedRecvError> {
            match e {
                UnboundedTryRecvError::Empty => None,
                UnboundedTryRecvError::Disconnected => Some(UnboundedRecvError),
            }
        }
    } else if #[cfg(feature = "channel-flume")] {
        pub use flume::{SendError as UnboundedSendError, TryRecvError as UnboundedTryRecvError, RecvError as UnboundedRecvError };
        use flume::{Sender, Receiver};

        pub struct UnboundedSender<T>(Sender<T>);
        pub struct UnboundedReceiver<T>(Receiver<T>);

        /// Turn a `TryRecvError` into a `RecvError` if it's not `Empty`
        fn try_recv_error_to_recv_error(e: UnboundedTryRecvError) -> Option<UnboundedRecvError> {
            match e {
                UnboundedTryRecvError::Empty => None,
                UnboundedTryRecvError::Disconnected => Some(UnboundedRecvError::Disconnected),
            }
        }
    } else if #[cfg(feature = "channel-async-std")] {
        pub use async_std::channel::{SendError as UnboundedSendError, TryRecvError as UnboundedTryRecvError, RecvError as UnboundedRecvError };
        use async_std::channel::{Sender, Receiver};

        pub struct UnboundedSender<T>(Sender<T>);
        pub struct UnboundedReceiver<T>(Receiver<T>);

        /// Turn a `TryRecvError` into a `RecvError` if it's not `Empty`
        fn try_recv_error_to_recv_error(e: UnboundedTryRecvError) -> Option<UnboundedRecvError> {
            match e {
                UnboundedTryRecvError::Empty => None,
                UnboundedTryRecvError::Closed => Some(UnboundedRecvError),
            }
        }
    }
}

#[must_use]
pub fn unbounded<T>() -> (UnboundedSender<T>, UnboundedReceiver<T>) {
    cfg_if! {
        if #[cfg(feature = "channel-tokio")] {
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            let receiver = Arc::new(Mutex::new(receiver));
        } else if #[cfg(feature = "channel-flume")] {
            let (sender, receiver) = flume::unbounded();
        } else if #[cfg(feature = "channel-async-std")] {
            let (sender, receiver) = async_std::channel::unbounded();
        }
    }
    (UnboundedSender(sender), UnboundedReceiver(receiver))
}

impl<T> UnboundedSender<T> {
    pub async fn send(&self, msg: T) -> Result<(), UnboundedSendError<T>> {
        cfg_if! {
            if #[cfg(feature = "channel-flume")] {
                self.0.send_async(msg).await
            } else if #[cfg(feature = "channel-tokio")] {
                self.0.send(msg)
            } else {
                self.0.send(msg).await
            }
        }
    }
}

impl<T> UnboundedReceiver<T> {
    pub async fn recv(&self) -> Result<T, UnboundedRecvError> {
        cfg_if! {
            if #[cfg(feature = "channel-flume")] {
                self.0.recv_async().await
            } else if #[cfg(feature = "channel-tokio")] {
                self.0.lock().await.recv().await.ok_or(UnboundedRecvError)
            } else {
                self.0.recv().await
            }
        }
    }
    #[must_use]
    pub fn into_stream(self) -> Option<impl Stream<Item = T>>
    where
        T: 'static,
    {
        cfg_if! {
            if #[cfg(feature = "channel-async-std")] {
                Some(self.0)
            } else if #[cfg(feature = "channel-tokio")] {
                // TODO: It would be really nice if we could make this not fail
                // Currently we need to take ownership of `self.0`, but we can't do that if there are multiple references
                // but we need to have `UnboundedReceiver` in a mutex because in tokio the `recv()` take `&mut self`
                let mutex = Arc::try_unwrap(self.0).ok()?;
                Some(tokio_stream::wrappers::UnboundedReceiverStream::new(mutex.into_inner()))
            } else if #[cfg(feature = "channel-flume")] {
                Some(self.0.into_stream())
            }
        }
    }
    #[must_use]
    pub fn recv_fuse(&self) -> impl FusedFuture<Output = Result<T, UnboundedRecvError>> + '_ {
        self.recv().fuse()
    }
    pub fn try_recv(&self) -> Result<T, UnboundedTryRecvError> {
        cfg_if! {
            if #[cfg(feature = "channel-tokio")] {
                // TODO: Check if this actually doesn't block
                crate::art::async_block_on(self.0.lock()).try_recv()
            } else {
                self.0.try_recv()
            }
        }
    }
    pub async fn drain_at_least_one(&self) -> Result<Vec<T>, UnboundedRecvError> {
        // Wait for the first message to come up
        let first = self.recv().await?;
        let mut ret = vec![first];
        loop {
            match self.try_recv() {
                Ok(x) => ret.push(x),
                Err(e) => {
                    if let Some(e) = try_recv_error_to_recv_error(e) {
                        tracing::error!(
                            "Tried to empty {:?} queue but it disconnected while we were emptying it ({} items are being dropped)",
                            std::any::type_name::<Self>(),
                            ret.len()
                        );
                        return Err(e);
                    }
                    break;
                }
            }
        }
        Ok(ret)
    }
    /// Drains the receiver from all messages in the queue, but will not poll for more messages
    pub fn drain(&self) -> Result<Vec<T>, UnboundedRecvError> {
        let mut result = Vec::new();
        loop {
            match self.try_recv() {
                Ok(t) => result.push(t),
                Err(e) => {
                    if let Some(e) = try_recv_error_to_recv_error(e) {
                        return Err(e);
                    }
                    break;
                }
            }
        }
        Ok(result)
    }
    #[allow(clippy::len_without_is_empty)]
    #[must_use]
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

// Clone impl
impl<T> Clone for UnboundedSender<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
// impl<T> Clone for UnboundedReceiver<T> {
//     fn clone(&self) -> Self {
//         Self(self.0.clone())
//     }
// }
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
