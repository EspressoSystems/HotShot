use async_compatibility_layer::channel::{
    unbounded, Sender, UnboundedReceiver, UnboundedSender, UnboundedStream,
};
use async_lock::RwLock;
#[allow(clippy::non_camel_case_types)]
// Async tasks will be the building blocks for the run view refactor.
// An async task should be spannable by some trigger. That could be some other task completing or some event coming from the network.
//
// Task should be able to
// - publish messages to a shared event stream (e.g. a view sync task can publish a view change event)
// - register themselves with a shared task registry
// - consume events from the shared event stream. Every task must handle the shutdown event.
// - remove themselves from the registry on their competition

// The spawner of the task should be able to fire and forget the task if it makes sense.
use async_stream::stream;
use async_trait::async_trait;
use atomic_enum::atomic_enum;
use either::Either;
use futures::{stream::Fuse, Future, Stream, StreamExt};
use nll::nll_todo::nll_todo;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    marker::PhantomData,
    ops::Deref,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll, Waker},
};
// NOTE use pin_project here because we're already bring in procedural macros elsewhere
// so there is no reason to use pin_project_lite
use pin_project::pin_project;

// NOTE: yoinked /from async-std
// except this is executor agnostic (doesn't rely on async-std streamext/fuse)
// TODO move this to async-compatibility-layer
#[pin_project]
/// Stream returned by the [`merge`](super::StreamExt::merge) method.
pub struct Merge<T, U> {
    #[pin]
    a: Fuse<T>,
    #[pin]
    b: Fuse<U>,
    // When `true`, poll `a` first, otherwise, `poll` b`.
    a_first: bool,
}

impl<T, U> Merge<T, U> {
    pub fn new(a: T, b: U) -> Merge<T, U>
    where
        T: Stream,
        U: Stream,
    {
        Merge {
            a: a.fuse(),
            b: b.fuse(),
            a_first: true,
        }
    }
}

impl<T, U> Stream for Merge<T, U>
where
    T: Stream,
    U: Stream<Item = T::Item>,
{
    type Item = T::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T::Item>> {
        let me = self.project();
        let a_first = *me.a_first;

        // Toggle the flag
        *me.a_first = !a_first;

        if a_first {
            poll_next(me.a, me.b, cx)
        } else {
            poll_next(me.b, me.a, cx)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (a_lower, a_upper) = self.a.size_hint();
        let (b_lower, b_upper) = self.b.size_hint();

        let upper = match (a_upper, b_upper) {
            (Some(a_upper), Some(b_upper)) => Some(a_upper + b_upper),
            _ => None,
        };

        (a_lower + b_lower, upper)
    }
}

fn poll_next<T, U>(
    first: Pin<&mut T>,
    second: Pin<&mut U>,
    cx: &mut Context<'_>,
) -> Poll<Option<T::Item>>
where
    T: Stream,
    U: Stream<Item = T::Item>,
{
    use Poll::*;

    let mut done = true;

    match first.poll_next(cx) {
        Ready(Some(val)) => return Ready(Some(val)),
        Ready(None) => {}
        Pending => done = false,
    }

    match second.poll_next(cx) {
        Ready(Some(val)) => return Ready(Some(val)),
        Ready(None) => {}
        Pending => done = false,
    }

    if done {
        Ready(None)
    } else {
        Pending
    }
}

pub trait PassType: Clone + std::fmt::Debug + Sync + Send {}
impl PassType for () {}

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

    async fn publish(&self, event: Self::EventType) {}

    async fn subscribe(
        &self,
        filter: FilterEvent<Self::EventType>,
    ) -> (Self::StreamType, StreamId) {
        (DummyStream, 0)
    }

    async fn unsubscribe(&self, id: StreamId) {}
}

/// this is only used for indexing
pub type StreamId = usize;

// async event stream
#[async_trait]
pub trait EventStream: Clone {
    /// the type of event to process
    type EventType: PassType;
    /// the type of stream to use
    type StreamType: Stream<Item = Self::EventType>;

    /// publish an event to the event stream
    async fn publish(&self, event: Self::EventType);

    /// subscribe to a particular set of events
    /// specified by `filter`. Filter returns true if the event should be propagated
    async fn subscribe(&self, filter: FilterEvent<Self::EventType>)
        -> (Self::StreamType, StreamId);

