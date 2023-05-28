use async_compatibility_layer::channel::{unbounded, UnboundedSender, UnboundedStream};
use async_lock::RwLock;
use std::sync::Arc;
use std::{
    collections::HashMap,
    pin::Pin,
    task::{Context, Poll},
};

use async_trait::async_trait;
use futures::Stream;

use crate::task::{FilterEvent, PassType};

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
    async fn subscribe(&self, filter: FilterEvent<Self::EventType>)
        -> (Self::StreamType, StreamId);

    /// unsubscribe from the stream
    async fn unsubscribe(&self, id: StreamId);
}

/// Event stream implementation using channels as the underlying primitive.
/// We want it to be cloneable
#[derive(Clone)]
pub struct ChannelStream<EVENT: PassType> {
    /// inner field. Useful for having the stream itself
    /// be clone
    inner: Arc<RwLock<ChannelStreamInner<EVENT>>>,
}

/// trick to make the event stream clonable
struct ChannelStreamInner<EVENT: PassType> {
    /// the subscribers to the channel
    subscribers: HashMap<StreamId, (FilterEvent<EVENT>, UnboundedSender<EVENT>)>,
    /// the next unused assignable id
    next_stream_id: StreamId,
}

impl<EVENT: PassType> ChannelStream<EVENT> {
    /// construct a new event stream
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ChannelStreamInner {
                subscribers: HashMap::new(),
                next_stream_id: 0,
            })),
        }
    }
}

impl<EVENT: PassType> Default for ChannelStream<EVENT> {
    fn default() -> Self {
        Self::new()
    }
}

impl<EVENT: PassType> SendableStream for UnboundedStream<EVENT> {}

#[async_trait]
impl<EVENT: PassType + 'static> EventStream for ChannelStream<EVENT> {
    type EventType = EVENT;
    type StreamType = UnboundedStream<Self::EventType>;

    /// publish an event to the event stream
    async fn publish(&self, event: Self::EventType) {
        let inner = self.inner.read().await;
        for (uid, (filter, sender)) in &inner.subscribers {
            if filter(&event) {
                match sender.send(event.clone()).await {
                    Ok(_) => (),
                    // error sending => stream is closed so remove it
                    Err(_) => self.unsubscribe(*uid).await,
                }
            }
        }
    }

    async fn subscribe(
        &self,
        filter: FilterEvent<Self::EventType>,
    ) -> (Self::StreamType, StreamId) {
        let mut inner = self.inner.write().await;
        let new_stream_id = inner.next_stream_id;
        let (s, r) = unbounded();
        inner.next_stream_id += 1;
        // NOTE: can never be already existing.
        // so, this should always return `None`
        inner.subscribers.insert(new_stream_id, (filter, s));
        (r.into_stream(), new_stream_id)
    }

    async fn unsubscribe(&self, uid: StreamId) {
        let mut inner = self.inner.write().await;
        inner.subscribers.remove(&uid);
    }
}
