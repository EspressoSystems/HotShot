
use std::sync::Arc;

use futures::future::BoxFuture;
use hotshot::{tasks::DATaskState, traits::TestableNodeImplementation};
use hotshot_types::traits::node_implementation::NodeType;
use nll::nll_todo::nll_todo;
use snafu::Snafu;
use hotshot_task::{task::{TaskErr, TS, HST, HotShotTaskCompleted}, task_impls::HSTWithEvent, event_stream::ChannelStream, global_registry::{HotShotTaskId, GlobalRegistry}};

use super::GlobalTestEvent;

/// Data Availability task error
#[derive(Snafu, Debug)]
pub struct SafetyTaskErr {}
impl TaskErr for SafetyTaskErr {}

/// Data availability task state
#[derive(Debug)]
pub struct SafetyTask {
    /// lambda to run at end
    finisher: (),
}
impl TS for SafetyTask {}

pub struct SafetyTaskFinisherBuilder {
    num_decided_views: Option<usize>,
    num_decided_txns: Option<usize>,
}

/// Type of function used for checking results after running a view of consensus
// #[derive(Clone)]
// #[allow(clippy::type_complexity)]
// pub struct SafetyFinisher<
//     TYPES: NodeType,
//     I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>,
// >(
//     pub  Arc<
//         dyn for<'a> Fn(
//             &'a TestRunner<TYPES, I>,
//             &'a mut RoundCtx<TYPES, I>,
//             RoundResult<TYPES, <I as NodeImplementation<TYPES>>::Leaf>,
//         ) -> LocalBoxFuture<'a, Result<(), ConsensusTestError>>,
//     >,
// );



/// builder describing custom safety properties
pub struct SafetyTaskBuilder {
}

// yes, we want consistency. That's about all...
// if there's something obviously wrong, then report that

impl SafetyTaskBuilder {
    /// build
    pub async fn build(
        self,
        state: SafetyTask,
        registry: &mut GlobalRegistry,
        event_stream: ChannelStream<GlobalTestEvent>,
    ) -> (HotShotTaskId, BoxFuture<'static, HotShotTaskCompleted>) {
        nll_todo()
    }
}

// /// Data Availability task types
pub type SafetyTaskTypes = HST<SafetyTaskErr>;
