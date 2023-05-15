// Async tasks will be the building blocks for the run view refactor.
// An async task should be spannable by some trigger. That could be some other task completing or some event coming from the network.
//
// Task should be able to
// - publish messages to a shared event stream (e.g. a view sync task can publish a view change event)
// - register themselves with a shared task registry
// - consume events from the shared event stream. Every task must handle the shutdown event.
// - remove themselves from the registry on their competition

// The spawner of the task should be able to fire and forget the task if it makes sense.

use std::{
    marker::PhantomData,
    sync::{atomic::{AtomicUsize, Ordering}, Arc, Mutex}, task::Waker,
};

use async_compatibility_layer::channel::{UnboundedReceiver, UnboundedSender, UnboundedStream};
use async_trait::async_trait;
use atomic_enum::atomic_enum;
use futures::Stream;
use nll::nll_todo::nll_todo;
use serde::{Deserialize, Serialize};

pub trait Event: Clone + std::fmt::Debug + Sync + Send {}

// async event stream
#[async_trait]
trait EventStream {
    /// the type of event to process
    type EventType: Event;
    /// the type of stream to use
    type StreamType: Stream<Item = Self::EventType>;

    /// publish an event to the event stream
    async fn publish(&self, event: Self::EventType);

    /// subscribe to a particular set of events
    /// specified by `filter`. Filter returns true if the event should be propagated
    async fn subscribe(&self, filter: u64) -> Self::StreamType;
}

/// the event stream. We want it to be cloneable
#[derive(Clone)]
pub struct ChannelEventStream<EVENT: Event> {
    inner: Arc<ChannelEventStreamInner<EVENT>>,
}

pub struct FilterType<EVENT: Event>(Box<dyn Fn(&EVENT) -> bool>);

pub struct ChannelEventStreamInner<EVENT: Event> {
    _pd: PhantomData<EVENT>, // TODO
                             // subscribers: Vec<(FilterType<EVENT>, UnboundedSender<EVENT>, UnboundedReceiver<EVENT>)>,
}

#[async_trait]
impl<EVENT: Event> EventStream for ChannelEventStream<EVENT> {
    type EventType = EVENT;
    type StreamType = UnboundedStream<Self::EventType>;

    /// publish an event to the event stream
    async fn publish(&self, event: Self::EventType) {
        nll_todo()
    }

    /// subscribe to a particular set of events
    /// specified by `filter`. Filter returns true if the event should be propagated
    async fn subscribe(&self, filter: u64) -> Self::StreamType {
        nll_todo()
    }
}

// Nit: wish this was for u8 but sadly no
#[atomic_enum]
#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum HotShotTaskStatus {
    Running,
    NotStarted,
    Completed
}

// //Example EventStream impl
struct HotShotTask<EVENT: Event, STATE, STREAM: EventStream<EventType = EVENT>> {
    // state of the task
    status: AtomicHotShotTaskStatus,
    // TODO remove
    _pd: PhantomData<(EVENT, STREAM, STATE)>,
    /// registry
    shared_registry: GlobalRegistry,
    /// internal event stream
    shared_stream: STREAM,
    /// state
    state: STATE,
    /// handler for events
    handle_event: Option<HandleEvent<EVENT, STATE>>,
    /// handler for messages
    handle_message: Option<HandleMessage<STATE>>,
    /// handler for filtering events (to use with stream)
    filter_event: Option<FilterEvent<EVENT>>,
}

pub enum HotShotTaskHandler<EVENT, STATE> {
    HandleEvent(HandleEvent<EVENT, STATE>),
    HandleMessage(HandleMessage<STATE>),
    FilterEvent(FilterEvent<EVENT>),
}

///
pub struct HandleEvent<EVENT, STATE>(Box<dyn Fn(EVENT, &mut STATE) -> bool>);

/// TODO hardcode? or generic?
pub struct Message;

pub struct HandleMessage<STATE>(Box<dyn Fn(&mut STATE, Message) -> bool>);

pub struct FilterEvent<EVENT>(Box<dyn Fn(EVENT) -> bool>);

