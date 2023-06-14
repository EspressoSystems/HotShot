use std::{collections::HashMap, ops::Deref, sync::Arc};

use futures::future::LocalBoxFuture;
use hotshot::{
    traits::{NodeImplementation, TestableNodeImplementation},
    HotShotError, HotShotType, SystemContext,
};
use hotshot_types::certificate::QuorumCertificate;
use hotshot_types::{
    data::LeafType,
    message::Message,
    traits::{
        election::ConsensusExchange,
        network::CommunicationChannel,
        node_implementation::{ExchangesType, NodeType, QuorumCommChannel, QuorumEx},
    },
};
use tracing::error;

use crate::{
    round_builder::{RoundSafetyCheckBuilder, RoundSetupBuilder},
    test_errors::{ConsensusRoundError, ConsensusTestError},
    test_runner::TestRunner,
};

/// Alias for `(Vec<S>, Vec<B>)`. Used in [`RoundResult`].
pub type StateAndBlock<S, B> = (Vec<S>, Vec<B>);

/// Result of running a round of consensus
#[derive(Debug)]
// TODO do we need static here
pub struct RoundResult<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> {
    /// Transactions that were submitted
    pub txns: Vec<TYPES::Transaction>,
    /// Nodes that committed this round
    /// id -> (leaf, qc)
    pub success_nodes: HashMap<u64, (Vec<LEAF>, QuorumCertificate<TYPES, LEAF>)>,
    /// Nodes that failed to commit this round
    pub failed_nodes: HashMap<u64, HotShotError<TYPES>>,

    /// state of the majority of the nodes
    pub agreed_state: Option<LEAF::MaybeState>,

    /// block of the majority of the nodes
    pub agreed_block: Option<LEAF::DeltasType>,

    /// leaf of the majority of the nodes
    pub agreed_leaf: Option<LEAF>,

    /// whether or not the round succeeded (for a custom defn of succeeded)
    pub success: Result<(), ConsensusRoundError>,
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> Default for RoundResult<TYPES, LEAF> {
    fn default() -> Self {
        Self {
            txns: Default::default(),
            success_nodes: Default::default(),
            failed_nodes: Default::default(),
            agreed_state: Default::default(),
            agreed_block: Default::default(),
            agreed_leaf: Default::default(),
            success: Ok(()),
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
    pub prior_round_results: Vec<RoundResult<TYPES, <I as NodeImplementation<TYPES>>::Leaf>>,
    /// views since we had a successful commit
    pub views_since_progress: usize,
    /// totall number o failed views. TODO this will need to change
    /// during the run view refactor
    pub total_failed_views: usize,
    /// successful views
    pub total_successful_views: usize,
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>
    RoundCtx<TYPES, I>
{
    /// prints a summary
    /// TODO this should probably include a formatter as an arg
    pub fn print_summary(&self) {
        let errors = self
            .prior_round_results
            .iter()
            .enumerate()
            .map(|(idx, r)| (format!("View: {}", idx), &r.success))
            .collect::<Vec<_>>();
        error!("SUMMARY OF TEST: {:#?}", errors);
    }
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> Default
    for RoundCtx<TYPES, I>
{
    fn default() -> Self {
        Self {
            prior_round_results: Default::default(),
            views_since_progress: 0,
            total_failed_views: 0,
            total_successful_views: 0,
        }
    }
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> Round<TYPES, I>
where
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
{
    /// an empty `Round`
    pub fn empty() -> Self {
        Self {
            safety_check: RoundSafetyCheck(Arc::new(empty_safety_check)),
            setup_round: RoundSetup(Arc::new(empty_setup_round)),
            hooks: vec![],
        }
    }
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> Default
    for Round<TYPES, I>
where
    SystemContext<TYPES::ConsensusType, TYPES, I>: HotShotType<TYPES, I>,
    I::Exchanges: ExchangesType<
        TYPES::ConsensusType,
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
        Networks = (QuorumCommChannel<TYPES, I>, I::CommitteeCommChannel),
    >,
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
{
    fn default() -> Self {
        Self {
            safety_check: RoundSafetyCheckBuilder::default().build(),
            setup_round: RoundSetupBuilder::default().build(),
            hooks: vec![],
        }
    }
}
impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> Clone
    for Round<TYPES, I>
where
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
{
    fn clone(&self) -> Self {
        Self {
            setup_round: self.setup_round.clone(),
            safety_check: self.safety_check.clone(),
            hooks: self.hooks.clone(),
        }
    }
}

/// an empty `RoundSetup`
pub fn empty_setup_round<
    'a,
    TYPES: NodeType,
    TRANS,
    I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>,
>(
    _asdf: &'a mut TestRunner<TYPES, I>,
    _ctx: &'a RoundCtx<TYPES, I>,
) -> LocalBoxFuture<'a, Vec<TRANS>>
where
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
{
    use futures::FutureExt;
    async move { vec![] }.boxed()
}

/// an empty `RoundSafetyCheck`
pub fn empty_safety_check<
    'a,
    TYPES: NodeType,
    I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>,
>(
    _asdf: &'a TestRunner<TYPES, I>,
    _ctx: &'a mut RoundCtx<TYPES, I>,
    _result: RoundResult<TYPES, <I as NodeImplementation<TYPES>>::Leaf>,
) -> LocalBoxFuture<'a, Result<(), ConsensusTestError>>
where
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
{
    use futures::FutureExt;
    async move { Ok(()) }.boxed()
}

/// Type of function used for checking results after running a view of consensus
#[derive(Clone)]
#[allow(clippy::type_complexity)]
pub struct RoundSafetyCheck<
    TYPES: NodeType,
    I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>,
>(
    pub  Arc<
        dyn for<'a> Fn(
            &'a TestRunner<TYPES, I>,
            &'a mut RoundCtx<TYPES, I>,
            RoundResult<TYPES, <I as NodeImplementation<TYPES>>::Leaf>,
        ) -> LocalBoxFuture<'a, Result<(), ConsensusTestError>>,
    >,
)
where
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >;

/// Type of function used for checking results after running a view of consensus
#[derive(Clone)]
#[allow(clippy::type_complexity)]
pub struct RoundHook<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>(
    pub  Arc<
        dyn for<'a> Fn(
            &'a TestRunner<TYPES, I>,
            &'a RoundCtx<TYPES, I>,
        ) -> LocalBoxFuture<'a, Result<(), ConsensusTestError>>,
    >,
)
where
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >;

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> Deref
    for RoundHook<TYPES, I>
where
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
{
    type Target = dyn for<'a> Fn(
        &'a TestRunner<TYPES, I>,
        &'a RoundCtx<TYPES, I>,
    ) -> LocalBoxFuture<'a, Result<(), ConsensusTestError>>;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> Deref
    for RoundSafetyCheck<TYPES, I>
where
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
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
#[allow(clippy::type_complexity)]
pub struct RoundSetup<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>(
    pub  Arc<
        dyn for<'a> Fn(
            &'a mut TestRunner<TYPES, I>,
            &'a RoundCtx<TYPES, I>,
        ) -> LocalBoxFuture<'a, Vec<TYPES::Transaction>>,
    >,
)
where
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >;

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> Deref
    for RoundSetup<TYPES, I>
where
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
{
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
pub struct Round<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>
where
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
{
    /// Safety check before round is set up and run
    /// to ensure consistent state
    pub hooks: Vec<RoundHook<TYPES, I>>,

    /// Round set up
    pub setup_round: RoundSetup<TYPES, I>,

    /// Safety check after round is complete
    pub safety_check: RoundSafetyCheck<TYPES, I>,
}
