use std::fmt::Formatter;
use std::ops::Deref;
use std::pin::Pin;
use std::task::{Poll, Context};

use either::Either::{self, Right};
use futures::{future::BoxFuture, stream::Fuse, Stream};
use futures::{Future, FutureExt, StreamExt};
use nll::nll_todo::nll_todo;
use pin_project::pin_project;
use std::sync::Arc;

use crate::event_stream::SendableStream;
use crate::global_registry::{GlobalRegistry, HotShotTaskId};
use crate::task_impls::TaskBuilder;
use crate::task_state::TaskStatus;
use crate::{event_stream::EventStream, global_registry::ShutdownFn, task_state::TaskState};

/// restrictions on types we wish to pass around.
/// Includes messages and events
pub trait PassType: Clone + std::fmt::Debug + Sync + Send + 'static {}
impl PassType for () {}

/// the task state
pub trait TS: std::fmt::Debug + Sync + Send + 'static {}

/// a task error that has nice qualities
#[allow(clippy::module_name_repetitions)]
pub trait TaskErr: std::error::Error + Sync + Send + 'static {}

/// group of types needed for a hotshot task
pub trait HotShotTaskTypes: 'static {
    /// the event type from the event stream
    type Event: PassType;
    /// the state of the task
    type State: TS;
    /// the global event stream
    type EventStream: EventStream<EventType = Self::Event>;
    /// the message stream to receive
    type Message: PassType;
    /// the steam of messages from other tasks
    type MessageStream: SendableStream<Item = Self::Message>;
    /// the error to return
    type Error: TaskErr + 'static + ?Sized;

    /// build a task
    /// NOTE: done here and not on `TaskBuilder` because
    /// we want specific checks done on each variant
    /// NOTE: all generics implement `Sized`, but this bound is
    /// NOT applied to `Self` unless we specify
    fn build(builder: TaskBuilder<Self>) -> HST<Self>
    where
        Self: Sized;
}

/// hot shot task
#[pin_project(project = ProjectedHST)]
#[allow(clippy::type_complexity)]
pub struct HST<HSTT: HotShotTaskTypes> {
    /// the eventual return value, post-cleanup
    r_val: Option<HotShotTaskCompleted>,
    /// if we have a future for tracking shutdown progress
    in_progress_shutdown_fut: Option<BoxFuture<'static, ()>>,
    /// the in progress future
    in_progress_fut: Option<BoxFuture<'static, (Option<HotShotTaskCompleted>, HSTT::State)>>,
    /// name of task
    name: String,
    /// state of the task
    /// TODO make this boxed. We don't want to assume this is a small future.
    /// since it concievably may be stored on the stack
    #[pin]
    status: TaskState,
    /// functions performing cleanup
    /// one should shut down the task
    /// if we're tracking with a global registry
    /// the other should unsubscribe from the stream
    shutdown_fns: Vec<ShutdownFn>,
    /// shared stream
    event_stream: MaybePinnedEventStream<HSTT>,
    /// stream of messages
    message_stream: Option<Pin<Box<Fuse<HSTT::MessageStream>>>>,
    /// state
    state: Option<HSTT::State>,
    /// handler for events
    handle_event: Option<HandleEvent<HSTT>>,
    /// handler for messages
    handle_message: Option<HandleMessage<HSTT>>,
    /// task id
    pub(crate) tid: Option<HotShotTaskId>,
}

/// an option of a pinned boxed fused event stream
pub type MaybePinnedEventStream<HSTT> = Option<Pin<Box<Fuse<<<HSTT as HotShotTaskTypes>::EventStream as EventStream>::StreamType>>>>;

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
    pub  Arc<
        dyn Fn(
                HSTT::Event,
                HSTT::State,
            ) -> BoxFuture<'static, (Option<HotShotTaskCompleted>, HSTT::State)>
            + Sync
            + Send,
    >,
);

impl<HSTT: HotShotTaskTypes> Default for HandleEvent<HSTT> {
    fn default() -> Self {
        Self(Arc::new(|_event, state| async { (None, state) }.boxed()))
    }
}

