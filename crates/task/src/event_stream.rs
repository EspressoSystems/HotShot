use async_compatibility_layer::channel::UnboundedStream;
use async_lock::RwLock;
use core::panic;
use std::{
    collections::HashMap,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use async_trait::async_trait;
use futures::Stream;

use crate::task::{FilterEvent, PassType};
use futures::ready;
/// A receiver broadcast channel based on tokio::broadcast.  
/// This/// Channel handles forwarding messages to channels which were cloned from it.
/// I don't love this impl for 2 reasons.  The receiver depends on the receiver it was
/// cloned from to get all messages.  A receiver may get messages in a different order
/// than they were sent.
use tokio::sync::broadcast::{channel, error::RecvError, Receiver, Sender};

use tokio_util::sync::ReusableBoxFuture;

use std::fmt;

/// A wrapper around [`tokio::sync::broadcast::Receiver`] that implements [`Stream`].
///
/// [`tokio::sync::broadcast::Receiver`]: struct@tokio::sync::broadcast::Receiver
/// [`Stream`]: trait@crate::Stream
#[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
pub struct BroadcastStream<T: Clone> {
    inner: ReusableBoxFuture<'static, (Result<T, RecvError>, BroadcastReceiver<T>)>,
}

async fn make_future<T: Clone>(
    mut rx: BroadcastReceiver<T>,
) -> (Result<T, RecvError>, BroadcastReceiver<T>) {
    let result = rx.recv().await;
    (result, rx)
}

impl<T: 'static + Clone + Send> BroadcastStream<T> {
    /// Create a new `BroadcastStream`.
    pub fn new(rx: BroadcastReceiver<T>) -> Self {
        Self {
            inner: ReusableBoxFuture::new(make_future(rx)),
        }
    }
}

impl<T: 'static + Clone + Send> Stream for BroadcastStream<T> {
    type Item = T;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let (result, rx) = ready!(self.inner.poll(cx));
        self.inner.set(make_future(rx));
        match result {
            Ok(item) => Poll::Ready(Some(item)),
            Err(RecvError::Closed) => Poll::Ready(None),
            Err(RecvError::Lagged(n)) => {
                panic!("Lagging stream");
            }
        }
    }
}

impl<T: Clone> fmt::Debug for BroadcastStream<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BroadcastStream").finish()
    }
}

impl<T: 'static + Clone + Send> From<BroadcastReceiver<T>> for BroadcastStream<T> {
    fn from(recv: BroadcastReceiver<T>) -> Self {
        Self::new(recv)
    }
}
pub struct BroadcastReceiver<T: Clone> {
    recevier: Receiver<T>,
    clone_sender: Sender<T>,
    clone_receiver: Option<Receiver<T>>,
    clone_recvs_expected: usize,
}

impl<T: Clone> BroadcastReceiver<T> {
    pub async fn recv(&mut self) -> Result<T, RecvError> {
        // if the receiver we cloned from has a value for us, take that, if not wait for our own
        let val = if self.clone_recvs_expected > 0
            && self.clone_receiver.as_ref().is_some_and(|c| !c.is_empty())
        {
            let v = self.clone_receiver.as_mut().unwrap().recv().await?;
            self.clone_recvs_expected -= 1;
            if self.clone_recvs_expected == 0 {
                self.clone_receiver = None;
            }
            v
        } else {
            self.recevier.recv().await?
        };

        // If we were cloned, send the value along
        if self.clone_sender.receiver_count() > 0 {
            let _ = self.clone_sender.send(val.clone());
        }
        Ok(val)
    }
}

impl<T: Clone> Clone for BroadcastReceiver<T> {
    fn clone(&self) -> Self {
        let (tx, _) = channel(1024);
        let queued_msgs = self.recevier.len();
        let clone_receiver = if queued_msgs > 0 {
            Some(self.clone_sender.subscribe())
        } else {
            None
        };
        Self {
            recevier: self.recevier.resubscribe(),
            clone_sender: tx,
            clone_receiver: clone_receiver,
            clone_recvs_expected: queued_msgs,
        }
    }
}
/// a stream that does nothing.
/// it's immediately closed
#[derive(Clone)]
pub struct DummyStream;

impl Stream for DummyStream {
    type Item = ();

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}

#[async_trait]
impl EventStream for DummyStream {
    type EventType = ();

    type StreamType = DummyStream;

    async fn publish(&self, _event: Self::EventType) {}

    async fn subscribe(
        &self,
        _filter: FilterEvent<Self::EventType>,
    ) -> (Self::StreamType, StreamId) {
        (DummyStream, 0)
    }

    async fn unsubscribe(&self, _id: StreamId) {}
}

impl SendableStream for DummyStream {}

/// this is only used for indexing
pub type StreamId = usize;

/// a stream that plays nicely with async
pub trait SendableStream: Stream + Sync + Send + 'static {}

