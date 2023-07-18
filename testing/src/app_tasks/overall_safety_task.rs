use std::{collections::{HashMap, HashSet, hash_map::Entry}, sync::Arc};
use hotshot_task::event_stream::EventStream;

use async_compatibility_layer::channel::UnboundedStream;
use futures::{future::BoxFuture, FutureExt};
use hotshot::{traits::{TestableNodeImplementation, NodeImplementation}, HotShotError};
use hotshot_task::{task::{TaskErr, HotShotTaskCompleted, HandleEvent, HandleMessage, FilterEvent, HotShotTaskTypes, PassType}, task_impls::{HSTWithEventAndMessage, TaskBuilder}, event_stream::ChannelStream, MergeN, global_registry::{GlobalRegistry, HotShotTaskId}};
use hotshot_types::{traits::node_implementation::NodeType, certificate::QuorumCertificate, data::LeafType, event::{Event, EventType}, error::RoundTimedoutState};
use hotshot_task::task::TS;
use nll::nll_todo::nll_todo;
use std::marker::PhantomData;
use snafu::Snafu;

use crate::{test_runner::Node, test_errors::ConsensusTestError};

use super::{GlobalTestEvent, node_ctx::ViewStatus};

#[derive(Snafu, Debug)]
pub enum OverallSafetyTaskErr {
    // TODO make this more detailed
    TooManyFailures { expected: usize, got: usize },
    NotEnoughDecides { expected: usize, got: usize },
}
impl TaskErr for OverallSafetyTaskErr {}

/// Data availability task state
pub struct OverallSafetyTask<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> {
    pub handles: Vec<Node<TYPES, I>>,
    pub ctx: RoundCtx<TYPES, I>,
    pub test_event_stream: ChannelStream<GlobalTestEvent>,
}

// impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> Default
//     for OverallSafetyTask<TYPES, I>
// {
//     fn default() -> Self {
//         Self {
//             handles: vec![],
//             ctx: RoundCtx::default()
//         }
//     }
// }

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> TS
    for OverallSafetyTask<TYPES, I> {}

/// Result of running a round of consensus
#[derive(Debug)]
pub struct RoundResult<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> {
    /// Transactions that were submitted
    // pub txns: Vec<TYPES::Transaction>,
    /// Nodes that committed this round
    /// id -> (leaf, qc)
    pub success_nodes: HashMap<u64, (Vec<LEAF>, QuorumCertificate<TYPES, LEAF>)>,
    /// Nodes that failed to commit this round
    pub failed_nodes: HashMap<u64, Vec<Arc<HotShotError<TYPES>>>>,

    /// state of the majority of the nodes
    // pub agreed_state: Option<LEAF::MaybeState>,
    //
    // /// block of the majority of the nodes
    // pub agreed_block: Option<LEAF::DeltasType>,

    /// leaf of the majority of the nodes
    pub agreed_leaf: Option<LEAF>,

    /// whether or not the round succeeded (for a custom defn of succeeded)
    pub success: bool,
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> Default for RoundResult<TYPES, LEAF> {
    fn default() -> Self {
        Self {
            success_nodes: Default::default(),
            failed_nodes: Default::default(),
            agreed_leaf: Default::default(),
            success: false
        }
    }
}

/// smh my head I shouldn't need to implement this
/// Rust doesn't realize I doesn't need to implement default
impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> Default for RoundCtx<TYPES, I> {
    fn default() -> Self {
        Self {
            round_results: Default::default(),
            failed_views: Default::default(),
            successful_views: Default::default()
        }
    }
}

/// context for a round
/// TODO eventually we want these to just be futures
/// that we poll when things are event driven
/// this context will be passed around
#[derive(Debug)]
pub struct RoundCtx<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> {
    /// results from previous rounds
    /// view number -> round result
    pub round_results: HashMap<TYPES::Time, RoundResult<TYPES, <I as NodeImplementation<TYPES>>::Leaf>>,
    /// during the run view refactor
    pub failed_views: HashSet<TYPES::Time>,
    /// successful views
    pub successful_views: HashSet<TYPES::Time>,
}

/// cross node safety properties
#[derive(Clone)]
pub struct OverallSafetyPropertiesDescription {
    /// required number of successful views
    pub num_successful_views: usize,
    /// number of out of sync nodes before considered failed
    pub num_out_of_sync: usize,
    /// whether or not to check the leaf
    pub check_leaf: bool,
    /// whether or not to check the state
    pub check_state: bool,
    /// whether or not to check the block
    pub check_block: bool,
    /// whether or not to check the transaction pool
    pub check_transactions: bool,
    /// num of total rounds allowed to fail
    pub num_failed_rounds_total: usize,
    /// threshold calculator. Given number of live and total nodes, provide number of successes
    /// required to mark view as successful
    pub threshold_calculator: Arc<dyn Fn(usize, usize) -> usize + Send + Sync>,
}

impl std::fmt::Debug for OverallSafetyPropertiesDescription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OverallSafetyPropertiesDescription")
         .field("num successful views", &self.num_successful_views)
         .field("num_out_of_sync", &self.num_out_of_sync)
         .field("check leaf", &self.check_leaf)
         .field("check_state", &self.check_state)
         .field("check_block", &self.check_block)
         .field("check_transactions", &self.check_transactions)
         .field("num_failed_rounds_total", &self.num_failed_rounds_total)
         .finish()
    }

}

