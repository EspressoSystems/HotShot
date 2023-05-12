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
    sync::{atomic::AtomicUsize, Arc},
};

use async_compatibility_layer::channel::{UnboundedReceiver, UnboundedSender, UnboundedStream};
use async_trait::async_trait;
use futures::Stream;
use nll::nll_todo::nll_todo;

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
    type StreamType = UnboundedStream<Item = Self::EventType>;

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

// //Example EventStream impl
struct HotShotTask<EVENT: Event, STATE, STREAM: EventStream<EventType = EVENT>> {
    // TODO remove
    _pd: PhantomData<(EVENT, STREAM, STATE)>,
    /// registry
    shared_registry: GlobalRegistry,
    /// internal event stream
    shared_stream: STREAM,
    /// state
    state: STATE,
    /// handler for events
    handle_event: HandleEvent<EVENT, STATE>,
    /// handler for messages
    handle_message: HandleMessage<STATE>,
    /// handler for filtering events (to use with stream)
    filter_event: FilterEvent<EVENT>,
}

pub enum HotShotTaskHandler<STATE, EVENT> {
    HandleEvent(HandleEvent<STATE, EVENT>),
    HandleMessage(HandleMessage<STATE>),
    FilterEvent(FilterEvent<EVENT>),
}

///
pub struct HandleEvent<EVENT, STATE>(Box<dyn Fn(&mut STATE, EVENT) -> bool>);

/// TODO hardcode? or generic?
pub struct Message;

pub struct HandleMessage<STATE>(Box<dyn Fn(&mut STATE, Message) -> bool>);

pub struct FilterEvent<EVENT>(Box<dyn Fn(EVENT) -> bool>);

impl<EVENT: Event, STATE, STREAM: EventStream<EventType = EVENT>>
    HotShotTask<EVENT, STATE, STREAM>
{
    /// register a handler with the task
    pub fn register_handler(&mut self, handler: HotShotTaskHandler<EVENT, STATE>) {
        match handler {
            HotShotTaskHandler::HandleEvent(handler) => self.handle_event = handler,
            HotShotTaskHandler::HandleMessage(handler) => self.handle_message = handler,
            HotShotTaskHandler::FilterEvent(handler) => self.filter_event = handler,
        }
    }

    /// create a new task
    pub fn new(shared_registry: GlobalRegistry, shared_stream: STREAM, state: STATE) -> Self {
        Self {
            _pd: PhantomData,
            shared_registry,
            shared_stream,
            state,
            handle_event: nll_todo(),
            handle_message: nll_todo(),
            filter_event: nll_todo(),
        }
    }

    /// fire and forget the task.
    pub fn run(mut self) {
        let running: bool = true;
        loop {}
    }
}

pub struct DeregisterFn(Box<dyn Fn()>);

// TODO should we also track the handle? Or is this only for book keeping?
// With a gc-ed bitvec, we'll be able to tell which tasks have been shut down
// threadsafe
#[derive(Clone, Debug)]
pub struct GlobalRegistry {
    /// each bit represents a task
    /// if 0, then task is killed
    /// if 1, then task is alive
    /// for the first iteration, we can do sth similar to a bump allocator
    /// TODO there's definitely a cleverer way to do this.
    /// reminds me of memory allocation.
    task_status: Arc<Vec<AtomicUsize>>,
    /// number of deleted blocks
    deleted_block_num: Arc<AtomicUsize>,
    /// bit index of next task
    next_task_idx: Arc<AtomicUsize>,
}

impl GlobalRegistry {
    /// create new registry
    pub fn new() -> Self {
        Self {
            task_status: Arc::new(vec![]),
            deleted_block_num: Arc::new(AtomicUsize::new(0)),
            next_task_idx: Arc::new(AtomicUsize::new(0)),
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

pub mod test {
    // TODO test global registry using either global + lazy_static
    // or passing around global registry
}
