#![allow(clippy::module_name_repetitions)]

use cfg_if::cfg_if;
use futures::Stream;

cfg_if! {
    if #[cfg(feature = "channel-tokio")] {
        pub use tokio::sync::mpsc::error::{SendError as UnboundedSendError, TryRecvError as UnboundedTryRecvError};
        use tokio::sync::mpsc::{
            UnboundedSender as InnerSender,
            UnboundedReceiver as InnerReceiver
        };

        /// A receiver error returned from [`UnboundedReceiver`]'s `recv`
        #[derive(Debug, PartialEq, Eq, Clone)]
        pub struct UnboundedRecvError;

        impl std::fmt::Display for UnboundedRecvError {
            fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(fmt, stringify!(UnboundedRecvError))
            }
        }

        impl std::error::Error for UnboundedRecvError {}

        use tokio::sync::Mutex;

        /// An unbounded sender, created with [`unbounded`]
        pub struct UnboundedSender<T>(InnerSender<T>);
        /// An unbounded receiver, created with [`unbounded`]
        pub struct UnboundedReceiver<T>(Mutex<InnerReceiver<T>>);

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

        /// An unbounded sender, created with [`unbounded`]
        pub struct UnboundedSender<T>(Sender<T>);
        /// An unbounded receiver, created with [`unbounded`]
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

        /// An unbounded sender, created with [`unbounded`]
        pub struct UnboundedSender<T>(Sender<T>);
        /// An unbounded receiver, created with [`unbounded`]
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

/// Create an unbounded channel. This will dynamically allocate whenever the internal buffer is full and a new message is added.
///
/// The names of the [`UnboundedSender`] and [`UnboundedReceiver`] are specifically chosen to be less ergonomic than the [`bounded`] channels. Please consider using a bounded channel instead for performance reasons.
#[must_use]
pub fn unbounded<T>() -> (UnboundedSender<T>, UnboundedReceiver<T>) {
    cfg_if! {
        if #[cfg(feature = "channel-tokio")] {
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            let receiver = Mutex::new(receiver);
        } else if #[cfg(feature = "channel-flume")] {
            let (sender, receiver) = flume::unbounded();
        } else if #[cfg(feature = "channel-async-std")] {
            let (sender, receiver) = async_std::channel::unbounded();
        }
    }
    (UnboundedSender(sender), UnboundedReceiver(receiver))
}

impl<T> UnboundedSender<T> {
    /// Send a value to the sender half of this channel.
    ///
    /// # Errors
    ///
    /// This may fail if the receiver is dropped.
    #[allow(clippy::unused_async)] // under tokio this function is actually sync
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
    /// Receive a value from the receiver half of this channel.
    ///
    /// Will block until a value is received
    ///
    /// # Errors
    ///
    /// Will produce an error if all senders are dropped
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
    /// Turn this receiver into a stream.
    pub fn into_stream(self) -> impl Stream<Item = T>
    where
        T: 'static,
    {
        cfg_if! {
            if #[cfg(feature = "channel-async-std")] {
                self.0
            } else if #[cfg(feature = "channel-tokio")] {
                tokio_stream::wrappers::UnboundedReceiverStream::new(self.0.into_inner())
            } else if #[cfg(feature = "channel-flume")] {
                self.0.into_stream()
            }
        }
    }
    /// Try to receive a value from the receiver.
    ///
    /// # Errors
    ///
    /// Will return an error if no value is currently queued. This function will not block.
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
    /// Asynchronously wait for at least 1 value to show up, then will greedily try to receive values until this receiver would block. The resulting values are returned.
    ///
    /// It is guaranteed that the returning vec contains at least 1 value
    ///
    /// # Errors
    ///
    /// Will return an error if there was an error retrieving the first value.
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
    ///
    /// # Errors
    ///
    /// Will return an error if all the senders get dropped before this ends.
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
    /// Attempt to load the length of the messages in queue.
    ///
    /// On some implementations this value does not exist, and this will return `None`.
    #[allow(clippy::len_without_is_empty, clippy::unused_self)]
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
