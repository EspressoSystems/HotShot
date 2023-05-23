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

    fn build(builder: TaskBuilder<Self>) -> HST<Self>
    where
        Self: Sized,
    {
        builder.0.base_check();
        builder.0.event_check();
        builder.0
    }
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

    fn build(builder: TaskBuilder<Self>) -> HST<Self>
    where
        Self: Sized,
    {
        builder.0.base_check();
        builder.0.message_check();
        builder.0
    }
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

    fn build(builder: TaskBuilder<Self>) -> HST<Self>
    where
        Self: Sized,
    {
        builder.0.base_check();
        builder.0.message_check();
        builder.0.event_check();
        builder.0
    }
}

#[cfg(test)]
pub mod test {
    use snafu::Snafu;

    use crate::event_stream;
    use crate::event_stream::ChannelStream;
    use crate::task::{PassType, HST, TS};

    use super::HSTWithEvent;
    use crate::task::HotShotTaskTypes;
    use crate::task_impls::TaskBuilder;

    #[derive(Snafu, Debug)]
    pub struct Error {}

    #[derive(Clone, Debug)]
    pub struct State {}

    impl TS for State {}
    impl PassType for State {}

    pub type AppliedHSTWithEvent = HSTWithEvent<Error, (), ChannelStream<()>, State>;

    #[cfg(test)]
    #[cfg_attr(
        feature = "tokio-executor",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(feature = "async-std-executor", async_std::test)]
    #[should_panic]
    async fn test_init_with_event_stream() {
        let task = HST::<AppliedHSTWithEvent>::new("Test Task".to_string());
        task.launch().await;
    }

    #[cfg(test)]
    #[cfg_attr(
        feature = "tokio-executor",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(feature = "async-std-executor", async_std::test)]
    async fn test_task_with_event_stream() {
        use crate::{
            global_registry::GlobalRegistry,
            task::{FilterEvent, HandleEvent},
        };

        let event_stream: event_stream::ChannelStream<()> = event_stream::ChannelStream::new();

        let state = State {};

        let mut registry = GlobalRegistry::spawn_new();

        let built_task = TaskBuilder::<AppliedHSTWithEvent>::new("Test Task".to_string())
            .register_event_stream(event_stream.clone(), FilterEvent::default())
            .await
            .register_registry(&mut registry)
            .await
            .register_state(state)
            .register_event_handler(HandleEvent::default());
        AppliedHSTWithEvent::build(built_task).launch().await;
    }
}