impl<EVENT: Event, STATE, STREAM: EventStream<EventType = EVENT>>
    HotShotTask<EVENT, STATE, STREAM>
{
    /// register a handler with the task
    pub fn register_handler(mut self, handler: HotShotTaskHandler<EVENT, STATE>) -> Self {
        match handler {
            HotShotTaskHandler::HandleEvent(handler) => {
                Self {
                  handle_event : Some(handler),
                  ..self
                }
            },
            HotShotTaskHandler::HandleMessage(handler) => {
                Self {
                  handle_message : Some(handler),
                  ..self
                }
            },
            HotShotTaskHandler::FilterEvent(handler) => {
                Self {
                  filter_event : Some(handler),
                  ..self
                }
            },
        }
    }

    /// create a new task
    pub fn new(
        shared_registry: GlobalRegistry,
        shared_stream: STREAM,
        state: STATE
    ) -> Self {
        Self {
            status: HotShotTaskStatus::NotStarted.into(),
            _pd: PhantomData,
            shared_registry,
            shared_stream,
            state,
            handle_event: None,
            handle_message: None,
            filter_event: None,
        }
    }

    /// fire and forget the task.
    pub fn register_and_run(mut self) {
        let running: bool = true;
        loop {}
    }
}

pub struct DeregisterFn(Box<dyn Fn()>);

/// the global registry provides a place to:
/// - inquire about the state of various tasks
/// - gracefully shut down tasks
// TODO should we also track the handle? Or is this only for book keeping?
// With a gc-ed bitvec, we'll be able to tell which tasks have been shut down
// threadsafe
#[derive(Debug)]
pub struct GlobalRegistry {
    /// writing to the status should gracefully shut down the task
    status_list: Vec<(AtomicHotShotTaskStatus, String)>,
}

impl GlobalRegistry {
    /// create new registry
    pub fn new() -> Self {
        Self {
            status_list: vec![]
        }
    }
    /// register with the garbage collector
    /// return a function to the caller (task) that can be used to deregister
    pub fn register(&self) -> DeregisterFn {
        DeregisterFn(Box::new(|| nll_todo()))
    }
    /// garbage collect `task_status`
    pub fn gc(&mut self) {
        nll_todo()
    }
}

/// the state
#[derive(Clone)]
pub struct HotShotTaskState {
    prev: Arc<AtomicHotShotTaskStatus>,
    next: Arc<AtomicHotShotTaskStatus>,
    wakers: Arc<Mutex<Vec<Waker>>>,
}

impl HotShotTaskState {
    pub fn new() -> Self {
        Self {
            prev: Arc::new(HotShotTaskStatus::NotStarted.into()),
            next: Arc::new(HotShotTaskStatus::NotStarted.into()),
            wakers: Arc::default()
        }
    }
    /// sets the state
    pub fn set_state(&self, state: HotShotTaskStatus) {
        self.next.swap(state, Ordering::Relaxed);
        // no panics, so can never be poisoned.
        let mut wakers = self.wakers.lock().unwrap();

        // drain the wakers
        for waker in wakers.drain(..) {
            waker.wake();
        }

    }
    /// gets a possibly stale version of the state
    pub fn get_status(&self) -> HotShotTaskStatus {
        self.next.load(Ordering::Relaxed)
    }
}

// TODO store as tuple
impl Stream for HotShotTaskState {
    type Item = HotShotTaskStatus;

    fn poll_next(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
        let next = self.next.load(Ordering::Relaxed);
        let prev = self.prev.swap(next, Ordering::Relaxed);
        // a new value has been set
        if prev != next {
            std::task::Poll::Ready(Some(next))
        } else {
            // no panics, so impossible to be poisoned
            self.wakers.lock().unwrap().push(cx.waker().clone());

            // no value has been set, poll again later
            std::task::Poll::Pending
        }
    }
}

#[cfg(test)]
pub mod test {
    use async_compatibility_layer::art::{async_spawn, async_sleep};
    use std::sync::Arc;

    #[cfg(test)]
    #[cfg_attr(
        feature = "tokio-executor",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
        )]
    #[cfg_attr(feature = "async-std-executor", async_std::test)]
    async fn test_stream() {
        setup_logging();
        use async_compatibility_layer::logging::setup_logging;
        use futures::StreamExt;
        tracing::error!("HELLO WORLD!");

        let mut task = crate::HotShotTaskState::new();

        let task_dup = task.clone();

        async_spawn(async move {
            async_sleep(std::time::Duration::from_secs(2)).await;
            task_dup.set_state(crate::HotShotTaskStatus::Running);
        });

        // spawn new task that sleeps then increments

        task.next().await;

    }
    // TODO test global registry using either global + lazy_static
    // or passing around global registry
}