impl<HSTT: HotShotTaskTypes> Deref for HandleEvent<HSTT> {
    type Target = dyn Fn(
        HSTT::Event,
        HSTT::State,
    ) -> BoxFuture<'static, (Option<HotShotTaskCompleted>, HSTT::State)>;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

/// Type wrapper for handling a message
#[allow(clippy::type_complexity)]
pub struct HandleMessage<HSTT: HotShotTaskTypes>(
    pub  Arc<
        dyn Fn(
                HSTT::Message,
                HSTT::State,
            ) -> BoxFuture<'static, (Option<HotShotTaskCompleted>, HSTT::State)>
            + Sync
            + Send,
    >,
);
impl<HSTT: HotShotTaskTypes> Deref for HandleMessage<HSTT> {
    type Target = dyn Fn(
        HSTT::Message,
        HSTT::State,
    ) -> BoxFuture<'static, (Option<HotShotTaskCompleted>, HSTT::State)>;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

/// Return `true` if the event should be filtered
#[derive(Clone)]
pub struct FilterEvent<EVENT: PassType>(pub Arc<dyn Fn(&EVENT) -> bool + Send + 'static + Sync>);

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
        assert!(!self.shutdown_fns.is_empty(), "No shutdown functions");
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
            self.shutdown_fns.len() == 2,
            "Expected 2 shutdown functions"
        );
        assert!(self.event_stream.is_some(), "Didn't register event stream");
        assert!(self.handle_event.is_some(), "Didn't register event handler");
    }

    /// perform message sanity checks
    pub(crate) fn message_check(&self) {
        assert!(
            self.handle_message.is_some(),
            "Didn't register message handler"
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
            HotShotTaskHandler::Shutdown(_handler) => unimplemented!(),
        }
    }

    /// register an event stream with the task
    pub(crate) async fn register_event_stream(
        self,
        event_stream: HSTT::EventStream,
        filter: FilterEvent<HSTT::Event>,
    ) -> Self {
        let (stream, uid) = event_stream.subscribe(filter).await;

        let mut shutdown_fns = self.shutdown_fns;
        {
            let event_stream = event_stream.clone();
            shutdown_fns.push(ShutdownFn(Arc::new(move || -> BoxFuture<'static, ()> {
                let event_stream = event_stream.clone();
                async move {
                    event_stream.clone().unsubscribe(uid).await;
                }
                .boxed()
            })));
        }
        // TODO perhaps GC the event stream
        // (unsunscribe)
        Self {
            event_stream: Some(Box::pin(stream.fuse())),
            shutdown_fns,
            ..self
        }
    }

    /// register a message with the task
    #[must_use]
    pub(crate) fn register_message_stream(self, stream: HSTT::MessageStream) -> Self {
        Self {
            message_stream: Some(Box::pin(stream.fuse())),
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
        let mut shutdown_fns = self.shutdown_fns;
        shutdown_fns.push(shutdown_fn);
        Self {
            shutdown_fns,
            tid: Some(id),
            ..self
        }
    }

    /// create a new task
    pub(crate) fn new(name: String) -> Self {
        Self {
            r_val: None,
            name,
            status: TaskState::new(),
            event_stream: None,
            state: None,
            handle_event: None,
            handle_message: None,
            shutdown_fns: vec![],
            message_stream: None,
            in_progress_fut: None,
            in_progress_shutdown_fut: None,
            tid: None,
        }
    }

    /// launch the task
    /// NOTE: the only way to get a `HST` is by usage
    /// of one of the impls. Those all have checks enabled.
    /// So, it should be safe to lanuch.
    pub fn launch(self) -> BoxFuture<'static, HotShotTaskCompleted> {
        async move { self.await }.boxed()
    }
}

/// enum describing how the tasks completed
pub enum HotShotTaskCompleted {
    /// the task shut down successfully
    ShutDown,
    /// the task encountered an error
    Error(Box<dyn TaskErr>),
    /// the streams the task was listening for died
    StreamsDied,
    /// we somehow lost the state
    /// this is definitely a bug.
    LostState,
    /// lost the return value somehow
    LostReturnValue,
}

impl std::fmt::Debug for HotShotTaskCompleted {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            HotShotTaskCompleted::ShutDown => f.write_str("HotShotTaskCompleted::ShutDown"),
            HotShotTaskCompleted::Error(_) => f.write_str("HotShotTaskCompleted::Error"),
            HotShotTaskCompleted::StreamsDied => f.write_str("HotShotTaskCompleted::StreamsDied"),
            HotShotTaskCompleted::LostState => f.write_str("HotShotTaskCompleted::LostState"),
            HotShotTaskCompleted::LostReturnValue => f.write_str("HotShotTaskCompleted::LostReturnValue"),
        }
    }
}

