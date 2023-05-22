use std::ops::Deref;
use std::task::Poll;

use futures::{future::LocalBoxFuture, stream::Fuse, Stream};
use futures::{Future, StreamExt};
use pin_project::pin_project;
use std::sync::Arc;

use crate::event_stream::StreamId;
use crate::global_registry::{GlobalRegistry, HotShotTaskId};
use crate::task_impls::TaskBuilder;
use crate::task_state::TaskStatus;
use crate::{event_stream::EventStream, global_registry::ShutdownFn, task_state::TaskState};

/// restrictions on types we wish to pass around.
/// Includes messages and events
pub trait PassType: Clone + std::fmt::Debug + Sync + Send {}
impl PassType for () {}

/// the task state
pub trait TS: std::fmt::Debug {}

/// group of types needed for a hotshot task
pub trait HotShotTaskTypes {
    /// the event type from the event stream
    type Event: PassType;
    /// the state of the task
    type State: TS;
    /// the global event stream
    type EventStream: EventStream<EventType = Self::Event>;
    /// the message stream to receive
    type Message: PassType;
    /// the steam of messages from other tasks
    type MessageStream: Stream<Item = Self::Message>;
    /// the error to return
    type Error: std::error::Error;

    /// build a task
    /// NOTE: done here and not on `TaskBuilder` because
    /// we want specific checks done on each
    /// NOTE: all generics implement `Sized`, but this bound is
    /// NOT applied to `Self` unless we specify
    fn build(builder: TaskBuilder<Self>) -> HST<Self>
    where
        Self: Sized;
}

/// hot shot task
#[pin_project]
/// this is for `in_progress_fut`. The type is internal only so it's probably fine
/// to not type alias
#[allow(clippy::type_complexity)]
pub struct HST<HSTT: HotShotTaskTypes> {
    /// the in progress future
    /// TODO does `'static` make sense here
    in_progress_fut:
        Option<LocalBoxFuture<'static, (Option<HotShotTaskCompleted<HSTT>>, HSTT::State)>>,
    /// name of task
    name: String,
    /// state of the task
    #[pin]
    status: TaskState,
    /// function to shut down the task
    /// if we're tracking with a global registry
    shutdown_fn: Option<ShutdownFn>,
    /// event stream uid
    event_stream_uid: Option<StreamId>,
    /// shared stream
    #[pin]
    event_stream: Option<Fuse<<HSTT::EventStream as EventStream>::StreamType>>,
    /// stream of messages
    #[pin]
    message_stream: Option<Fuse<HSTT::MessageStream>>,
    /// state
    state: Option<HSTT::State>,
    /// handler for events
    handle_event: Option<HandleEvent<HSTT>>,
    /// handler for messages
    handle_message: Option<HandleMessage<HSTT>>,
    /// handler for filtering events (to use with stream)
    filter_event: Option<FilterEvent<HSTT::Event>>,
    /// task id
    pub(crate) tid: Option<HotShotTaskId>,
}

/// ADT for wrapping all possible handler types
#[allow(dead_code)]
pub(crate) enum HotShotTaskHandler<HSTT: HotShotTaskTypes> {
    /// handle an event
    HandleEvent(HandleEvent<HSTT>),
    /// handle a message
    HandleMessage(HandleMessage<HSTT>),
    /// filter an event
    FilterEvent(FilterEvent<HSTT::Event>),
    /// deregister with the registry
    Shutdown(ShutdownFn),
}

/// Type wrapper for handling an event
#[allow(clippy::type_complexity)]
pub struct HandleEvent<HSTT: HotShotTaskTypes>(
    Arc<
        dyn Fn(
            HSTT::Event,
            HSTT::State,
        )
            -> LocalBoxFuture<'static, (Option<HotShotTaskCompleted<HSTT>>, HSTT::State)>,
    >,
);
impl<HSTT: HotShotTaskTypes> Deref for HandleEvent<HSTT> {
    type Target =
        dyn Fn(
            HSTT::Event,
            HSTT::State,
        )
            -> LocalBoxFuture<'static, (Option<HotShotTaskCompleted<HSTT>>, HSTT::State)>;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

/// Type wrapper for handling a message
#[allow(clippy::type_complexity)]
pub struct HandleMessage<HSTT: HotShotTaskTypes>(
    Arc<
        dyn Fn(
            HSTT::Message,
            HSTT::State,
        )
            -> LocalBoxFuture<'static, (Option<HotShotTaskCompleted<HSTT>>, HSTT::State)>,
    >,
);
impl<HSTT: HotShotTaskTypes> Deref for HandleMessage<HSTT> {
    type Target =
        dyn Fn(
            HSTT::Message,
            HSTT::State,
        )
            -> LocalBoxFuture<'static, (Option<HotShotTaskCompleted<HSTT>>, HSTT::State)>;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

/// Return `true` if the event should be filtered
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

impl<HSTT: HotShotTaskTypes> HST<HSTT> {
    /// Do a consistency check on the `HST` construction
    pub(crate) fn base_check(&self) {
        assert!(
            self.shutdown_fn.is_some(),
            "Didn't register global registry"
        );
        assert!(
            self.in_progress_fut.is_none(),
            "This future has already been polled"
        );

        assert!(self.state.is_some(), "Didn't register state");

        assert!(self.tid.is_some(), "Didn't register global registry");
    }

