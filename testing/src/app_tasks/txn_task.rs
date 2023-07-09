use async_compatibility_layer::{art::async_sleep, channel::UnboundedStream};
use std::{sync::Arc, time::Duration};

use either::Either::{self, Left, Right};
use futures::{future::BoxFuture, FutureExt};
use hotshot::traits::{NodeImplementation, TestableNodeImplementation};
use hotshot_task::{
    boxed_sync,
    event_stream::{ChannelStream, SendableStream},
    global_registry::{GlobalRegistry, HotShotTaskId},
    task::{
        FilterEvent, HandleEvent, HandleMessage, HotShotTaskCompleted, HotShotTaskTypes, TaskErr,
        HST, TS,
    },
    task_impls::{HSTWithEvent, HSTWithEventAndMessage, TaskBuilder},
    GeneratedStream, Merge,
};
use hotshot_types::{event::Event, traits::node_implementation::NodeType};
use rand::thread_rng;
use snafu::Snafu;

use crate::test_runner::Node;

use super::{completion_task::CompletionTaskTypes, GlobalTestEvent, TestTask};

// the obvious idea here is to pass in a "stream" that completes every `n` seconds
// the stream construction can definitely be fancier but that's the baseline idea

/// Data Availability task error
#[derive(Snafu, Debug)]
pub struct TxnTaskErr {}
impl TaskErr for TxnTaskErr {}

/// state of task that decides when things are completed
pub struct TxnTask<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> {
    // TODO should this be in a rwlock? Or maybe a similar abstraction to the registry is in order
    pub handles: Vec<Node<TYPES, I>>,
    pub next_node_idx: Option<usize>,
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> TS
    for TxnTask<TYPES, I>
{
}

/// types for task that deices when things are completed
pub type TxnTaskTypes<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> =
    HSTWithEventAndMessage<
        TxnTaskErr,
        GlobalTestEvent,
        ChannelStream<GlobalTestEvent>,
        (),
        GeneratedStream<()>,
        TxnTask<TYPES, I>,
    >;

/// build the transaction task
#[derive(Clone, Debug)]
pub enum TxnTaskDescription {
    /// submit transactions in a round robin style using
    /// every `Duration` seconds
    RoundRobinTimeBased(Duration),
    /// TODO
    DistributionBased, // others?
}

impl TxnTaskDescription {
    /// build a task
    pub fn build<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>(
        self,
    ) -> Box<
        dyn FnOnce(
            TxnTask<TYPES, I>,
            GlobalRegistry,
            ChannelStream<GlobalTestEvent>,
        )
            -> BoxFuture<'static, (HotShotTaskId, BoxFuture<'static, HotShotTaskCompleted>)>,
    > {
        Box::new(move |state, mut registry, test_event_stream| {
            async move {
                // consistency check
                match self {
                    TxnTaskDescription::RoundRobinTimeBased(_) => {
                        assert!(state.next_node_idx.is_some())
                    }
                    TxnTaskDescription::DistributionBased => assert!(state.next_node_idx.is_none()),
                }
                // TODO we'll possibly want multiple criterion including:
                // - certain number of txns committed
                // - anchor of certain depth
                // - some other stuff? probably?
                let event_handler =
                    HandleEvent::<TxnTaskTypes<TYPES, I>>(Arc::new(move |event, state| {
                        async move {
                            match event {
                                GlobalTestEvent::ShutDown => {
                                    return (Some(HotShotTaskCompleted::ShutDown), state);
                                }
                                // TODO
                                _ => {
                                    unimplemented!()
                                }
                            }
                        }
                        .boxed()
                    }));
                let message_handler =
                    HandleMessage::<TxnTaskTypes<TYPES, I>>(Arc::new(move |msg, mut state| {
                        async move {
                            if let Some(idx) = state.next_node_idx {
                                // submit to idx handle
                                // increment state
                                state.next_node_idx = Some((idx + 1) % state.handles.len());
                                match state.handles.get(idx) {
                                    None => {
                                        // should do error
                                        unimplemented!()
                                    }
                                    Some(node) => {
                                        // use rand::seq::IteratorRandom;
                                        // handle.submit_transaction()
                                        // we're assuming all nodes have the same leaf.
                                        // If they don't match, this is probably fine since
                                        // it should be caught by an assertion (and the txn will be rejected anyway)
                                        let leaf = node.handle.get_decided_leaf().await;
                                        let txn = I::leaf_create_random_transaction(
                                            &leaf,
                                            &mut thread_rng(),
                                            0,
                                        );
                                        node.handle
                                            .submit_transaction(txn.clone())
                                            .await
                                            .expect("Could not send transaction");
                                        return (None, state);
                                    }
                                }
                            } else {
                                // TODO make an issue
                                // in the case that this is random
                                // which I haven't implemented yet
                                unimplemented!()
                            }
                        }
                        .boxed()
                    }));
                let stream_generator = match self {
                    TxnTaskDescription::RoundRobinTimeBased(duration) => {
                        GeneratedStream::new(Arc::new(move || {
                            let fut = async move {
                                async_sleep(duration).await;
                            };
                            boxed_sync(fut)
                        }))
                    }
                    TxnTaskDescription::DistributionBased => unimplemented!(),
                };
                let builder = TaskBuilder::<TxnTaskTypes<TYPES, I>>::new(
                    "Test Transaction Submission Task".to_string(),
                )
                .register_event_stream(test_event_stream, FilterEvent::default())
                .await
                .register_registry(&mut registry)
                .await
                .register_state(state)
                .register_event_handler(event_handler)
                .register_message_handler(message_handler)
                .register_message_stream(stream_generator);
                let task_id = builder.get_task_id().unwrap();
                (task_id, TxnTaskTypes::build(builder).launch())
            }
            .boxed()
        })
    }
}