impl PartialEq for HotShotTaskCompleted {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Error(_l0), Self::Error(_r0)) => false,
            _ => core::mem::discriminant(self) == core::mem::discriminant(other),
        }
    }
}

impl<'pin, HSTT: HotShotTaskTypes> ProjectedHST<'pin, HSTT> {
    /// launches the shutdown future
    fn launch_shutdown_fut(&mut self, cx: &mut Context<'_>) -> Poll<HotShotTaskCompleted> {
        let fut = self.create_shutdown_fut();
        self.check_ip_shutdown_fut(fut, cx)
    }

    /// checks the in progress shutdown future, `fut`
    fn check_ip_shutdown_fut(&mut self, mut fut: Pin<Box<dyn Future<Output = ()> + Send>>, cx: &mut Context<'_>) -> Poll<HotShotTaskCompleted>{
        match fut.as_mut().poll(cx) {
            Poll::Ready(_) => {
                Poll::Ready(
                    self
                    .r_val
                    .take()
                    .unwrap_or_else(|| HotShotTaskCompleted::LostReturnValue),
                    )
            }
            Poll::Pending => {
                *self.in_progress_shutdown_fut = Some(fut);
                Poll::Pending
            }
        }

    }

    /// creates the shutdown future and returns it
    fn create_shutdown_fut(&mut self) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        let shutdown_fns = self.shutdown_fns.clone();
        let fut = async move {
            for shutdown_fn in shutdown_fns {
                shutdown_fn().await;
            }
        }
        .boxed();
        fut
    }

    /// returns event and message stream to their proper places inside the projection
    fn cleanup(self,
               event_stream: MaybePinnedEventStream<HSTT>,
               message_stream: Option<Pin<Box<Fuse<<HSTT>::MessageStream>>>>
               ) {
        *self.event_stream = event_stream;
        *self.message_stream = message_stream;
    }

    /// check the event stream
    /// returns either a poll if there's a future IP
    /// or a bool stating whether or not the stream is finished
    fn check_event_stream(&mut self, event_stream: MaybePinnedEventStream<HSTT>, cx: &mut Context<'_>) -> (Either<Poll<HotShotTaskCompleted>, bool>, MaybePinnedEventStream<HSTT>){
        if let Some(mut inner_event_stream) = event_stream {
            while let Poll::Ready(maybe_event) = inner_event_stream.as_mut().poll_next(cx) {
                if let Some(event) = maybe_event {
                } else {

                }

            }

        } else {
            return (Right(true), event_stream);
        }

        nll_todo()
    }

    /// check the message stream
    /// returns either a poll if there's a future IP
    /// or a bool stating whether or not the stream is finished
    fn check_message(&mut self) -> Either<Poll<HotShotTaskCompleted>, bool> {
        nll_todo()
    }

}

// NOTE: this is a Future, but it could easily be a stream.
// but these are semantically equivalent because instead of
// returning when paused, we just return `Poll::Pending`
impl<HSTT: HotShotTaskTypes> Future for HST<HSTT> {
    type Output = HotShotTaskCompleted;

    // NOTE: this is too many lines
    // with a lot of repeated code
    // but I'm not sure how to separate this out
    // into separate functions. `projected` and `self` are hard to
    // pass around
    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        tracing::info!("HotShot Task {:?} awakened", self.name);
        let mut projected = self.as_mut().project();


        if let Some(fut) = projected.in_progress_shutdown_fut.take() {
            return projected.check_ip_shutdown_fut(fut, cx);
        }

        // check if task is complete
        match projected.status.as_mut().poll_next(cx) {
            Poll::Ready(Some(state_change)) => match state_change {
                TaskStatus::NotStarted | TaskStatus::Paused => {
                    return Poll::Pending;
                }
                TaskStatus::Running => {}
                TaskStatus::Completed => {
                    *projected.r_val = Some(HotShotTaskCompleted::ShutDown);
                    return projected.launch_shutdown_fut(cx);
                }
            },
            // this primitive's stream will never end
            Poll::Ready(None) => {
                unreachable!()
            }
            // if there's nothing, that's fine
            Poll::Pending => {}
        }

