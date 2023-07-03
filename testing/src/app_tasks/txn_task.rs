use std::{sync::Arc, time::Duration};
use async_compatibility_layer::{art::async_sleep, channel::UnboundedStream};

use either::Either::{self, Left, Right};
use futures::{FutureExt, future::BoxFuture};
use hotshot::{tasks::DATaskState, traits::{NodeImplementation, TestableNodeImplementation}};
use hotshot_types::{traits::node_implementation::NodeType, event::Event};
use nll::nll_todo::nll_todo;
use rand::thread_rng;
use snafu::Snafu;
use hotshot_task::{task::{TaskErr, TS, HST, HandleEvent, HandleMessage, HotShotTaskCompleted, FilterEvent, HotShotTaskTypes}, task_impls::{HSTWithEvent, HSTWithEventAndMessage, TaskBuilder}, event_stream::{ChannelStream, SendableStream}, GeneratedStream, global_registry::{HotShotTaskId, GlobalRegistry}, boxed_sync, Merge};

use crate::test_runner::Node;

use super::{TestTask, GlobalTestEvent, completion_task::CompletionTaskTypes};

// the obvious idea here is to pass in a "stream" that completes every `n` seconds
// the stream construction can definitely be fancier but that's the baseline idea

/// Data Availability task error
#[derive(Snafu, Debug)]
pub struct TxnTaskErr {}
impl TaskErr for TxnTaskErr {}

/// state of task that decides when things are completed
pub struct TxnTask<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>{
    // TODO should this be in a rwlock? Or maybe a similar abstraction to the registry is in order
    handles: Vec<Node<TYPES, I>>,
    next_node_idx: Option<usize>
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> TS for TxnTask<TYPES, I> {}

/// types for task that deices when things are completed
pub type TxnTaskTypes<
    TYPES: NodeType,
    I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> =
        HSTWithEventAndMessage<
            TxnTaskErr,
            GlobalTestEvent,
            ChannelStream<GlobalTestEvent>,
            Either<(), Event<TYPES, I::Leaf>>,
            Merge<GeneratedStream<()>, UnboundedStream<Event<TYPES, I::Leaf>>>,
            TxnTask<TYPES, I>
        >;

/// build the transaction task
pub enum TxnTaskBuilder {
    /// submit transactions in a round robin style using
    /// every `Duration` seconds
    RoundRobinTimeBased(Duration),
    /// TODO
    DistributionBased
    // others?
}

impl TxnTaskBuilder {
    /// build a task
    pub async fn build<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>(
        self,
        state: TxnTask<TYPES, I>,
        registry: &mut GlobalRegistry,
        test_event_stream: ChannelStream<GlobalTestEvent>,
        hotshot_event_stream: UnboundedStream<Event<TYPES, I::Leaf>>,
    ) ->  (HotShotTaskId, BoxFuture<'static, HotShotTaskCompleted>) {
        // consistency check
        match self {
            TxnTaskBuilder::RoundRobinTimeBased(_) => assert!(state.next_node_idx.is_some()),
            TxnTaskBuilder::DistributionBased => assert!(state.next_node_idx.is_none()),
        }
        // TODO we'll possibly want multiple criterion including:
        // - certain number of txns committed
        // - anchor of certain depth
        // - some other stuff? probably?
        let event_handler = HandleEvent::<TxnTaskTypes<TYPES, I>>(Arc::new(move |event, state| {
            async move {
                match event {
                    GlobalTestEvent::ShutDown => {
                        return (Some(HotShotTaskCompleted::ShutDown), state);
                    },
                    // TODO
                    _ => { unimplemented!() }
                }
            }.boxed()
        }));
        let message_handler = HandleMessage::<TxnTaskTypes<TYPES, I>>(Arc::new(move |msg, mut state| {
            async move {
                match msg {
                    Left(_) => {
                        if let Some(idx) = state.next_node_idx {
                            // submit to idx handle
                            // increment state
                            state.next_node_idx = Some((idx + 1) % state.handles.len());
                            match state.handles.get(idx) {
                                None => {
                                    // should do error
                                    unimplemented!()
                                },
                                Some(node) => {
                                    // use rand::seq::IteratorRandom;
                                    // handle.submit_transaction()
                                    // we're assuming all nodes have the same leaf.
                                    // If they don't match, this is probably fine since
                                    // it should be caught by an assertion (and the txn will be rejected anyway)
                                    let leaf = node.handle.get_decided_leaf().await;
                                    let txn = I::leaf_create_random_transaction(&leaf, &mut thread_rng(), 0);
                                    node.handle
                                        .submit_transaction(txn.clone())
                                        .await
                                        .expect("Could not send transaction");
                                    return (None, state)
                                }
                            }
                        } else {
                            // TODO make an issue
                            // in the case that this is random
                            // which I haven't implemented yet
                            unimplemented!()
                        }

                    },
                    Right(_) => {
                        return (None, state)
                    }
                }
            }.boxed()
        }));
        let stream_generator =
            match self {
                TxnTaskBuilder::RoundRobinTimeBased(duration) => {
                    GeneratedStream::new(Arc::new(move || {
                        let fut = async move {
                            async_sleep(duration).await;
                        };
                        boxed_sync(fut)
                    }))

                },
                TxnTaskBuilder::DistributionBased => unimplemented!(),
            };
        let merged_stream = Merge::new(stream_generator, hotshot_event_stream);
        let builder = TaskBuilder::<TxnTaskTypes<TYPES, I>>::new("Test Completion Task".to_string())
            .register_event_stream(test_event_stream, FilterEvent::default()).await
            .register_registry(registry).await
            .register_state(state)
            .register_event_handler(event_handler)
            .register_message_handler(message_handler)
            .register_message_stream(merged_stream)
            ;
        let task_id = builder.get_task_id().unwrap();
        (task_id, TxnTaskTypes::build(builder).launch())
    }
}
