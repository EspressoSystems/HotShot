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
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::Waker,
};

use async_compatibility_layer::channel::{
    Sender, UnboundedReceiver, UnboundedSender, UnboundedStream,
};
use async_lock::RwLock;
use async_trait::async_trait;
use atomic_enum::atomic_enum;
use either::Either;
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
    NotStarted = 0,
    Running = 1,
    /// NOTE: not useful generally, but VERY useful for byzantine nodes
    /// and testing malfunctions
    /// we'll have a granular way to, from the registry, stop a task momentarily
    Paused = 2,
    Completed = 3,
}

// //Example EventStream impl
struct HotShotTask<
    EVENT: Event,
    STATE,
    STREAM: EventStream<EventType = EVENT>, /* , MESSAGE: Clone + Sync + Send */
> {
    /// name of task
    name: String,
    /// state of the task
    status: AtomicHotShotTaskStatus,
    /// function to shut down the task
    /// if we're tracking with a global registry
    shutdown_fn: Option<ShutdownFn>,
    // TODO remove
    _pd: PhantomData<(EVENT, STREAM, STATE)>,
    /// internal event stream
    shared_stream: Option<STREAM>,
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

/// event handler
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
            HotShotTaskHandler::HandleEvent(handler) => Self {
                handle_event: Some(handler),
                ..self
            },
            HotShotTaskHandler::HandleMessage(handler) => Self {
                handle_message: Some(handler),
                ..self
            },
            HotShotTaskHandler::FilterEvent(handler) => Self {
                filter_event: Some(handler),
                ..self
            },
        }
    }

    pub fn with_stream(self, stream: STREAM) -> Self {
        Self {
            shared_stream: Some(stream),
            ..self
        }
    }

    /// create a new task
    pub fn new(state: STATE, name: String) -> Self {
        Self {
            status: HotShotTaskStatus::NotStarted.into(),
            _pd: PhantomData,
            shared_stream: None,
            state,
            handle_event: None,
            handle_message: None,
            filter_event: None,
            shutdown_fn: None,
            name,
        }
    }

    /// fire and forget the task.
    pub fn register_and_run(mut self) {
        /// register with the registry
        let running: bool = true;
        loop {}
    }
}

pub struct ShutdownFn(Box<dyn Fn()>);
/// id of task. Usize instead of u64 because
/// used for primarily for indexing
pub type HotShotTaskId = usize;

/// the global registry provides a place to:
/// - inquire about the state of various tasks
/// - gracefully shut down tasks
#[derive(Debug, Clone)]
pub struct GlobalRegistry {
    /// up-to-date shared list of statuses
    /// only used if `state_cpy` is out of date
    /// or if appending
    status_list: Arc<RwLock<Vec<(HotShotTaskState, String)>>>,
    /// possibly stale read version of state
    /// NOTE: must include entire state in order to
    /// support both incrementing and reading
    /// writing to the status should gracefully shut down the task
    state_cpy: Vec<(HotShotTaskState, String)>,
}

/// function to modify state
struct Modifier(Box<dyn Fn(&HotShotTaskState) -> Either<HotShotTaskStatus, bool>>);

impl GlobalRegistry {
    /// create new registry
    pub fn spawn_new() -> Self {
        Self {
            status_list: Arc::new(RwLock::new(vec![])),
            state_cpy: vec![],
        }
    }

    /// register with the garbage collector
    /// return a function to the caller (task) that can be used to deregister
    /// returns a function to call to shut down the task
    /// and the unique identifier of the task
    pub async fn register(&mut self, name: String) -> (ShutdownFn, HotShotTaskId) {
        let mut list = self.status_list.write().await;
        let next_id = list.len();
        let new_entry = (HotShotTaskState::new(), name);
        let new_entry_dup = new_entry.0.clone();
        list.push(new_entry);

        for i in self.state_cpy.len()..list.len() {
            self.state_cpy.push(list[i].clone());
        }

        let shutdown_fn = ShutdownFn(Box::new(move || {
            new_entry_dup.set_state(HotShotTaskStatus::Completed);
        }));
        (shutdown_fn, next_id)
    }