impl Default for OverallSafetyPropertiesDescription {
    fn default() -> Self {
        Self {
            num_successful_views: 5,
            num_out_of_sync: 5,
            check_leaf: false,
            check_state: true,
            check_block: true,
            check_transactions: true,
            num_failed_rounds_total: 10,
            // very strict
            threshold_calculator: Arc::new(|num_live, num_total| { num_total })
        }
    }
}

impl OverallSafetyPropertiesDescription {
    /// build a task
    pub fn build<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>(self) -> Box<
        dyn FnOnce(
            OverallSafetyTask<TYPES, I>,
            GlobalRegistry,
            ChannelStream<GlobalTestEvent>,
        )
            -> BoxFuture<'static, (HotShotTaskId, BoxFuture<'static, HotShotTaskCompleted>)>,
    > {
        let Self {
            num_out_of_sync,
            check_leaf,
            check_state,
            check_block,
            // TODO <https://github.com/EspressoSystems/HotShot/issues/1167>
            // We can't exactly check that the transactions all match those submitted
            // because of a type error. We ONLY have `MaybeState` and we need `State`.
            // unless we specialize, this won't happen.
            // so waiting on refactor for this
            // code is below:
            //
            //     ```
            //         let next_state /* : Option<_> */ = {
            //         if let Some(last_leaf) = ctx.prior_round_results.iter().last() {
            //             if let Some(parent_state) = last_leaf.agreed_state {
            //                 let mut block = <TYPES as NodeType>::StateType::next_block(Some(parent_state.clone()));
            //             }
            //         }
            //     };
            // ```
            check_transactions: _,
            num_failed_rounds_total,
            num_successful_views,
            threshold_calculator,
        }: Self = self;




        Box::new(move |mut state, mut registry, test_event_stream| {
            async move {
                let event_handler =
                    HandleEvent::<OverallSafetyTaskTypes<TYPES, I>>(Arc::new(move |event, state| {
                        async move {
                            match event {
                                GlobalTestEvent::ShutDown => {
                                    if state.ctx.successful_views.len() < num_successful_views {
                                        return (Some(HotShotTaskCompleted::Error(Box::new(OverallSafetyTaskErr::NotEnoughDecides { got: state.ctx.successful_views.len(), expected: num_successful_views}))), state);
                                    }

                                    if state.ctx.failed_views.len() >= num_failed_rounds_total {
                                        return (Some(HotShotTaskCompleted::Error(Box::new(OverallSafetyTaskErr::TooManyFailures { got: state.ctx.failed_views.len(), expected: num_failed_rounds_total}))), state);
                                    }
                                    // TODO check if we got enough successful views
                                    (Some(HotShotTaskCompleted::ShutDown), state)
                                }
                                _ => (None, state)
                            }
                        }.boxed()
                    }));

                let message_handler =
                    HandleMessage::<OverallSafetyTaskTypes<TYPES, I>>(Arc::new(move |msg, mut state| {
                        let threshold_calculator = threshold_calculator.clone();
                        async move {
                            let (idx, Event { view_number, event }) = msg;
                            match event {
                                EventType::Error { error } => {
                                    insert_error_to_context(&mut state.ctx, view_number, error);
                                },
                                EventType::Decide { leaf_chain, qc } => {
                                    let paired_up = (leaf_chain.to_vec(), (*qc).clone());
                                    match state.ctx.round_results.entry(view_number) {
                                        Entry::Occupied(mut o) => {
                                            o.get_mut().success_nodes.insert(idx as u64, paired_up);
                                        },
                                        Entry::Vacant(v) => {
                                            let mut round_result = RoundResult::default();
                                            round_result.success_nodes.insert(idx as u64, paired_up);
                                            v.insert(round_result);
                                        }
                                    }
                                }
                                EventType::ReplicaViewTimeout { view_number } =>
                                {
                                    let error =
                                        Arc::new(HotShotError::<TYPES>::ViewTimeoutError {
                                            view_number,
                                            state: RoundTimedoutState::TestCollectRoundEventsTimedOut,
                                        });
                                    insert_error_to_context(&mut state.ctx, view_number, error);
                                },
                                _ => {}
                            }

                            // update view count
                            let threshold = (threshold_calculator)(state.handles.len(), state.handles.len());

                            let mut view = state.ctx.round_results.get_mut(&view_number).unwrap();
                            if view.success_nodes.len() >= threshold {
                                // mark as success
                                view.success = true;
                                state.ctx.successful_views.insert(view_number);
                                if state.ctx.successful_views.len() >= num_successful_views {
                                    state.test_event_stream.publish(GlobalTestEvent::ShutDown).await;
                                    return (Some(HotShotTaskCompleted::ShutDown), state);
                                }
                            }

                            if view.failed_nodes.len() > state.handles.len() - threshold {
                                // mark as failure
                                view.success = false;
                                state.ctx.failed_views.insert(view_number);
                                if state.ctx.failed_views.len() >= num_failed_rounds_total {
                                    state.test_event_stream.publish(GlobalTestEvent::ShutDown).await;
                                    return (Some(HotShotTaskCompleted::Error(Box::new(OverallSafetyTaskErr::TooManyFailures { got: state.ctx.failed_views.len(), expected: num_failed_rounds_total}))), state);
                                }
                            }

                            (None, state)
                        }.boxed()

                    }));

                let mut streams = vec![];
                for handle in &mut state.handles {
                    streams.push(handle.handle.get_event_stream_known_impl(FilterEvent::default()).await.0);
                }
                let builder =
                    TaskBuilder::<OverallSafetyTaskTypes<TYPES, I>>::new("Test Overall Safety Task".to_string())
                    .register_event_stream(test_event_stream, FilterEvent::default())
                    .await
                    .register_registry(&mut registry)
                    .await
                    .register_message_handler(message_handler)
                    .register_message_stream(MergeN::new(streams))
                    .register_event_handler(event_handler)
                    .register_state(state);
                let task_id = builder.get_task_id().unwrap();
                (task_id, OverallSafetyTaskTypes::build(builder).launch())
            }.boxed()
        })
    }
}

/// inserts an error into the context
pub fn insert_error_to_context<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>(ctx: &mut RoundCtx<TYPES, I>, view_number: TYPES::Time, error: Arc<HotShotError<TYPES>>) {
    match ctx.round_results.entry(view_number) {
        Entry::Occupied(mut o) => {
            match o.get_mut().failed_nodes.entry(*view_number) {
                Entry::Occupied(mut o2) => {
                    o2.get_mut().push(error);
                },
                Entry::Vacant(v) => {
                    v.insert(vec![error]);
                }
            }
        },
        Entry::Vacant(v) => {
            let mut round_result = RoundResult::default();
            round_result.failed_nodes.insert(*view_number, vec![error]);
            v.insert(round_result);
        }
    }


}

pub type OverallSafetyTaskTypes<
    TYPES: NodeType,
    I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>,
> = HSTWithEventAndMessage<
    OverallSafetyTaskErr,
    GlobalTestEvent,
    ChannelStream<GlobalTestEvent>,
    (usize, Event<TYPES, I::Leaf>),
    MergeN<UnboundedStream<Event<TYPES, I::Leaf>>>,
    OverallSafetyTask<TYPES, I>,
>;
