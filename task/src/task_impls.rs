use std::marker::PhantomData;

use futures::Stream;

use crate::{
    event_stream::{DummyStream, EventStream},
    global_registry::{GlobalRegistry, HotShotTaskId},
    task::{
        FilterEvent, HandleEvent, HandleMessage, HotShotTaskHandler, HotShotTaskTypes, PassType,
        HST, TS,
    },
};

/// trait to specify features
pub trait ImplMessageStream {}
/// trait to specify features
pub trait ImplEventStream {}

/// builder for task
pub struct TaskBuilder<HSTT: HotShotTaskTypes>(HST<HSTT>);

impl<HSTT: HotShotTaskTypes> TaskBuilder<HSTT> {
    /// register an event handler
    #[must_use]
    pub fn register_event_handler(self, handler: HandleEvent<HSTT>) -> Self
    where
        HSTT: ImplEventStream,
    {
        Self(
            self.0
                .register_handler(HotShotTaskHandler::HandleEvent(handler)),
        )
    }

    /// register a message handler
    #[must_use]
    pub fn register_message_handler(self, handler: HandleMessage<HSTT>) -> Self
    where
        HSTT: ImplMessageStream,
    {
        Self(
            self.0
                .register_handler(HotShotTaskHandler::HandleMessage(handler)),
        )
    }

    /// register a message stream
    #[must_use]
    pub fn register_message_stream(self, stream: HSTT::MessageStream) -> Self
    where
        HSTT: ImplMessageStream,
    {
        Self(self.0.register_message_stream(stream))
    }

    /// register a event filter
    #[must_use]
    pub fn register_event_filter(self, handler: FilterEvent<HSTT::Event>) -> Self
    where
        HSTT: ImplEventStream,
    {
        Self(
            self.0
                .register_handler(HotShotTaskHandler::FilterEvent(handler)),
        )
    }

    /// register an event stream
    pub async fn register_event_stream(
        self,
        stream: HSTT::EventStream,
        filter: FilterEvent<HSTT::Event>,
    ) -> Self
    where
        HSTT: ImplEventStream,
    {
        Self(self.0.register_event_stream(stream, filter).await)
    }

    /// register the state
    #[must_use]
    pub fn register_state(self, state: HSTT::State) -> Self {
        Self(self.0.register_state(state))
    }

    /// register with the global registry
    pub async fn register_registry(self, registry: &mut GlobalRegistry) -> Self {
        Self(self.0.register_registry(registry).await)
    }

    /// get the task id in the global registry
    pub fn get_task_id(&self) -> Option<HotShotTaskId> {
        self.0.tid
    }

    /// build the task
    pub fn build(self) -> HST<HSTT> {
        self.0
    }

    /// create a new task builder
    #[must_use]
    pub fn new(name: String) -> Self {
        Self(HST::new(name))
    }
}

/// a hotshot task with an event stream
pub struct HSTWithEvent<
    ERR: std::error::Error,
    EVENT: PassType,
    ESTREAM: EventStream<EventType = EVENT>,
    STATE: TS,
> {
    /// phantom data
    _pd: PhantomData<(ERR, EVENT, ESTREAM, STATE)>,
}

impl<
        ERR: std::error::Error,
        EVENT: PassType,
        ESTREAM: EventStream<EventType = EVENT>,
        STATE: TS,
    > ImplEventStream for HSTWithEvent<ERR, EVENT, ESTREAM, STATE>
{
}

impl<ERR: std::error::Error, MSG: PassType, MSTREAM: Stream<Item = MSG>, STATE: TS>
    ImplMessageStream for HSTWithMessage<ERR, MSG, MSTREAM, STATE>
{
}

impl<
        ERR: std::error::Error,
        EVENT: PassType,
        ESTREAM: EventStream<EventType = EVENT>,
        STATE: TS,
    > HotShotTaskTypes for HSTWithEvent<ERR, EVENT, ESTREAM, STATE>
{
    type Event = EVENT;
    type State = STATE;
    type EventStream = ESTREAM;
    type Message = ();
    type MessageStream = DummyStream;
    type Error = ERR;
}

/// a hotshot task with a message
pub struct HSTWithMessage<
    ERR: std::error::Error,
    MSG: PassType,
    MSTREAM: Stream<Item = MSG>,
    STATE: TS,
> {
    /// phantom data
    _pd: PhantomData<(ERR, MSG, MSTREAM, STATE)>,
}

impl<ERR: std::error::Error, MSG: PassType, MSTREAM: Stream<Item = MSG>, STATE: TS> HotShotTaskTypes
    for HSTWithMessage<ERR, MSG, MSTREAM, STATE>
{
    type Event = ();
    type State = STATE;
    type EventStream = DummyStream;
    type Message = MSG;
    type MessageStream = MSTREAM;
    type Error = ERR;
}

/// hotshot task with even and message
pub struct HSTWithEventAndMessage<
    ERR: std::error::Error,
    EVENT: PassType,
    ESTREAM: EventStream<EventType = EVENT>,
    MSG: PassType,
    MSTREAM: Stream<Item = MSG>,
    STATE: TS,
> {
    /// phantom data
    _pd: PhantomData<(ERR, EVENT, ESTREAM, MSG, MSTREAM, STATE)>,
}

impl<
        ERR: std::error::Error,
        EVENT: PassType,
        ESTREAM: EventStream<EventType = EVENT>,
        MSG: PassType,
        MSTREAM: Stream<Item = MSG>,
        STATE: TS,
    > ImplEventStream for HSTWithEventAndMessage<ERR, EVENT, ESTREAM, MSG, MSTREAM, STATE>
{
}

impl<
        ERR: std::error::Error,
        EVENT: PassType,
        ESTREAM: EventStream<EventType = EVENT>,
        MSG: PassType,
        MSTREAM: Stream<Item = MSG>,
        STATE: TS,
    > ImplMessageStream for HSTWithEventAndMessage<ERR, EVENT, ESTREAM, MSG, MSTREAM, STATE>
{
}

impl<
        ERR: std::error::Error,
        EVENT: PassType,
        ESTREAM: EventStream<EventType = EVENT>,
        MSG: PassType,
        MSTREAM: Stream<Item = MSG>,
        STATE: TS,
    > HotShotTaskTypes for HSTWithEventAndMessage<ERR, EVENT, ESTREAM, MSG, MSTREAM, STATE>
{
    type Event = ();
    type State = STATE;
    type EventStream = DummyStream;
    type Message = MSG;
    type MessageStream = MSTREAM;
    type Error = ERR;
}