    /// perform event sanity checks
    pub(crate) fn event_check(&self) {
        assert!(
            self.event_stream_uid.is_some(),
            "Didn't register event stream uid"
        );
        assert!(self.event_stream.is_some(), "Didn't register event stream");
        assert!(self.handle_event.is_some(), "Didn't register event handler");
        assert!(self.filter_event.is_some(), "Didn't register event filter");
    }

    /// perform message sanity checks
    pub(crate) fn message_check(&self) {
        assert!(
            self.handle_message.is_some(),
            "Didn't reegister message handler"
        );
        assert!(
            self.message_stream.is_some(),
            "Didn't register message stream"
        );
    }

    /// register a handler with the task
    #[must_use]
    pub(crate) fn register_handler(self, handler: HotShotTaskHandler<HSTT>) -> Self {
        match handler {
            HotShotTaskHandler::HandleEvent(handler) => Self {
                handle_event: Some(handler),
                ..self
            },
            HotShotTaskHandler::HandleMessage(handler) => Self {
                handle_message: Some(handler),
                ..self
            },
            HotShotTaskHandler::FilterEvent(_handler) => unimplemented!(),
            HotShotTaskHandler::Shutdown(handler) => Self {
                shutdown_fn: Some(handler),
                ..self
            },
        }
    }

    /// register an event stream with the task
    pub(crate) async fn register_event_stream(
        self,
        stream: HSTT::EventStream,
        filter: FilterEvent<HSTT::Event>,
    ) -> Self {
        // TODO perhaps GC the event stream
        // (unsunscribe)
        Self {
            event_stream: Some(stream.subscribe(filter).await.0.fuse()),
            ..self
        }
    }

    /// register a message with the task
    #[must_use]
    pub(crate) fn register_message_stream(self, stream: HSTT::MessageStream) -> Self {
        Self {
            message_stream: Some(stream.fuse()),
            ..self
        }
    }

    /// register state with the task
    #[must_use]
    pub(crate) fn register_state(self, state: HSTT::State) -> Self {
        Self {
            state: Some(state),
            ..self
        }
    }

    /// register with the registry
    pub(crate) async fn register_registry(self, registry: &mut GlobalRegistry) -> Self {
        let (shutdown_fn, id) = registry.register(&self.name, self.status.clone()).await;
        Self {
            shutdown_fn: Some(shutdown_fn),
            tid: Some(id),
            ..self
        }
    }

    /// create a new task
    pub(crate) fn new(name: String) -> Self {
        Self {
            name,
            status: TaskState::new(),
            event_stream: None,
            state: None,
            handle_event: None,
            handle_message: None,
            filter_event: None,
            shutdown_fn: None,
            message_stream: None,
            event_stream_uid: None,
            in_progress_fut: None,
            tid: None,
        }
    }

    /// launch the task
    pub async fn launch(self) -> HotShotTaskCompleted<HSTT> {
        self.await
    }
}

/// enum describing how the tasks completed
pub enum HotShotTaskCompleted<HSTT: HotShotTaskTypes> {
    /// the task shut down successfully
    ShutDown,
    /// the task encountered an error
    Error(HSTT::Error),
    /// the streams the task was listening for died
    StreamsDied,
    /// we somehow lost the state
    /// this is definitely a bug.
    LostState,
}

// NOTE: this is a Future, but it could easily be a stream.
// but these are semantically equivalent because instead of
// returning when paused, we just return `Poll::Pending`
impl<HSTT: HotShotTaskTypes> Future for HST<HSTT> {
    type Output = HotShotTaskCompleted<HSTT>;

    // NOTE: this is too many lines
    // but I'm not sure how to separate this out
    // into separate functions. `projected` and `self` are hard to
    // pass around
    #[allow(clippy::too_many_lines)]
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
                match state_change {
                    TaskStatus::NotStarted | TaskStatus::Paused => {
                        return Poll::Pending;
                    }
                    TaskStatus::Running => { /* do nothing if we are running */ }
                    TaskStatus::Completed => {
                        return Poll::Ready(HotShotTaskCompleted::ShutDown);
                    }
                }
            }
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
                    // if the future errored out, return it, we're done
                    if let Some(completed) = result {
                        return Poll::Ready(completed);
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
                                        if let Some(completed) = result {
                                            return Poll::Ready(completed);
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
                                        if let Some(completed) = result {
                                            return Poll::Ready(completed);
                                        }
                                    }
                                    Poll::Pending => {
                                        *projected.in_progress_fut = Some(fut);
                                        // TODO add in logic
                                        return Poll::Pending;
                                    }
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
