use std::{collections::HashMap, sync::Arc};

use hotshot::{traits::TestableNodeImplementation, HotShotError};
use hotshot_types::{traits::node_implementation::NodeType, data::LeafType};
use snafu::Snafu;

// context for a round
// TODO eventually we want these to just be futures
// that we poll when things are event driven
// this context will be passed around
#[derive(Debug, Clone)]
pub struct NodeCtx<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> {
    /// results from previous rounds
    pub round_results: HashMap<TYPES::Time, ViewStatus<TYPES, I>>,
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> Default for NodeCtx<TYPES, I> {
    fn default() -> Self {
        Self { round_results: Default::default() }
    }
}


#[derive(Debug, Clone)]
pub enum ViewStatus<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> {
    InProgress(InProgress),
    ViewFailed(ViewFailed<TYPES>),
    ViewSuccess(ViewSuccess<TYPES, I::Leaf>)
}

#[derive(Debug, Clone)]
pub struct InProgress {

}

#[derive(Debug, Clone)]
pub struct ViewFailed<TYPES: NodeType>(pub Arc<HotShotError<TYPES>>);

#[derive(Debug, Clone)]
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