/// Async pub sub event stream
/// NOTE: static bound indicates that if the type points to data, that data lives for the lifetime
/// of the program
#[async_trait]
pub trait EventStream: Clone + 'static + Sync + Send {
    /// the type of event to process
    type EventType: PassType;
    /// the type of stream to use
    type StreamType: SendableStream<Item = Self::EventType>;

    /// publish an event to the event stream
    async fn publish(&self, event: Self::EventType);

    /// subscribe to a particular set of events
    /// specified by `filter`. Filter returns true if the event should be propagated
    /// TODO (justin) rethink API, we might be able just to use `StreamExt::filter` and `Filter`
    /// That would certainly be cleaner
    async fn subscribe(&self, filter: FilterEvent<Self::EventType>)
        -> (Self::StreamType, StreamId);

    /// unsubscribe from the stream
    async fn unsubscribe(&self, id: StreamId);
}

/// Event stream implementation using channels as the underlying primitive.
/// We want it to be cloneable
pub struct ChannelStream<EVENT: PassType> {
    sender: Sender<EVENT>,
    receiver: BroadcastStream<EVENT>,
}

impl<EVENT: PassType> Clone for ChannelStream<EVENT> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            receiver: BroadcastStream::new(self.receiver.inner.clone()),
        }
    }
}

impl<EVENT: PassType> ChannelStream<EVENT> {
    /// construct a new event stream
    #[must_use]
    pub fn new() -> Self {
        let (tx, rx) = channel(1024);
        let (clone_tx, _) = channel(1024);
        Self {
            sender: tx,
            receiver: BroadcastStream::new(BroadcastReceiver {
                recevier: rx,
                clone_sender: clone_tx,
                clone_receiver: None,
                clone_recvs_expected: 0,
            }),
        }
    }
}

impl<EVENT: PassType> Default for ChannelStream<EVENT> {
    fn default() -> Self {
        Self::new()
    }
}

impl<EVENT: PassType> SendableStream for BroadcastStream<EVENT> {}

#[async_trait]
impl<EVENT: PassType + 'static> EventStream for ChannelStream<EVENT> {
    type EventType = EVENT;
    type StreamType = BroadcastStream<Self::EventType>;

    /// publish an event to the event stream
    fn publish(&self, event: Self::EventType) {
        self.sender.send(event);
    }

    async fn subscribe(
        &self,
        filter: FilterEvent<Self::EventType>,
    ) -> (Self::StreamType, StreamId) {
        (
            BroadcastStream::<EVENT>::new(self.receiver),
            0,
        )
    }

    async fn unsubscribe(&self, uid: StreamId) {}
}

#[cfg(test)]
pub mod test {
    use crate::{event_stream::EventStream, StreamExt};
    use async_compatibility_layer::art::{async_sleep, async_spawn};
    use std::time::Duration;

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum TestMessage {
        One,
        Two,
        Three,
    }

    #[cfg_attr(
        async_executor_impl = "tokio",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(async_executor_impl = "async-std", async_std::test)]
    async fn test_channel_stream_basic() {
        use crate::task::FilterEvent;

        use super::ChannelStream;

        let channel_stream = ChannelStream::<TestMessage>::new();
        let (mut stream, _) = channel_stream.subscribe(FilterEvent::default()).await;
        let dup_channel_stream = channel_stream.clone();

        let dup_dup_channel_stream = channel_stream.clone();

        async_spawn(async move {
            let (mut stream, _) = dup_channel_stream.subscribe(FilterEvent::default()).await;
            assert!(stream.next().await.unwrap() == TestMessage::Three);
            assert!(stream.next().await.unwrap() == TestMessage::One);
            assert!(stream.next().await.unwrap() == TestMessage::Two);
        });

        async_spawn(async move {
            dup_dup_channel_stream.publish(TestMessage::Three).await;
            dup_dup_channel_stream.publish(TestMessage::One).await;
            dup_dup_channel_stream.publish(TestMessage::Two).await;
        });
        async_sleep(Duration::new(3, 0)).await;

        assert!(stream.next().await.unwrap() == TestMessage::Three);
        assert!(stream.next().await.unwrap() == TestMessage::One);
        assert!(stream.next().await.unwrap() == TestMessage::Two);
    }

    #[cfg_attr(
        async_executor_impl = "tokio",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(async_executor_impl = "async-std", async_std::test)]
    async fn test_channel_stream_xtreme() {
        use crate::task::FilterEvent;

        use super::ChannelStream;

        let channel_stream = ChannelStream::<TestMessage>::new();
        let mut streams = Vec::new();

        for _i in 0..1000 {
            let dup_channel_stream = channel_stream.clone();
            let (stream, _) = dup_channel_stream.subscribe(FilterEvent::default()).await;
            streams.push(stream);
        }

        let dup_dup_channel_stream = channel_stream.clone();

        for _i in 0..1000 {
            let mut stream = streams.pop().unwrap();
            async_spawn(async move {
                for event in [TestMessage::One, TestMessage::Two, TestMessage::Three] {
                    for _ in 0..100 {
                        assert!(stream.next().await.unwrap() == event);
                    }
                }
            });
        }

        async_spawn(async move {
            for event in [TestMessage::One, TestMessage::Two, TestMessage::Three] {
                for _ in 0..100 {
                    dup_dup_channel_stream.publish(event.clone()).await;
                }
            }
        });
    }
}
