use std::{marker::PhantomData, task::Poll};
use std::ops::Deref;

use std::sync::Arc;
use futures::{Future, StreamExt};
use futures::{future::LocalBoxFuture, stream::Fuse, Stream};
use pin_project::pin_project;

use crate::global_registry::{HotShotTaskId, GlobalRegistry};
use crate::task_state::HotShotTaskStatus;
use crate::{event_stream::{DummyStream, EventStream}, task_state::HotShotTaskState, global_registry::ShutdownFn};

pub trait PassType: Clone + std::fmt::Debug + Sync + Send {}
impl PassType for () {}

/// the task state
pub trait TaskState: std::fmt::Debug {}

pub trait HotShotTaskTypes {
    type Event: PassType;
    type State: TaskState;
    type EventStream: EventStream<EventType = Self::Event>;
    type Message: PassType;
    type MessageStream: Stream<Item = Self::Message>;
    // TODO this requires a trait bound
    type Error: std::error::Error;
}

pub struct HSTWithEvent<
    ERR: std::error::Error,
    EVENT: PassType,
    EVENT_STREAM: EventStream<EventType = EVENT>,
    STATE: TaskState,
> {
    _pd: PhantomData<(ERR, EVENT, EVENT_STREAM, STATE)>,
}

impl<ERR: std::error::Error, EVENT: PassType, EVENT_STREAM: EventStream<EventType = EVENT>, STATE: TaskState>
    HotShotTaskTypes for HSTWithEvent<ERR, EVENT, EVENT_STREAM, STATE>
{
    type Event = EVENT;
    type State = STATE;
    type EventStream = EVENT_STREAM;
    type Message = ();
    type MessageStream = DummyStream;
    type Error = ERR;
}

pub struct HSTWithMessage<ERR: std::error::Error, MSG: PassType, MSG_STREAM: Stream<Item = MSG>, STATE: TaskState> {
    _pd: PhantomData<(ERR, MSG, MSG_STREAM, STATE)>,
}

impl<ERR: std::error::Error, MSG: PassType, MSG_STREAM: Stream<Item = MSG>, STATE: TaskState> HotShotTaskTypes
    for HSTWithMessage<ERR, MSG, MSG_STREAM, STATE>
{
    type Event = ();
    type State = STATE;
    type EventStream = DummyStream;
    type Message = MSG;
    type MessageStream = MSG_STREAM;
    type Error = ERR;
}

pub struct HSTWithEventAndMessage<
    ERR: std::error::Error,
    EVENT: PassType,
    EVENT_STREAM: EventStream<EventType = EVENT>,
    MSG: PassType,
    MSG_STREAM: Stream<Item = MSG>,
    STATE: TaskState,
> {
    _pd: PhantomData<(ERR, EVENT, EVENT_STREAM, MSG, MSG_STREAM, STATE)>,
}

impl<
        ERR: std::error::Error,
        EVENT: PassType,
        EVENT_STREAM: EventStream<EventType = EVENT>,
        MSG: PassType,
        MSG_STREAM: Stream<Item = MSG>,
        STATE: TaskState,
    > HotShotTaskTypes for HSTWithEventAndMessage<ERR, EVENT, EVENT_STREAM, MSG, MSG_STREAM, STATE>
{
    type Event = ();
    type State = STATE;
    type EventStream = DummyStream;
    type Message = MSG;
    type MessageStream = MSG_STREAM;
    type Error = ERR;
}

/// hot shot task
#[pin_project]
pub struct HotShotTask<HST: HotShotTaskTypes> {
    /// the in progress future
    /// TODO does `'static` make sense here
    in_progress_fut: Option<LocalBoxFuture<'static, (Option<HotShotTaskCompleted<HST>>, HST::State)>>,
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
    state: Option<HST::State>,
    /// handler for events
    handle_event: Option<HandleEvent<HST>>,
    /// handler for messages
    handle_message: Option<HandleMessage<HST>>,
    /// handler for filtering events (to use with stream)
    filter_event: Option<FilterEvent<HST::Event>>,
}

pub enum HotShotTaskHandler<HST: HotShotTaskTypes> {
    HandleEvent(HandleEvent<HST>),
    HandleMessage(HandleMessage<HST>),
    FilterEvent(FilterEvent<HST::Event>),
    Shutdown(ShutdownFn),
}