    async fn unsubscribe(&self, id: StreamId);
}

/// the event stream. We want it to be cloneable
#[derive(Clone)]
pub struct ChannelEventStream<EVENT: PassType> {
    inner: Arc<RwLock<ChannelEventStreamInner<EVENT>>>,
}

pub struct ChannelEventStreamInner<EVENT: PassType> {
    subscribers: HashMap<StreamId, (FilterEvent<EVENT>, UnboundedSender<EVENT>)>,
    next_stream_id: StreamId,
}

impl<EVENT: PassType> ChannelEventStream<EVENT> {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ChannelEventStreamInner {
                subscribers: HashMap::new(),
                next_stream_id: 0,
            })),
        }
    }
}

#[async_trait]
impl<EVENT: PassType> EventStream for ChannelEventStream<EVENT> {
    type EventType = EVENT;
    type StreamType = UnboundedStream<Self::EventType>;

    /// publish an event to the event stream
    async fn publish(&self, event: Self::EventType) {
        let inner = self.inner.read().await;
        for (_, (filter, sender)) in &inner.subscribers {
            if filter(&event) {
                match sender.send(event).await {
                    Ok(_) => todo!(),
                    Err(_) => todo!(),
                }
            }
        }
    }

    /// subscribe to a particular set of events
    /// specified by `filter`. Filter returns true if the event should be propagated
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

// Nit: wish this was for u8 but sadly no
#[atomic_enum]
#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum HotShotTaskStatus {
    /// the task hasn't started running
    NotStarted = 0,
    /// the task is running
    Running = 1,
    /// NOTE: not useful generally, but VERY useful for byzantine nodes
    /// and testing malfunctions
    /// we'll have a granular way to, from the registry, stop a task momentarily
    Paused = 2,
    /// the task completed
    Completed = 3,
}

pub trait TaskState: std::fmt::Debug {}

pub trait HotShotTaskTypes {
    type Event: PassType;
    type State: TaskState;
    type EventStream: EventStream<EventType = Self::Event>;
    type Message: PassType;
    type MessageStream: Stream<Item = Self::Message>;
}

pub struct HST<STATE: TaskState> {
    _pd: PhantomData<STATE>,
}

impl<STATE: TaskState> HotShotTaskTypes for HST<STATE> {
    type Event = ();
    type State = STATE;
    type EventStream = DummyStream;
    type Message = ();
    type MessageStream = DummyStream;
}

pub struct HSTWithEvent<
    STATE: TaskState,
    EVENT: PassType,
    EVENT_STREAM: EventStream<EventType = EVENT>,
> {
    _pd: PhantomData<(STATE, EVENT, EVENT_STREAM)>,
}

impl<STATE: TaskState, EVENT: PassType, EVENT_STREAM: EventStream<EventType = EVENT>>
    HotShotTaskTypes for HSTWithEvent<STATE, EVENT, EVENT_STREAM>
{
    type Event = EVENT;
    type State = STATE;
    type EventStream = EVENT_STREAM;
    type Message = ();
    type MessageStream = DummyStream;
}

pub struct HSTWithMessage<STATE: TaskState, MSG: PassType, MSG_STREAM: Stream<Item = MSG>> {
    _pd: PhantomData<(STATE, MSG, MSG_STREAM)>,
}

impl<STATE: TaskState, MSG: PassType, MSG_STREAM: Stream<Item = MSG>> HotShotTaskTypes
    for HSTWithMessage<STATE, MSG, MSG_STREAM>
{
    type Event = ();
    type State = STATE;
    type EventStream = DummyStream;
    type Message = MSG;
    type MessageStream = MSG_STREAM;
}

pub struct HSTWithEventAndMessage<
    STATE: TaskState,
    EVENT: PassType,
    EVENT_STREAM: EventStream<EventType = EVENT>,
    MSG: PassType,
    MSG_STREAM: Stream<Item = MSG>,
> {
    _pd: PhantomData<(STATE, EVENT, EVENT_STREAM, MSG, MSG_STREAM)>,
}

impl<
        STATE: TaskState,
        EVENT: PassType,
        EVENT_STREAM: EventStream<EventType = EVENT>,
        MSG: PassType,
        MSG_STREAM: Stream<Item = MSG>,
    > HotShotTaskTypes for HSTWithEventAndMessage<STATE, EVENT, EVENT_STREAM, MSG, MSG_STREAM>
{
    type Event = ();
    type State = STATE;
    type EventStream = DummyStream;
    type Message = MSG;
    type MessageStream = MSG_STREAM;
}

