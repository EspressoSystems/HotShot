use futures::Stream;
use std::marker::PhantomData;

use crate::{
    event_stream::{DummyStream, EventStream, SendableStream},
    global_registry::{GlobalRegistry, HotShotTaskId},
    task::{
        FilterEvent, HandleEvent, HandleMessage, HotShotTaskHandler, HotShotTaskTypes, PassType,
        TaskErr, HST, TS,
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

    // pub fn state(&self) -> Option<HSTT::State> {
    //     self.0.state()
    // }

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

impl<ERR: TaskErr, EVENT: PassType, ESTREAM: EventStream<EventType = EVENT>, STATE: TS>
    HotShotTaskTypes for HSTWithEvent<ERR, EVENT, ESTREAM, STATE>
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

impl<ERR: TaskErr, MSG: PassType, MSTREAM: SendableStream<Item = MSG>, STATE: TS> HotShotTaskTypes
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
        ERR: TaskErr,
        EVENT: PassType,
        ESTREAM: EventStream<EventType = EVENT>,
        MSG: PassType,
        MSTREAM: SendableStream<Item = MSG>,
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
    use async_compatibility_layer::channel::{unbounded, UnboundedStream};
    use snafu::Snafu;

    use crate::event_stream;
    use crate::event_stream::ChannelStream;
    use crate::task::{PassType, TaskErr, TS};

    use super::{HSTWithEvent, HSTWithEventAndMessage, HSTWithMessage};
    use crate::event_stream::EventStream;
    use crate::task::HotShotTaskTypes;
    use crate::task_impls::TaskBuilder;
    use async_compatibility_layer::art::async_spawn;
    use futures::FutureExt;
    use std::sync::Arc;

    use crate::{
        global_registry::GlobalRegistry,
        task::{FilterEvent, HandleEvent, HandleMessage, HotShotTaskCompleted},
    };
    use async_compatibility_layer::logging::setup_logging;

    #[derive(Snafu, Debug)]
    pub struct Error {}

    impl TaskErr for Error {}

    #[derive(Clone, Debug)]
    pub struct State {}

    #[derive(Clone, Debug)]
    pub enum Event {
        Finished,
        Dummy,
    }

    impl PassType for Event {}

    impl TS for State {}
    impl PassType for State {}

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Message {
        Finished,
        Dummy,
    }

    impl PassType for Message {}

    // TODO fill in generics for stream

    pub type AppliedHSTWithEvent = HSTWithEvent<Error, Event, ChannelStream<Event>, State>;
    pub type AppliedHSTWithMessage =
        HSTWithMessage<Error, Message, UnboundedStream<Message>, State>;
    pub type AppliedHSTWithEventMessage = HSTWithEventAndMessage<
        Error,
        Message,
        UnboundedStream<Message>,
        Event,
        ChannelStream<Event>,
        State,
    >;

    #[cfg(test)]
    #[cfg_attr(
        feature = "tokio-executor",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(feature = "async-std-executor", async_std::test)]
    #[should_panic]
    async fn test_init_with_event_stream() {
        setup_logging();
        let task = TaskBuilder::<AppliedHSTWithEvent>::new("Test Task".to_string());
        AppliedHSTWithEvent::build(task).launch().await;
    }

    // TODO this should be moved to async-compatibility-layer
    #[cfg(test)]
    #[cfg_attr(
        feature = "tokio-executor",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(feature = "async-std-executor", async_std::test)]
    async fn test_channel_stream() {
        use futures::StreamExt;
        let (s, r) = unbounded();
        let mut stream: UnboundedStream<Message> = r.into_stream();
        s.send(Message::Dummy).await.unwrap();
        s.send(Message::Finished).await.unwrap();
        assert!(stream.next().await.unwrap() == Message::Dummy);
        assert!(stream.next().await.unwrap() == Message::Finished);
    }

    #[cfg(test)]
    #[cfg_attr(
        feature = "tokio-executor",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(feature = "async-std-executor", async_std::test)]
    async fn test_task_with_event_stream() {
        setup_logging();
        let event_stream: event_stream::ChannelStream<Event> = event_stream::ChannelStream::new();

        let state = State {};

        let mut registry = GlobalRegistry::new();

        let event_handler = HandleEvent(Arc::new(move |event, state| {
            async move {
                if let Event::Finished = event {
                    (Some(HotShotTaskCompleted::ShutDown), state)
                } else {
                    (None, state)
                }
            }
            .boxed()
        }));

        let built_task = TaskBuilder::<AppliedHSTWithEvent>::new("Test Task".to_string())
            .register_event_stream(event_stream.clone(), FilterEvent::default())
            .await
            .register_registry(&mut registry)
            .await
            .register_state(state)
            .register_event_handler(event_handler);
        event_stream.publish(Event::Dummy).await;
        event_stream.publish(Event::Dummy).await;
        event_stream.publish(Event::Finished).await;
        AppliedHSTWithEvent::build(built_task).launch().await;
    }

    #[cfg(test)]
    #[cfg_attr(
        feature = "tokio-executor",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(feature = "async-std-executor", async_std::test)]
    async fn test_task_with_message_stream() {
        setup_logging();
        let state = State {};

        let mut registry = GlobalRegistry::new();

        let (s, r) = async_compatibility_layer::channel::unbounded();

        let message_handler = HandleMessage(Arc::new(move |message, state| {
            async move {
                if let Message::Finished = message {
                    (Some(HotShotTaskCompleted::ShutDown), state)
                } else {
                    (None, state)
                }
            }
            .boxed()
        }));

        let built_task = TaskBuilder::<AppliedHSTWithMessage>::new("Test Task".to_string())
            .register_message_handler(message_handler)
            .register_message_stream(r.into_stream())
            .register_registry(&mut registry)
            .await
            .register_state(state);
        async_spawn(async move {
            s.send(Message::Dummy).await.unwrap();
            s.send(Message::Finished).await.unwrap();
        });
        let result = AppliedHSTWithMessage::build(built_task).launch().await;
        assert!(result == HotShotTaskCompleted::ShutDown);
    }
}
