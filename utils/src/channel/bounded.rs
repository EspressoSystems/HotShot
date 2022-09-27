use cfg_if::cfg_if;
use futures::{future::FusedFuture, FutureExt, Stream};

cfg_if! {
    if #[cfg(feature = "channel-tokio")] {
        pub use tokio::sync::mpsc::error::{SendError, TryRecvError};

        use tokio::sync::mpsc::{
            Sender as InnerSender,
            Receiver as InnerReceiver,
        };

        #[derive(Debug, PartialEq, Eq)]
        pub struct RecvError;

        impl std::fmt::Display for RecvError {
            fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(fmt, stringify!(RecvError))
            }
        }

        impl std::error::Error for RecvError {}

        use tokio::sync::Mutex;
        use std::sync::Arc;

        pub struct Sender<T>(InnerSender<T>);
        pub struct Receiver<T>(Arc<Mutex<InnerReceiver<T>>>);

        /// Turn a `TryRecvError` into a `RecvError` if it's not `Empty`
        fn try_recv_error_to_recv_error(e: TryRecvError) -> Option<RecvError> {
            match e {
                TryRecvError::Empty => None,
                TryRecvError::Disconnected => Some(RecvError),
            }
        }
    } else if #[cfg(feature = "channel-flume")] {
        pub use flume::{SendError, TryRecvError, RecvError};

        use flume::{
            Sender as InnerSender,
            Receiver as InnerReceiver,
        };

        pub struct Sender<T>(InnerSender<T>);
        pub struct Receiver<T>(InnerReceiver<T>);

        /// Turn a `TryRecvError` into a `RecvError` if it's not `Empty`
        fn try_recv_error_to_recv_error(e: TryRecvError) -> Option<RecvError> {
            match e {
                TryRecvError::Empty => None,
                TryRecvError::Disconnected => Some(RecvError::Disconnected),
            }
        }
    } else if #[cfg(feature = "channel-async-std")] {
        pub use async_std::channel::{SendError, TryRecvError, RecvError};

        use async_std::channel::{
            Sender as InnerSender,
            Receiver as InnerReceiver,
        };

        pub struct Sender<T>(InnerSender<T>);
        pub struct Receiver<T>(InnerReceiver<T>);

        /// Turn a `TryRecvError` into a `RecvError` if it's not `Empty`
        fn try_recv_error_to_recv_error(e: TryRecvError) -> Option<RecvError> {
            match e {
                TryRecvError::Empty => None,
                TryRecvError::Closed => Some(RecvError),
            }
        }
    }
}

#[must_use]
pub fn bounded<T>(len: usize) -> (Sender<T>, Receiver<T>) {
    cfg_if! {
        if #[cfg(feature = "channel-tokio")] {
            let (sender, receiver) = tokio::sync::mpsc::channel(len);
            let receiver = Arc::new(Mutex::new(receiver));
        } else if #[cfg(feature = "channel-flume")] {
            let (sender, receiver) = flume::bounded(len);
        } else if #[cfg(feature = "channel-async-std")] {
            let (sender, receiver) = async_std::channel::bounded(len);
        }
    }
    (Sender(sender), Receiver(receiver))
}

impl<T> Sender<T> {
    pub async fn send(&self, msg: T) -> Result<(), SendError<T>> {
        cfg_if! {
            if #[cfg(feature = "channel-flume")] {
                self.0.send_async(msg).await
            } else {
                self.0.send(msg).await
            }
        }
    }
}

impl<T> Receiver<T> {
    pub async fn recv(&self) -> Result<T, RecvError> {
        cfg_if! {
            if #[cfg(feature = "channel-flume")] {
                self.0.recv_async().await
            } else if #[cfg(feature = "channel-tokio")] {
                self.0.lock().await.recv().await.ok_or(RecvError)
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
                let mutex = Arc::try_unwrap(self.0).ok()?;
                Some(tokio_stream::wrappers::ReceiverStream::new(mutex.into_inner()))
            } else if #[cfg(feature = "channel-flume")] {
                Some(self.0.into_stream())
            }
        }
    }
    #[must_use]
    pub fn recv_fuse(&self) -> impl FusedFuture<Output = Result<T, RecvError>> + '_ {
        self.recv().fuse()
    }
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        cfg_if! {
            if #[cfg(feature = "channel-tokio")] {
                // TODO: Check if this actually doesn't block
                crate::art::async_block_on(self.0.lock()).try_recv()
            } else {
                self.0.try_recv()
            }
        }
    }
    pub async fn drain_at_least_one(&self) -> Result<Vec<T>, RecvError> {
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
    pub fn drain(&self) -> Result<Vec<T>, RecvError> {
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
}

// Clone impl
impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
// Debug impl
impl<T> std::fmt::Debug for Sender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sender").finish()
    }
}
impl<T> std::fmt::Debug for Receiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Receiver").finish()
    }
}