    /// update the cache
    async fn update_cache(&mut self) {
        let list = self.status_list.read().await;
        if list.len() > self.state_cpy.len() {
            for i in self.state_cpy.len()..list.len() {
                self.state_cpy.push(list[i].clone());
            }
        }
    }

    /// internal function to run `modifier` on `uid`
    /// if it exists
    async fn operate_on_task(
        &mut self,
        uid: HotShotTaskId,
        modifier: Modifier,
    ) -> Either<HotShotTaskStatus, bool> {
        // the happy path
        if uid < self.state_cpy.len() {
            modifier.0(&self.state_cpy[uid].0)
        }
        // the sad path
        else {
            self.update_cache().await;
            if uid < self.state_cpy.len() {
                modifier.0(&self.state_cpy[uid].0)
            } else {
                Either::Right(false)
            }
        }
    }

    /// set `uid`'s state to paused
    /// returns true upon success and false if `uid` is not registered
    pub async fn pause_task(&mut self, uid: HotShotTaskId) -> bool {
        let modifier = Modifier(Box::new(|state| {
            state.set_state(HotShotTaskStatus::Paused);
            Either::Right(true)
        }));
        match self.operate_on_task(uid, modifier).await {
            Either::Left(_) => unreachable!(),
            Either::Right(b) => b,
        }
    }

    /// set `uid`'s state to running
    /// returns true upon success and false if `uid` is not registered
    pub async fn run_task(&mut self, uid: HotShotTaskId) -> bool {
        let modifier = Modifier(Box::new(|state| {
            state.set_state(HotShotTaskStatus::Running);
            Either::Right(true)
        }));
        match self.operate_on_task(uid, modifier).await {
            Either::Left(_) => unreachable!(),
            Either::Right(b) => b,
        }
    }

    /// if the `uid` is registered with the global registry
    /// return its task status
    pub async fn get_task_state(&mut self, uid: HotShotTaskId) -> Option<HotShotTaskStatus> {
        let modifier = Modifier(Box::new(|state| Either::Left(state.get_status())));
        match self.operate_on_task(uid, modifier).await {
            Either::Left(state) => Some(state),
            Either::Right(false) => None,
            Either::Right(true) => unreachable!(),
        }
    }

    /// shut down a task from a different thread
    /// returns true if succeeded
    /// returns false if the task does not exist
    pub async fn shutdown_task(&mut self, uid: usize) -> bool {
        let modifier = Modifier(Box::new(|state| {
            state.set_state(HotShotTaskStatus::Completed);
            Either::Right(true)
        }));
        match self.operate_on_task(uid, modifier).await {
            Either::Left(_) => unreachable!(),
            Either::Right(b) => b,
        }
    }
}

/// The state of a task
/// `AtomicHotShotTaskStatus` + book keeping to notify btwn tasks
#[derive(Clone)]
pub struct HotShotTaskState {
    /// previous status
    prev: Arc<AtomicHotShotTaskStatus>,
    /// next status
    next: Arc<AtomicHotShotTaskStatus>,
    /// using `std::sync::mutex` here because it's faster than async's version
    wakers: Arc<Mutex<Vec<Waker>>>,
}

impl std::fmt::Debug for HotShotTaskState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HotShotTaskState")
            .field("status", &self.get_status())
            .finish()
    }
}

impl HotShotTaskState {
    /// create a new state
    pub fn new() -> Self {
        Self {
            prev: Arc::new(HotShotTaskStatus::NotStarted.into()),
            next: Arc::new(HotShotTaskStatus::NotStarted.into()),
            wakers: Arc::default(),
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

impl Stream for HotShotTaskState {
    type Item = HotShotTaskStatus;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
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
    use async_compatibility_layer::art::{async_sleep, async_spawn};

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

        let mut task = crate::HotShotTaskState::new();

        let task_dup = task.clone();

        async_spawn(async move {
            async_sleep(std::time::Duration::from_secs(2)).await;
            task_dup.set_state(crate::HotShotTaskStatus::Running);
        });

        // spawn new task that sleeps then increments

        assert_eq!(
            task.next().await.unwrap(),
            crate::HotShotTaskStatus::Running
        );
    }
    // TODO test global registry using either global + lazy_static
    // or passing around global registry
}
