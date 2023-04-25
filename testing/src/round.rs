use std::{collections::HashMap, sync::Arc, ops::Deref};

use futures::future::LocalBoxFuture;
use hotshot::{HotShotError, traits::{TestableNodeImplementation, NodeImplementation}};
use hotshot_types::{traits::node_implementation::NodeType, data::LeafType};

use crate::{test_runner::TestRunner, test_errors::{ConsensusFailedError, ConsensusTestError}, round_builder::{RoundSafetyCheckBuilder, RoundSetupBuilder}};

/// Alias for `(Vec<S>, Vec<B>)`. Used in [`RoundResult`].
pub type StateAndBlock<S, B> = (Vec<S>, Vec<B>);

/// Result of running a round of consensus
#[derive(Debug, Default)]
// TODO do we need static here
pub struct RoundResult<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> {
    /// Transactions that were submitted
    pub txns: Vec<TYPES::Transaction>,
    /// Nodes that committed this round
    pub success_nodes: HashMap<u64, StateAndBlock<LEAF::StateCommitmentType, LEAF::DeltasType>>,
    /// Nodes that failed to commit this round
    pub failed_nodes: HashMap<u64, HotShotError<TYPES>>,

    /// state of the majority of the nodes
    pub agreed_state: Option<LEAF::StateCommitmentType>,

    /// block of the majority of the nodes
    pub agreed_block: Option<LEAF::DeltasType>,

    /// leaf of the majority of the nodes
    pub agreed_leaf: Option<LEAF>,

    /// whether or not the round succeeded (for a custom defn of succeeded)
    pub success: bool,
}

/// context for a round
/// TODO eventually we want these to just be futures
/// that we poll when things are event driven
/// this context will be passed around
#[derive(Debug)]
pub struct RoundCtx<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> {
    pub prior_round_results: Vec<RoundResult<TYPES, <I as NodeImplementation<TYPES>>::Leaf>>,
    pub views_since_progress: usize,
    pub total_failed_views: usize,
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> Default for RoundCtx<TYPES, I> {
    fn default() -> Self {
        Self {
            prior_round_results: Default::default(),
            views_since_progress: 0,
            total_failed_views: 0,
        }
    }
}
impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> Round<TYPES, I> {
    pub fn empty() -> Self {
        Self {
            safety_check: RoundSafetyCheck(Arc::new(empty_safety_check)),
            setup_round: RoundSetup(Arc::new(empty_setup_round)),
            hooks: vec![]

        }
    }
}


impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> Default for Round<TYPES, I> {
    fn default() -> Self {
        Self {
            safety_check: RoundSafetyCheckBuilder::default().build(),
            setup_round: RoundSetupBuilder::default().build(),
            hooks: vec![]
        }
    }
}
impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> Clone for Round<TYPES, I> {
    fn clone(&self) -> Self {
        Self {
            setup_round: self.setup_round.clone(),
            safety_check: self.safety_check.clone(),
            hooks: self.hooks.clone()
        }
    }
}

pub fn empty_setup_round<'a, TYPES: NodeType, TRANS, I: TestableNodeImplementation<TYPES>>(
    _asdf: &'a mut TestRunner<TYPES, I>,
    _ctx: &'a RoundCtx<TYPES, I>,
) -> LocalBoxFuture<'a, Vec<TRANS>> {
    use futures::FutureExt;
    async move { vec![] }.boxed()
}

pub fn empty_safety_check<'a, TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(
    _asdf: &'a TestRunner<TYPES, I>,
    _ctx: &'a mut RoundCtx<TYPES, I>,
    _result: RoundResult<TYPES, <I as NodeImplementation<TYPES>>::Leaf>,
) -> LocalBoxFuture<'a, Result<(), ConsensusTestError>> {
    use futures::FutureExt;
    async move { Ok(()) }.boxed()
}

/// Type of function used for checking results after running a view of consensus
#[derive(Clone)]
pub struct RoundSafetyCheck<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(
    pub  Arc<
        dyn for<'a> Fn(
            &'a TestRunner<TYPES, I>,
            &'a mut RoundCtx<TYPES, I>,
            RoundResult<TYPES, <I as NodeImplementation<TYPES>>::Leaf>,
        ) -> LocalBoxFuture<'a, Result<(), ConsensusTestError>>,
    >,
);

#[derive(Clone)]
pub struct RunnerBuilder<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(
    pub Arc<
        dyn Fn(&Round<TYPES, I>) -> TestRunner<TYPES, I>>
);

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> Deref
    for RunnerBuilder<TYPES, I>
{
    type Target =
        dyn Fn(&Round<TYPES, I>) -> TestRunner<TYPES, I>;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}


/// Type of function used for checking results after running a view of consensus
#[derive(Clone)]
pub struct RoundHook<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(
    pub  Arc<
        dyn for<'a> Fn(
            &'a TestRunner<TYPES, I>,
            &'a RoundCtx<TYPES, I>,
        ) -> LocalBoxFuture<'a, Result<(), ConsensusFailedError>>,
    >,
);

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> Deref
    for RoundHook<TYPES, I>
{
    type Target =
        dyn for<'a> Fn(
            &'a TestRunner<TYPES, I>,
            &'a RoundCtx<TYPES, I>,
        ) -> LocalBoxFuture<'a, Result<(), ConsensusFailedError>>;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> Deref
    for RoundSafetyCheck<TYPES, I>
{
    type Target = dyn for<'a> Fn(
        &'a TestRunner<TYPES, I>,
        &'a mut RoundCtx<TYPES, I>,
        RoundResult<TYPES, <I as NodeImplementation<TYPES>>::Leaf>,
    ) -> LocalBoxFuture<'a, Result<(), ConsensusTestError>>;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

/// Type of function used for configuring a round of consensus
#[derive(Clone)]
pub struct RoundSetup<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(
    pub  Arc<
        dyn for<'a> Fn(
            &'a mut TestRunner<TYPES, I>,
            &'a RoundCtx<TYPES, I>,
        ) -> LocalBoxFuture<'a, Vec<TYPES::Transaction>>,
    >,
);

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> Deref for RoundSetup<TYPES, I> {
    type Target = dyn for<'a> Fn(
        &'a mut TestRunner<TYPES, I>,
        &'a RoundCtx<TYPES, I>,
    ) -> LocalBoxFuture<'a, Vec<TYPES::Transaction>>;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

/// functions to run a round of consensus
/// the control flow is: (0) setup round, (1) hooks, (2) execute round, (3) safety check
pub struct Round<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> {
    /// Safety check before round is set up and run
    /// to ensure consistent state
    pub hooks: Vec<RoundHook<TYPES, I>>,

    /// Round set up
    pub setup_round: RoundSetup<TYPES, I>,

    /// Safety check after round is complete
    pub safety_check: RoundSafetyCheck<TYPES, I>,
}
