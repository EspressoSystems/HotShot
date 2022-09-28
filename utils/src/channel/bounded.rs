use cfg_if::cfg_if;
use futures::Stream;

cfg_if! {
    if #[cfg(feature = "channel-tokio")] {
        pub use tokio::sync::mpsc::error::{SendError, TryRecvError};

        use tokio::sync::mpsc::{
            Sender as InnerSender,
            Receiver as InnerReceiver,
        };

        /// A receiver error returned from [`Receiver`]'s `recv`
        #[derive(Debug, PartialEq, Eq)]
        pub struct RecvError;

        impl std::fmt::Display for RecvError {
            fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(fmt, stringify!(RecvError))
            }
        }

        impl std::error::Error for RecvError {}

        /// A bounded sender, created with [`bounded`]
        pub struct Sender<T>(InnerSender<T>);
        /// A bounded receiver, created with [`bounded`]
        pub struct Receiver<T>(InnerReceiver<T>);

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

        /// A bounded sender, created with [`bounded`]
        pub struct Sender<T>(InnerSender<T>);
        /// A bounded receiver, created with [`bounded`]
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

        /// A bounded sender, created with [`channel`]
        pub struct Sender<T>(InnerSender<T>);
        /// A bounded receiver, created with [`channel`]
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

/// Create a bounded sender/receiver pair, limited to `len` messages at a time.
#[must_use]
pub fn bounded<T>(len: usize) -> (Sender<T>, Receiver<T>) {
    cfg_if! {
        if #[cfg(feature = "channel-tokio")] {
            let (sender, receiver) = tokio::sync::mpsc::channel(len);
        } else if #[cfg(feature = "channel-flume")] {
            let (sender, receiver) = flume::bounded(len);
        } else if #[cfg(feature = "channel-async-std")] {
            let (sender, receiver) = async_std::channel::bounded(len);
        }
    }
    (Sender(sender), Receiver(receiver))
}

impl<T> Sender<T> {
    /// Send a value to the channel. May return a [`SendError`] if the receiver is dropped
    ///
    /// # Errors
    ///
    /// Will return an error if the receiver is dropped
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
    /// Receive a value from te channel. This will async block until a value is received, or until a [`RecvError`] is encountered.
    ///
    /// # Errors
    ///
    /// Will return an error if the sender is dropped
    pub async fn recv(&mut self) -> Result<T, RecvError> {
        cfg_if! {
            if #[cfg(feature = "channel-flume")] {
                self.0.recv_async().await
            } else if #[cfg(feature = "channel-tokio")] {
                self.0.recv().await.ok_or(RecvError)
            } else {
                self.0.recv().await
            }
        }
    }
    /// Turn this recever into a stream. This may fail on some implementations if multiple references of a receiver exist
    pub fn into_stream(self) -> impl Stream<Item = T>
    where
        T: 'static,
    {
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
    /// Try to receive a channel from the receiver. Will return immediately if there is no value available.
    ///
    /// # Errors
    ///
    /// Will return an error if the sender is dropped
    pub fn try_recv(&mut self) -> Result<T, TryRecvError> {
        self.0.try_recv()
    }
    /// Asynchronously wait for at least 1 value to show up, then will greedily try to receive values until this receiver would block. The resulting values are returned.
    ///
    /// It is guaranteed that the returning vec contains at least 1 value
    ///
    /// # Errors
    ///
    /// Will return an error if the sender is dropped
    pub async fn drain_at_least_one(&mut self) -> Result<Vec<T>, RecvError> {
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
    /// Will return an error if the sender is dropped
    pub fn drain(&mut self) -> Result<Vec<T>, RecvError> {
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
