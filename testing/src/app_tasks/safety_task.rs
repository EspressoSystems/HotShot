// TODO rename this file to per-node

use std::{ops::Deref, sync::Arc};

use super::{
    node_ctx::{NodeCtx, ViewFailed, ViewStatus, ViewSuccess},
    test_launcher::TaskFuture,
    GlobalTestEvent,
};
use async_compatibility_layer::channel::UnboundedStream;
use futures::{future::BoxFuture, FutureExt};
use hotshot::traits::TestableNodeImplementation;
use hotshot_task::{
    event_stream::ChannelStream,
    global_registry::GlobalRegistry,
    task::{FilterEvent, HandleEvent, HandleMessage, HotShotTaskCompleted, TaskErr, TS},
    task_impls::{HSTWithEventAndMessage, TaskBuilder},
};
use hotshot_types::{
    event::{Event, EventType},
    traits::node_implementation::NodeType,
};
use snafu::Snafu;
use tracing::log::warn;

// TODO
/// Properties to describe the overall safety.
#[derive(Clone, Debug)]
pub struct OverallSafetyPropertiesDescription {}

/// Data Availability task error
#[derive(Snafu, Debug)]
pub enum SafetyTaskErr {
    // TODO make this more detailed
    /// There are too many failures.
    TooManyFailures,
    /// The number of decides isn't enough.
    NotEnoughDecides,
}
impl TaskErr for SafetyTaskErr {}

