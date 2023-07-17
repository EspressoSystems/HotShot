use std::{collections::HashMap, sync::Arc};

use async_compatibility_layer::channel::UnboundedStream;
use futures::{future::BoxFuture, FutureExt};
use hotshot::{traits::{TestableNodeImplementation, NodeImplementation}, HotShotError};
use hotshot_task::{task::{TaskErr, HotShotTaskCompleted, HandleEvent, HandleMessage, FilterEvent, HotShotTaskTypes, PassType}, task_impls::{HSTWithEventAndMessage, TaskBuilder}, event_stream::ChannelStream, MergeN, global_registry::{GlobalRegistry, HotShotTaskId}};
use hotshot_types::{traits::node_implementation::NodeType, certificate::QuorumCertificate, data::LeafType, event::{Event, EventType}};
use hotshot_task::task::TS;
use nll::nll_todo::nll_todo;
use std::marker::PhantomData;
use snafu::Snafu;

use crate::{test_runner::Node};

use super::{GlobalTestEvent, node_ctx::ViewStatus};

// TODO
#[derive(Clone, Debug)]
pub struct OverallSafetyPropertiesDescription {}

#[derive(Snafu, Debug)]
pub enum OverallSafetyTaskErr {
    // TODO make this more detailed
    TooManyFailures,
    NotEnoughDecides,
}
impl TaskErr for OverallSafetyTaskErr {}

/// Data availability task state
pub struct OverallSafetyTask<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> {
    pub handles: Vec<Node<TYPES, I>>,
    pub ctx: RoundCtx<TYPES, I>
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> Default
    for OverallSafetyTask<TYPES, I>
{
    fn default() -> Self {
        Self {
            handles: vec![],
            ctx: RoundCtx::default()
        }
    }
}

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
    pub failed_nodes: HashMap<u64, /* HotShotError<TYPES> */ ()>,

    /// state of the majority of the nodes
    // pub agreed_state: Option<LEAF::MaybeState>,
    //
    // /// block of the majority of the nodes
    // pub agreed_block: Option<LEAF::DeltasType>,

    /// leaf of the majority of the nodes
    pub agreed_leaf: Option<LEAF>,

    /// whether or not the round succeeded (for a custom defn of succeeded)
    pub success: Result<(), HotShotError<TYPES>>,
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> Default for RoundResult<TYPES, LEAF> {
    fn default() -> Self {
        Self {
            success_nodes: Default::default(),
            failed_nodes: Default::default(),
            agreed_leaf: Default::default(),
            success: Ok(())
        }
    }
}

/// smh my head I shouldn't need to implement this
/// Rust doesn't realize I doesn't need to implement default
impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> Default for RoundCtx<TYPES, I> {
    fn default() -> Self {
        Self {
            prior_round_results: Default::default(),
            views_since_progress: Default::default(),
            total_failed_views: Default::default(),
            total_successful_views: Default::default()
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
    pub prior_round_results: HashMap<usize, RoundResult<TYPES, <I as NodeImplementation<TYPES>>::Leaf>>,
    /// views since we had a successful commit
    pub views_since_progress: HashMap<usize, usize>,
    /// during the run view refactor
    pub total_failed_views: HashMap<usize, usize>,
    /// successful views
    pub total_successful_views: usize,
}

pub struct OverallSafetyTaskDescription {
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
    /// num of consecutive failed rounds before failing
    pub num_failed_consecutive_rounds: usize,
    /// num of total rounds allowed to fail
    pub num_failed_rounds_total: usize,
}

impl Default for OverallSafetyTaskDescription {
    fn default() -> Self {
        Self {
            num_out_of_sync: 5,
            check_leaf: false,
            check_state: true,
            check_block: true,
            check_transactions: true,
            num_failed_consecutive_rounds: 5,
            num_failed_rounds_total: 10,
        }
    }
}

impl OverallSafetyTaskDescription {
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
            num_failed_consecutive_rounds,
            num_failed_rounds_total,
        }: Self = self;




        Box::new(move |state, mut registry, test_event_stream| {
            async move {

                let event_handler =
                    HandleEvent::<OverallSafetyTaskTypes<TYPES, I>>(Arc::new(move |event, state| {
                        async move {
                            match event {
                                GlobalTestEvent::ShutDown => {
                                    // TODO check if we got enough successful views
                                    (Some(HotShotTaskCompleted::ShutDown), state)
                                }
                                _ => (None, state)
                            }
                        }.boxed()
                    }));

                let message_handler =
                    HandleMessage::<OverallSafetyTaskTypes<TYPES, I>>(Arc::new(move |msg, mut state| {
                        async move {
                            let (idx, Event { view_number, event }) = msg;
                            match event {
                                EventType::Error { error } => {

                                },
                                EventType::Decide { leaf_chain, qc } => {
                                    // for leaf in leaf_chain {
                                    // TODO how to test this
                                    // }
                                    // TODO how to do this counting
                                    // state.ctx.round_results.insert(
                                    //     view_number,
                                    //     ViewStatus::ViewSuccess(nll_todo()),
                                    //     );
                                }
                                // these aren't failures
                                EventType::ReplicaViewTimeout { view_number }
                                | EventType::NextLeaderViewTimeout { view_number }
                                | EventType::ViewFinished { view_number } => todo!(),
                                _ => todo!()

                            }
                            (None, state)
                        }.boxed()

                    }));

                let mut streams = vec![];
                for handle in state.handles.iter() {
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