/// event handler
pub struct HandleEvent<HST: HotShotTaskTypes>(Arc<dyn Fn(HST::Event, HST::State) -> LocalBoxFuture<'static, (Option<HotShotTaskCompleted<HST>>, HST::State)>>);
impl<HST: HotShotTaskTypes> Deref for HandleEvent<HST> {
    type Target = dyn Fn(HST::Event, HST::State) -> LocalBoxFuture<'static, (Option<HotShotTaskCompleted<HST>>, HST::State)>;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

pub struct HandleMessage<HST: HotShotTaskTypes>(Arc<dyn Fn(HST::Message, HST::State) -> LocalBoxFuture<'static, (Option<HotShotTaskCompleted<HST>>, HST::State)>>);
impl<HST: HotShotTaskTypes> Deref for HandleMessage<HST> {
    type Target = dyn Fn(HST::Message, HST::State) -> LocalBoxFuture<'static, (Option<HotShotTaskCompleted<HST>>, HST::State)>;

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
    pub fn register_handler(self, handler: HotShotTaskHandler<HST>) -> Self {
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
        Self { state: Some(state), ..self }
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
            state: Some(state),
            handle_event: None,
            handle_message: None,
            filter_event: None,
            shutdown_fn: None,
            message_stream: None,
            name,
            in_progress_fut: None
        }
    }

    pub async fn launch(self) -> HotShotTaskCompleted<HST>{
        self.await
    }
}

/// enum describing how the tasks completed
pub enum HotShotTaskCompleted<HST: HotShotTaskTypes>{
    /// the task shut down successfully
    ShutDown,
    /// the task encountered an error
    Error(HST::Error),
    /// the streams the task was listening for died
    StreamsDied,
    /// we somehow lost the state
    /// this is definitely a bug.
    LostState
}

// NOTE: this is a Future, but it could easily be a stream.
// but these are semantically equivalent because instead of
// returning when paused, we just return `Poll::Pending`
impl<HST: HotShotTaskTypes> Future for HotShotTask<HST> {
    type Output = HotShotTaskCompleted<HST>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        // useful if we ever need to use self later.
        // this doesn't consume the reference
        let projected = self.as_mut().project();

        // check if task is complete
        match projected.status.poll_next(cx) {
            Poll::Ready(Some(state_change)) => {
                match state_change{
                    HotShotTaskStatus::NotStarted | HotShotTaskStatus::Paused => {
                        return Poll::Pending;
                    }
                    HotShotTaskStatus::Running => {/* do nothing if we are running */}
                    HotShotTaskStatus::Completed => {
                        return Poll::Ready(HotShotTaskCompleted::ShutDown);
                    }
                }
            },
            // this primitive's stream will never end
            Poll::Ready(None) => unreachable!(),
            // if there's nothing, that's fine
            Poll::Pending => (),
        }


        if let Some(in_progress_fut) = projected.in_progress_fut {
            match in_progress_fut.as_mut().poll(cx) {
                Poll::Ready((result, state)) => {
                    *projected.in_progress_fut = None;
                    *projected.state = Some(state);
                    match result {
                        Some(completed) => return Poll::Ready(completed),
                        None => {},
                    }
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
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
                            let maybe_state = projected.state.take();
                            if let Some(state) = maybe_state {
                                let mut fut = handle_event(event, state);
                                match fut.as_mut().poll(cx) {
                                    Poll::Ready((result, state)) => {
                                        *projected.in_progress_fut = None;
                                        *projected.state = Some(state);
                                        match result {
                                            Some(completed) => return Poll::Ready(completed),
                                            None => {},
                                        }
                                    }
                                    Poll::Pending => {
                                        *projected.in_progress_fut = Some(fut);
                                        return Poll::Pending;
                                    }
                                }
                            } else {
                                return Poll::Ready(HotShotTaskCompleted::LostState);
                            }
                       }
                    }
                    // this is a fused future so `None` will come every time after the stream
                    // finishes
                    None => {
                        event_stream_finished = true;
                    }
                },
                Poll::Pending => (),
            }
        } else {
            event_stream_finished = true;
        }

        if let Some(message_stream) = message_stream {
            match message_stream.poll_next(cx) {
                Poll::Ready(maybe_msg) => match maybe_msg {
                    Some(msg) => {
                        if let Some(handle_msg) = projected.handle_message {
                            let maybe_state = projected.state.take();
                            if let Some(state) = maybe_state {
                                let mut fut = handle_msg(msg, state);
                                match fut.as_mut().poll(cx) {
                                    Poll::Ready((result, state)) => {
                                        *projected.in_progress_fut = None;
                                        *projected.state = Some(state);
                                        match result {
                                            Some(completed) => return Poll::Ready(completed),
                                            None => {},
                                        }
                                    },
                                    Poll::Pending => {
                                        *projected.in_progress_fut = Some(fut);
                                        // TODO add in logic
                                        return Poll::Pending
                                    },
                                };
                            } else {
                                return Poll::Ready(HotShotTaskCompleted::LostState);
                            }
                        }
                    }
                    // this is a fused future so `None` will come every time after the stream
                    // finishes
                    None => {
                        message_stream_finished = true;
                    }
                },
                Poll::Pending => {}
            }
        } else {
            event_stream_finished = true;
        }
        if message_stream_finished && event_stream_finished {
            return Poll::Ready(HotShotTaskCompleted::StreamsDied);
        }

        Poll::Pending
    }
}
