use std::collections::HashMap;

use hotshot::traits::TestableNodeImplementation;
use hotshot_types::{traits::node_implementation::NodeType, data::LeafType};
use snafu::Snafu;

pub struct AggregatedViews<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> {

}

// context for a round
// TODO eventually we want these to just be futures
// that we poll when things are event driven
// this context will be passed around
#[derive(Debug)]
pub struct NodeCtx<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> {
    /// results from previous rounds
    pub round_results: HashMap<TYPES::Time, ViewStatus<TYPES, I>>,
    pub num_failed_views: usize,
    /// successful views
    pub num_decide_events: usize,
}

pub enum ViewStatus<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> {
    InProgress(InProgress),
    ViewFailed(ViewFailed),
    ViewSuccess(ViewSuccess<TYPES, I::Leaf>)
}

pub struct InProgress {

}

pub struct ViewFailed {
}

pub struct ViewSuccess<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> {
    /// Transactions that were committed
    pub txns: Vec<TYPES::Transaction>,
    /// state after decide event
    pub agreed_state: Option<LEAF::MaybeState>,

    /// block after decide event
    pub agreed_block: Option<LEAF::DeltasType>,

    /// leaf after decide event
    pub agreed_leaf: Option<LEAF>,
}