/// hot shot task
#[pin_project]
pub struct HotShotTask<HST: HotShotTaskTypes> {
    /// name of task
    name: String,
    /// state of the task
    #[pin]
    status: HotShotTaskState,
    /// function to shut down the task
    /// if we're tracking with a global registry
    shutdown_fn: Option<ShutdownFn>,
    /// shared stream
    #[pin]
    event_stream: Option<Fuse<<HST::EventStream as EventStream>::StreamType>>,
    /// stream of messages
    #[pin]
    message_stream: Option<Fuse<HST::MessageStream>>,
    /// state
    state: HST::State,
    /// handler for events
    handle_event: Option<HandleEvent<HST>>,
    /// handler for messages
    handle_message: Option<HandleMessage<HST>>,
    /// handler for filtering events (to use with stream)
    filter_event: Option<FilterEvent<HST::Event>>,
}

/// TODO revive this explicitly for tasks
/// convenience launcher for tasks
// pub struct TaskLauncher<
//     const N: usize,
//     HST: HotS
// > {
//     tasks: [HotShotTask<EVENT, STATE, STREAM, MSG, MSG_STREAM>; N],
// }

/// TODO these tasks need to be async. Having them be syncronous isn't going to cut it.
pub enum HotShotTaskHandler<HST: HotShotTaskTypes> {
    HandleEvent(HandleEvent<HST>),
    HandleMessage(HandleMessage<HST>),
    FilterEvent(FilterEvent<HST::Event>),
    Shutdown(ShutdownFn),
}

/// event handler
pub struct HandleEvent<HST: HotShotTaskTypes>(Box<dyn Fn(HST::Event, &mut HST::State) -> bool>);
impl<HST: HotShotTaskTypes> Deref for HandleEvent<HST> {
    type Target = dyn Fn(HST::Event, &mut HST::State) -> bool;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

/// TODO hardcode? or generic?
pub struct Message;

pub struct HandleMessage<HST: HotShotTaskTypes>(Box<dyn Fn(&mut HST::State, HST::Message) -> bool>);
impl<HST: HotShotTaskTypes> Deref for HandleMessage<HST> {
    type Target = dyn Fn(&mut HST::State, HST::Message) -> bool;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

/// arc for `Clone`
#[derive(Clone)]
pub struct FilterEvent<EVENT: PassType>(Arc<dyn Fn(&EVENT) -> bool + Send + 'static + Sync>);

impl<EVENT: PassType> Default for FilterEvent<EVENT> {
    fn default() -> Self {
        Self(Arc::new(|_| true))
    }
}

impl<EVENT: PassType> Deref for FilterEvent<EVENT> {
    type Target = dyn Fn(&EVENT) -> bool + Send + 'static + Sync;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl<HST: HotShotTaskTypes> HotShotTask<HST> {
    /// register a handler with the task
    pub fn register_handler(mut self, handler: HotShotTaskHandler<HST>) -> Self {
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
            HotShotTaskHandler::Shutdown(handler) => Self {
                shutdown_fn: Some(handler),
                ..self
            }
        }
    }

    pub async fn with_event_stream(self, stream: HST::EventStream, filter: FilterEvent<HST::Event>) -> Self {
        // TODO perhaps GC the event stream
        // (unsunscribe)
        Self {
            event_stream: Some(stream.subscribe(filter).await.0.fuse()),
            ..self
        }
    }

    pub async fn with_message_stream(self, stream: HST::MessageStream) -> Self {
        Self {
            message_stream: Some(stream.fuse()),
            ..self
        }
    }

    pub async fn with_state(self, state: HST::State) -> Self {
        Self { state, ..self }
    }

    pub async fn register_with_registry(self, registry: &mut GlobalRegistry) -> (Self, HotShotTaskId) {
        let (shutdown_fn, id) = registry.register(&self.name, self.status.clone()).await;
        (Self {
            shutdown_fn: Some(shutdown_fn),
            ..self
        }, id)
    }

    /// create a new task
    pub fn new(state: HST::State, name: String) -> Self {
        Self {
            status: HotShotTaskState::new(),
            event_stream: None,
            state,
            handle_event: None,
            handle_message: None,
            filter_event: None,
            shutdown_fn: None,
            message_stream: None,
            name,
        }
    }

    pub async fn launch(self) -> HotShotTaskCompleted<HST> {
        self.await
    }
}

pub enum HotShotTaskCompleted<HST: HotShotTaskTypes> {
    ShutDown,
    // TODO this needs to contain error variants but this creates a circular dependency on crates.
    // Maybe this should be a generic?
    Error,
    StreamsDied,
    Paused(HotShotTask<HST>),
}

// TODO make this a stream instead of a poll
// This way, we can error out the task, but gracefully restart it
impl<HST: HotShotTaskTypes> Future for HotShotTask<HST> {
    type Output = HotShotTaskCompleted<HST>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        // useful if we ever need to use self later.
        // this doesn't consume the reference
        let mut projected = self.as_mut().project();