/// Data availability task state
#[derive(Debug)]
pub struct SafetyTask<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> {
    pub(crate) ctx: NodeCtx<TYPES, I>,
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> Default
    for SafetyTask<TYPES, I>
{
    fn default() -> Self {
        Self {
            ctx: Default::default(),
        }
    }
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> TS
    for SafetyTask<TYPES, I>
{
}

/// builder describing custom safety properties
#[derive(Clone)]
pub enum SafetyTaskDescription<
    TYPES: NodeType,
    I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>,
> {
    /// General properties.
    GenProperties(NodeSafetyPropertiesDescription),
    /// Customized propoerties.
    CustomProperties(SafetyFinisher<TYPES, I>),
}

/// properties used for gen
#[derive(Clone, Debug)]
pub struct NodeSafetyPropertiesDescription {
    /// number failed views
    pub num_failed_views: Option<usize>,
    /// number decide events
    pub num_decide_events: Option<usize>,
}

// basic consistency check for single node
/// Exists for easier overriding
/// runs at end of all tasks
#[derive(Clone)]
#[allow(clippy::type_complexity)]
pub struct SafetyFinisher<
    TYPES: NodeType,
    I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>,
>(
    pub  Arc<
        dyn for<'a> Fn(&'a mut NodeCtx<TYPES, I>) -> BoxFuture<'a, Result<(), SafetyTaskErr>>
            + Send
            + 'static
            + Sync,
    >,
);

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> Deref
    for SafetyFinisher<TYPES, I>
{
    type Target = dyn for<'a> Fn(&'a mut NodeCtx<TYPES, I>) -> BoxFuture<'a, Result<(), SafetyTaskErr>>
        + Send
        + 'static
        + Sync;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>
    SafetyTaskDescription<TYPES, I>
{
    fn gen_finisher(self) -> SafetyFinisher<TYPES, I> {
        match self {
            SafetyTaskDescription::CustomProperties(finisher) => finisher,
            SafetyTaskDescription::GenProperties(NodeSafetyPropertiesDescription {
                num_failed_views,
                num_decide_events,
            }) => SafetyFinisher(Arc::new(move |ctx: &mut NodeCtx<TYPES, I>| {
                async move {
                    let mut num_failed = 0;
                    let mut num_decided = 0;
                    for view_status in ctx.round_results.values() {
                        match view_status {
                            ViewStatus::InProgress(_) => {}
                            ViewStatus::ViewFailed(_) => {
                                num_failed += 1;
                            }
                            ViewStatus::ViewSuccess(_) => {
                                num_decided += 1;
                            }
                        }
                    }
                    if let Some(num_failed_views) = num_failed_views {
                        if num_failed >= num_failed_views {
                            return Err(SafetyTaskErr::TooManyFailures);
                        }
                    }

                    if let Some(num_decide_events) = num_decide_events {
                        if num_decided < num_decide_events {
                            return Err(SafetyTaskErr::NotEnoughDecides);
                        }
                    }
                    Ok(())
                }
                .boxed()
            })),
        }
    }

    /// build
    pub fn build(
        self,
        // registry: &mut GlobalRegistry,
        // test_event_stream: ChannelStream<GlobalTestEvent>,
        // hotshot_event_stream: UnboundedStream<Event<TYPES, I::Leaf>>,
    ) -> Box<
        dyn Fn(
            SafetyTask<TYPES, I>,
            GlobalRegistry,
            ChannelStream<GlobalTestEvent>,
            UnboundedStream<Event<TYPES, I::Leaf>>,
        ) -> TaskFuture,
    > {
        Box::new(
            move |state, mut registry, test_event_stream, hotshot_event_stream| {
                // TODO this is cursed, there's definitely a better way to do this
                let desc = self.clone();
                async move {
                    let test_event_handler = HandleEvent::<SafetyTaskTypes<TYPES, I>>(Arc::new(
                        move |event, mut state| {
                            let finisher = desc.clone().gen_finisher();
                            async move {
                                match event {
                                    GlobalTestEvent::ShutDown => {
                                        let finished = finisher(&mut state.ctx).await;
                                        let result = match finished {
                                            Ok(()) => HotShotTaskCompleted::ShutDown,
                                            Err(err) => HotShotTaskCompleted::Error(Box::new(err)),
                                        };
                                        (Some(result), state)
                                        // return (Some(HotShotTaskCompleted, finisher(state.ctx).await)
                                        // TODO run lambda on gathered state
                                        // return (Some(HotShotTaskCompleted::ShutDown), state);
                                    }
                                }
                            }
                            .boxed()
                        },
                    ));
                    let message_handler = HandleMessage::<SafetyTaskTypes<TYPES, I>>(Arc::new(
                        move |msg, mut state| {
                            async move {
                                let Event { view_number, event } = msg;
                                match event {
                                    EventType::Error { error } => {
                                        // TODO better warn with node idx
                                        warn!("View {:?} failed for a replica", view_number);
                                        state.ctx.round_results.insert(
                                            view_number,
                                            ViewStatus::ViewFailed(ViewFailed(error)),
                                        );
                                    }
                                    EventType::Decide { .. } => {
                                        // for leaf in leaf_chain {
                                        // TODO how to test this
                                        // }
                                        // TODO how to do this counting
                                        state.ctx.round_results.insert(
                                            view_number,
                                            ViewStatus::ViewSuccess(ViewSuccess {
                                                txns: Vec::new(),
                                                agreed_state: None,
                                                agreed_block: None,
                                                agreed_leaf: None,
                                            }),
                                        );
                                    }
                                    // these aren't failures
                                    EventType::ReplicaViewTimeout { view_number: _ }
                                    | EventType::NextLeaderViewTimeout { view_number: _ }
                                    | EventType::ViewFinished { view_number: _ } => {}
                                    _ => todo!(),
                                }
                                (None, state)
                            }
                            .boxed()
                        },
                    ));

                    let builder = TaskBuilder::<SafetyTaskTypes<TYPES, I>>::new(
                        "Safety Check Task".to_string(),
                    )
                    .register_event_stream(test_event_stream, FilterEvent::default())
                    .await
                    .register_registry(&mut registry)
                    .await
                    .register_state(state)
                    .register_event_handler(test_event_handler)
                    .register_message_handler(message_handler)
                    .register_message_stream(hotshot_event_stream);
                    let task_id = builder.get_task_id().unwrap();
                    (task_id, SafetyTaskTypes::build(builder).launch())
                }
                .boxed()
            },
        )
    }
}

/// Data Availability task types
pub type SafetyTaskTypes<TYPES, I> = HSTWithEventAndMessage<
    SafetyTaskErr,
    GlobalTestEvent,
    ChannelStream<GlobalTestEvent>,
    Event<TYPES, I::Leaf>,
    UnboundedStream<Event<TYPES, I::Leaf>>,
    SafetyTask<TYPES, I>,
>;