        if let Some(in_progress_fut) = projected.in_progress_fut {
            match in_progress_fut.as_mut().poll(cx) {
                Poll::Ready((result, state)) => {
                    *projected.in_progress_fut = None;
                    *projected.state = Some(state);
                    // if the future errored out, return it, we're done
                    if let Some(completed) = result {
                        *projected.r_val = Some(completed);
                        return projected.launch_shutdown_fut(cx);
                    }
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }

        let mut event_stream = projected.event_stream.take();

        // do a thing


        let mut message_stream = projected.message_stream.take();

        // do a thing

        let mut event_stream_finished = false;
        let mut message_stream_finished = false;

        if let Some(mut inner_event_stream) = event_stream {
            while let Poll::Ready(maybe_event) = inner_event_stream.as_mut().poll_next(cx) {
                if let Some(event) = maybe_event {
                    if let Some(ref handle_event) = projected.handle_event {
                        tracing::error!("GOT AN EVENT");
                        let maybe_state = projected.state.take();
                        if let Some(state) = maybe_state {
                            let mut fut = handle_event(event, state);
                            match fut.as_mut().poll(cx) {
                                Poll::Ready((result, state)) => {
                                    *projected.in_progress_fut = None;
                                    *projected.state = Some(state);
                                    if let Some(completed) = result {
                                        *projected.r_val = Some(completed);
                                        let result = projected.launch_shutdown_fut(cx);
                                        projected.cleanup(Some(inner_event_stream), message_stream);
                                        return result;
                                    }
                                }
                                Poll::Pending => {
                                    *projected.in_progress_fut = Some(fut);
                                    projected.cleanup(Some(inner_event_stream), message_stream);
                                    return Poll::Pending;
                                }
                            }
                        } else {
                            *projected.r_val = Some(HotShotTaskCompleted::LostState);
                            let result = projected.launch_shutdown_fut(cx);
                            projected.cleanup(Some(inner_event_stream), message_stream);
                            return result;
                        }
                    }
                }
                else {
                    tracing::error!("Did NOT get an event");
                    // this is a fused future so `None` will come every time after the stream
                    // finishes
                    event_stream_finished = true;
                    break;
                }
            }
            // passing this stuff around is a trick and a half
            event_stream = Some(inner_event_stream);
        } else {
            event_stream_finished = true;
        }

        if let Some(mut inner_message_stream) = message_stream {
            while let Poll::Ready(maybe_msg) = inner_message_stream.as_mut().poll_next(cx) {
                if let Some(msg) = maybe_msg {
                    if let Some(ref handle_msg) = projected.handle_message {
                        let maybe_state = projected.state.take();
                        if let Some(state) = maybe_state {
                            let mut fut = handle_msg(msg, state);
                            match fut.as_mut().poll(cx) {
                                Poll::Ready((result, state)) => {
                                    *projected.in_progress_fut = None;
                                    *projected.state = Some(state);
                                    if let Some(completed) = result {
                                        *projected.r_val = Some(completed);
                                        let result = projected.launch_shutdown_fut(cx);
                                        projected.cleanup(event_stream, Some(inner_message_stream));
                                        return result;
                                    }
                                }
                                Poll::Pending => {
                                    *projected.in_progress_fut = Some(fut);
                                    projected.cleanup(event_stream, Some(inner_message_stream));
                                    return Poll::Pending;
                                }
                            };
                        } else {
                            *projected.r_val = Some(HotShotTaskCompleted::LostState);
                            let result = projected.launch_shutdown_fut(cx);
                            projected.cleanup(event_stream, Some(inner_message_stream));
                            return result;
                        }
                    }
                }
                // this is a fused future so `None` will come every time after the stream
                // finishes
                else {
                    message_stream_finished = true;
                    break;
                }
            }
            message_stream = Some(inner_message_stream);
        } else {
            message_stream_finished = true;
        }
        if message_stream_finished && event_stream_finished {
            tracing::error!("Message and event stream both finished!");
            *projected.r_val = Some(HotShotTaskCompleted::StreamsDied);
            let result = projected.launch_shutdown_fut(cx);
            projected.cleanup(event_stream, message_stream);
            return result;
        }

        projected.cleanup(event_stream, message_stream);

        tracing::error!("PENDING AT THE END");
        Poll::Pending
    }
}
