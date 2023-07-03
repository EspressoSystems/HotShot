
use std::{sync::Arc, ops::Deref};

use async_compatibility_layer::channel::UnboundedStream;
use either::Either;
use futures::{future::{BoxFuture, LocalBoxFuture}, FutureExt};
use hotshot::{tasks::DATaskState, traits::TestableNodeImplementation};
use hotshot_types::{traits::node_implementation::NodeType, event::{Event, EventType}};
use nll::nll_todo::nll_todo;
use snafu::Snafu;
use hotshot_task::{task::{TaskErr, TS, HST, HotShotTaskCompleted, HandleEvent, HandleMessage, FilterEvent, HotShotTaskTypes}, task_impls::{HSTWithEvent, HSTWithEventAndMessage, TaskBuilder}, event_stream::ChannelStream, global_registry::{HotShotTaskId, GlobalRegistry}};
use tracing::log::warn;

use crate::test_errors::ConsensusTestError;

use super::{GlobalTestEvent, node_ctx::{NodeCtx, ViewStatus, ViewFailed, ViewSuccess}};

/// Data Availability task error
#[derive(Snafu, Debug)]
pub enum SafetyTaskErr {
    // TODO make this more detailed
    TooManyFailures,
    NotEnoughDecides
}
impl TaskErr for SafetyTaskErr {}

/// Data availability task state
#[derive(Debug)]
pub struct SafetyTask<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> {
    ctx: NodeCtx<TYPES, I>
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> Default for SafetyTask<TYPES, I> {
    fn default() -> Self {
        Self { ctx: Default::default() }
    }
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> TS for SafetyTask<TYPES, I> {}

/// builder describing custom safety properties
#[derive(Clone)]
pub enum SafetyTaskBuilder<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> {
    GenProperties{
        /// number failed views
        num_failed_views: Option<usize>,
        /// number decide events
        num_decide_events: Option<usize>,
    },
    CustomProperties(SafetyFinisher<TYPES, I>)
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
        dyn for<'a> Fn(
            &'a mut NodeCtx<TYPES, I>,
        ) -> BoxFuture<'a, Result<(), SafetyTaskErr>> + Send + 'static + Sync,
    >,
);


impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> Deref for SafetyFinisher<TYPES, I> {
    type Target =
        dyn for<'a> Fn(
            &'a mut NodeCtx<TYPES, I>,
        ) -> BoxFuture<'a, Result<(), SafetyTaskErr>> + Send + 'static + Sync;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}




impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> SafetyTaskBuilder<TYPES, I> {
    fn gen_finisher(self) -> SafetyFinisher<TYPES, I>{
        match self {
            SafetyTaskBuilder::CustomProperties(finisher) => {
                finisher
            },
            SafetyTaskBuilder::GenProperties { num_failed_views, num_decide_events } => {
                SafetyFinisher(Arc::new( move |ctx: &mut NodeCtx<TYPES, I>| {
                        async move {
                            let mut num_failed = 0;
                            let mut num_decided = 0;
                            for (_view_num, view_status) in &ctx.round_results {
                                match view_status {
                                    ViewStatus::InProgress(_) => {

                                    },
                                    ViewStatus::ViewFailed(_) => {
                                        num_failed += 1;
                                    },
                                    ViewStatus::ViewSuccess(_) => {
                                        num_decided += 1;
                                    },
                                }
                            }
                            if let Some(num_failed_views) = num_failed_views {
                                if num_failed >= num_failed_views {
                                    return Err(SafetyTaskErr::TooManyFailures)
                                }
                            }

                            if let Some(num_decide_events) = num_decide_events {
                                if num_decided < num_decide_events {
                                    return Err(SafetyTaskErr::NotEnoughDecides)
                                }
                            }
                            Ok(())
                        }.boxed()
                    }
                ))
            }

        }

    }


     /// build
    pub async fn build(
        self,
        registry: &mut GlobalRegistry,
        test_event_stream: ChannelStream<GlobalTestEvent>,
        hotshot_event_stream: UnboundedStream<Event<TYPES, I::Leaf>>,
    ) -> (HotShotTaskId, BoxFuture<'static, HotShotTaskCompleted>) {


        let test_event_handler = HandleEvent::<SafetyTaskTypes<TYPES, I>>(Arc::new(move |event, mut state| {
            let finisher = self.clone().gen_finisher();
            async move {
                match event {
                    GlobalTestEvent::ShutDown => {
                        let finished = finisher(&mut state.ctx).await;
                        let result =
                        match finished {
                            Ok(()) => HotShotTaskCompleted::ShutDown,
                            Err(err) => HotShotTaskCompleted::Error(Box::new(err)),

                        };
                        return (Some(result), state);
                        // return (Some(HotShotTaskCompleted, finisher(state.ctx).await)
                        // TODO run lambda on gathered state
                        // return (Some(HotShotTaskCompleted::ShutDown), state);
                    },
                    _ => { unimplemented!() }
                }
            }.boxed()
        }));
        let message_handler = HandleMessage::<SafetyTaskTypes<TYPES, I>>(Arc::new(move |msg, mut state| {
            async move {
                let  Event {view_number, event} = msg;
                match event {
                    EventType::Error { error } => {
                        // TODO better warn with node idx
                        warn!("View {:?} failed for a replica", view_number);
                        state.ctx.round_results.insert(view_number, ViewStatus::ViewFailed(ViewFailed(error)));
                    },
                    EventType::Decide { leaf_chain, qc } => {
                        // for leaf in leaf_chain {
                            // TODO how to test this
                        // }
                        // TODO how to do this counting
                        state.ctx.round_results.insert(view_number, ViewStatus::ViewSuccess(ViewSuccess(nll_todo())));

                    },
                    // these aren't failures
                    EventType::ReplicaViewTimeout { view_number } |
                    EventType::NextLeaderViewTimeout { view_number } |
                    EventType::ViewFinished { view_number } => todo!(),
                    _ => todo!(),
                }
                (None, state)
            }.boxed()
        }));

        let state = SafetyTask::default();

        let builder = TaskBuilder::<SafetyTaskTypes<TYPES, I>>::new("Safety Check Task".to_string())
            .register_event_stream(test_event_stream, FilterEvent::default()).await
            .register_registry(registry).await
            .register_state(state)
            .register_event_handler(test_event_handler)
            .register_message_handler(message_handler)
            .register_message_stream(hotshot_event_stream)
        ;
        let task_id = builder.get_task_id().unwrap();
        (task_id, SafetyTaskTypes::build(builder).launch())
    }
}

// /// Data Availability task types
pub type SafetyTaskTypes<
        TYPES: NodeType,
        I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>
    =
        HSTWithEventAndMessage<
            SafetyTaskErr,
            GlobalTestEvent,
            ChannelStream<GlobalTestEvent>,
            Event<TYPES, I::Leaf>,
            UnboundedStream<Event<TYPES, I::Leaf>>,
            SafetyTask<TYPES, I>
        >;