        match projected.status.poll_next(cx) {
            Poll::Ready(Some(state_change)) => {
                match state_change{
                    HotShotTaskStatus::NotStarted | HotShotTaskStatus::Paused => {
                        return Poll::Pending;
                    }
                    HotShotTaskStatus::Running => {}
                    HotShotTaskStatus::Completed => {
                        return Poll::Ready(HotShotTaskCompleted::ShutDown);
                    }
                }
            },
            Poll::Ready(None) => unreachable!(),
            Poll::Pending => todo!(),
        }

        let event_stream = projected.event_stream.as_pin_mut();

        let message_stream = projected.message_stream.as_pin_mut();

        let mut event_stream_finished = false;
        let mut message_stream_finished = false;

        if let Some(shared_stream) = event_stream {
            match shared_stream.poll_next(cx) {
                Poll::Ready(maybe_event) => match maybe_event {
                    Some(event) => {
                        if let Some(handle_event) = projected.handle_event {
                            handle_event(event, &mut projected.state);
                        }
                    }
                    None => {
                        event_stream_finished = true;
                    }
                },
                Poll::Pending => (),
            }
        }

        if let Some(message_stream) = message_stream {
            match message_stream.poll_next(cx) {
                Poll::Ready(maybe_msg) => match maybe_msg {
                    Some(msg) => {
                        if let Some(handle_msg) = projected.handle_message {
                            handle_msg(projected.state, msg);
                        }
                    }
                    None => {
                        message_stream_finished = true;
                    }
                },
                Poll::Pending => {}
            }
        }
        if message_stream_finished && event_stream_finished {
            return Poll::Ready(HotShotTaskCompleted::StreamsDied);
        }

        Poll::Pending
    }
}

pub struct ShutdownFn(Box<dyn Fn()>);

impl Deref for ShutdownFn {
    type Target = dyn Fn();

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}
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

    /// register with the global registry
    /// return a function to the caller (task) that can be used to deregister
    /// returns a function to call to shut down the task
    /// and the unique identifier of the task
    pub async fn register(&mut self, name: &str, status: HotShotTaskState) -> (ShutdownFn, HotShotTaskId) {
        let mut list = self.status_list.write().await;
        let next_id = list.len();
        let new_entry = (status.clone(), name.to_string());
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
    /// this is a way to subscribe to state changes from the taskstatus
    /// since HotShotTaskStatus implements stream
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
    // task waker
    // task_waker: Option<Waker>
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
            // task_waker: None
        }
    }

    pub fn from_status(state: Arc<AtomicHotShotTaskStatus>) -> Self {
        let prev_state = AtomicHotShotTaskStatus::new(state.load(Ordering::SeqCst));
        Self {
            prev: Arc::new(prev_state),
            next: state,
            wakers: Arc::default(),
        }
    }

    /// sets the state
    pub fn set_state(&self, state: HotShotTaskStatus) {
        self.next.swap(state, Ordering::SeqCst);
        // no panics, so can never be poisoned.
        let mut wakers = self.wakers.lock().unwrap();

        // drain the wakers
        for waker in wakers.drain(..) {
            waker.wake();
        }
    }
    /// gets a possibly stale version of the state
    pub fn get_status(&self) -> HotShotTaskStatus {
        self.next.load(Ordering::SeqCst)
    }
}

impl Stream for HotShotTaskState {
    type Item = HotShotTaskStatus;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let next = self.next.load(Ordering::SeqCst);
        let prev = self.prev.swap(next, Ordering::SeqCst);
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
