use async_lock::Mutex;

use hotshot_testing::{
    round::{Round, RoundCtx, RoundHook, RoundResult, RoundSafetyCheck, RoundSetup},
    round_builder::RoundBuilder,
    test_builder::{TestBuilder, TestMetadata, TimingData},
    test_errors::{ConsensusSafetyFailedSnafu, ConsensusTestError},
    test_types::{
        AppliedTestRunner, StandardNodeImplType, StaticCommitteeTestTypes, StaticNodeImplType,
        VrfTestTypes,
    },
};

use commit::Committable;
use either::Either::{self, Left};
use futures::{
    future::{join_all, LocalBoxFuture},
    FutureExt,
};
use hotshot::{
    certificate::QuorumCertificate,
    demos::vdemo::random_validating_leaf,
    traits::{NodeImplementation, TestableNodeImplementation},
    types::HotShotHandle,
};

use hotshot_types::message::Message;
use hotshot_types::traits::election::QuorumExchangeType;
use hotshot_types::{
    data::{LeafType, ValidatingLeaf, ValidatingProposal},
    event::EventType,
    message::{ConsensusMessage, Proposal},
    traits::{
        election::{ConsensusExchange, SignedCertificate, TestableElection},
        node_implementation::NodeType,
        state::{ConsensusTime, ValidatingConsensus},
    },
    vote::QuorumVote,
};

use snafu::{ensure, OptionExt};

use std::iter::once;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tracing::{instrument, warn};

const NUM_VIEWS: u64 = 100;
const DEFAULT_NODE_ID: u64 = 0;

type AppliedValidatingTestRunner<TYPES, I> = AppliedTestRunner<TYPES, I>;

enum QueuedMessageTense {
    Past(Option<usize>),
    Future(Option<usize>),
}

/// Returns true if `node_id` is the leader of `view_number`
async fn is_upcoming_validating_leader<
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    I: TestableNodeImplementation<TYPES, Leaf = ValidatingLeaf<TYPES>>,
>(
    runner: &AppliedValidatingTestRunner<TYPES, I>,
    node_id: u64,
    view_number: TYPES::Time,
) -> bool
where
    I::QuorumExchange: QuorumExchangeType<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
        Proposal = ValidatingProposal<TYPES, I::Leaf>,
        Certificate = QuorumCertificate<TYPES, I::Leaf>,
        Vote = QuorumVote<TYPES, I::Leaf>,
    >,
{
    let handle = runner.get_handle(node_id).unwrap();
    let leader = handle.get_leader(view_number).await;
    leader == handle.get_public_key()
}

/// Builds and submits a random proposal for the specified view number
async fn submit_validating_proposal<
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    I: TestableNodeImplementation<TYPES, Leaf = ValidatingLeaf<TYPES>>,
>(
    runner: &AppliedValidatingTestRunner<TYPES, I>,
    sender_node_id: u64,
    view_number: TYPES::Time,
) where
    I::QuorumExchange: QuorumExchangeType<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
        Proposal = ValidatingProposal<TYPES, I::Leaf>,
        Certificate = QuorumCertificate<TYPES, I::Leaf>,
        Vote = QuorumVote<TYPES, I::Leaf>,
    >,
{
    let mut rng = rand::thread_rng();
    let handle = runner.get_handle(sender_node_id).unwrap();

    let genesis = <I as TestableNodeImplementation<TYPES>>::block_genesis();
    // Build proposal
    let mut leaf = random_validating_leaf::<TYPES>(genesis, &mut rng);
    leaf.view_number = view_number;
    leaf.set_height(handle.get_decided_leaf().await.get_height() + 1);
    let signature = handle.sign_validating_or_commitment_proposal(&leaf.commit());
    let msg = ConsensusMessage::Proposal(Proposal {
        data: leaf.into(),
        signature,
    });

    // Send proposal
    handle.send_broadcast_consensus_message(msg.clone()).await;
}

/// Builds and submits a random vote for the specified view number from the specified node
async fn submit_validating_vote<
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    I: TestableNodeImplementation<TYPES, Leaf = ValidatingLeaf<TYPES>>,
>(
    runner: &AppliedValidatingTestRunner<TYPES, I>,
    sender_node_id: u64,
    view_number: TYPES::Time,
    recipient_node_id: u64,
) where
    I::QuorumExchange: QuorumExchangeType<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
        Proposal = ValidatingProposal<TYPES, I::Leaf>,
        Certificate = QuorumCertificate<TYPES, I::Leaf>,
        Vote = QuorumVote<TYPES, I::Leaf>,
    >,
    <I::QuorumExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership:
        TestableElection<TYPES>,
{
    let mut rng = rand::thread_rng();
    let handle = runner.get_handle(sender_node_id).unwrap();

    // Build vote
    let mut leaf = random_validating_leaf::<TYPES>(I::block_genesis(), &mut rng);
    leaf.view_number = view_number;
    let msg = handle.create_yes_message(
        leaf.justify_qc.commit(),
        leaf.commit(),
        leaf.view_number,
<<I::QuorumExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership as TestableElection<TYPES>>::generate_test_vote_token(),
    );

    let recipient = runner
        .get_handle(recipient_node_id)
        .unwrap()
        .get_public_key();

    // Send vote
    handle
        .send_direct_consensus_message(msg.clone(), recipient)
        .await;
}

/// Return an enum representing the state of the message queue
fn get_queue_len(is_past: bool, len: Option<usize>) -> QueuedMessageTense {
    if is_past {
        QueuedMessageTense::Past(len)
    } else {
        QueuedMessageTense::Future(len)
    }
}

/// Checks that votes are queued correctly for views 1..NUM_VIEWS
fn test_validating_vote_queueing_post_safety_check<
    'a,
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    I: TestableNodeImplementation<TYPES, Leaf = ValidatingLeaf<TYPES>>,
>(
    runner: &'a AppliedValidatingTestRunner<TYPES, I>,
    _ctx: &'a mut RoundCtx<TYPES, I>,
    _results: RoundResult<TYPES, ValidatingLeaf<TYPES>>,
) -> LocalBoxFuture<'a, Result<(), ConsensusTestError>>
where
    I::QuorumExchange: QuorumExchangeType<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
        Proposal = ValidatingProposal<TYPES, I::Leaf>,
        Certificate = QuorumCertificate<TYPES, I::Leaf>,
        Vote = QuorumVote<TYPES, I::Leaf>,
    >,
{
    async move {
        let node_id = DEFAULT_NODE_ID;
        let mut result = Ok(());

        let handle = runner.get_handle(node_id).unwrap();
        let cur_view = handle.get_current_view().await;


        for i in 1..NUM_VIEWS {
            if is_upcoming_validating_leader(runner, node_id, TYPES::Time::new(i)).await {
                let ref_view_number = TYPES::Time::new(i - 1);
                let is_past = ref_view_number < cur_view;
                let len = handle
                    .get_next_leader_receiver_channel_len(ref_view_number)
                    .await;

                let queue_state = get_queue_len(is_past, len);
                match queue_state {
                    QueuedMessageTense::Past(Some(len)) => {
                        result = Err(ConsensusTestError::ConsensusSafetyFailed {
                                                description: format!("Past view's next leader receiver channel for node {node_id} still exists for {ref_view_number:?} with {len} items in it.  We are currently in {cur_view:?}")});
                    }
                    QueuedMessageTense::Future(None) => {
                        result = Err(ConsensusTestError::ConsensusSafetyFailed  {
                            description: format!("Next ValidatingLeader did not properly queue future vote for {ref_view_number:?}.  We are currently in {cur_view:?}")});
                    }
                    _ => {}
                }
            }
    }
        result
    }
    .boxed_local()
}

/// For 1..NUM_VIEWS submit votes to each node in the network
fn test_validating_vote_queueing_round_setup<
    'a,
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    I: TestableNodeImplementation<TYPES, Leaf = ValidatingLeaf<TYPES>>,
>(
    runner: &'a mut AppliedValidatingTestRunner<TYPES, I>,
    _ctx: &'a RoundCtx<TYPES, I>,
) -> LocalBoxFuture<'a, Vec<TYPES::Transaction>>
where
    I::QuorumExchange: QuorumExchangeType<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
        Proposal = ValidatingProposal<TYPES, I::Leaf>,
        Certificate = QuorumCertificate<TYPES, I::Leaf>,
        Vote = QuorumVote<TYPES, I::Leaf>,
    >,
    <I::QuorumExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership:
        TestableElection<TYPES>,
{
    async move {
        let node_id = DEFAULT_NODE_ID;
        for j in runner.ids() {
            for i in 1..NUM_VIEWS {
                if is_upcoming_validating_leader(runner, node_id, TYPES::Time::new(i)).await {
                    submit_validating_vote(runner, j, TYPES::Time::new(i - 1), node_id).await;
                }
            }
        }
        Vec::new()
    }
    .boxed_local()
}

/// Checks views 0..NUM_VIEWS for whether proposal messages are properly queued
fn test_validating_proposal_queueing_post_safety_check<
    'a,
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    I: TestableNodeImplementation<TYPES, Leaf = ValidatingLeaf<TYPES>>,
>(
    runner: &'a AppliedValidatingTestRunner<TYPES, I>,
    _cx: &'a mut RoundCtx<TYPES, I>,
    _results: RoundResult<TYPES, ValidatingLeaf<TYPES>>,
) -> LocalBoxFuture<'a, Result<(), ConsensusTestError>>
where
    I::QuorumExchange: QuorumExchangeType<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
        Proposal = ValidatingProposal<TYPES, I::Leaf>,
        Certificate = QuorumCertificate<TYPES, I::Leaf>,
        Vote = QuorumVote<TYPES, I::Leaf>,
    >,
{
    async move {
        let node_id = DEFAULT_NODE_ID;
        let mut result = Ok(());

        for node in runner.nodes() {

            let handle = node;
            let cur_view = handle.get_current_view().await;

            for i in 0..NUM_VIEWS {
                if is_upcoming_validating_leader(runner, node_id, TYPES::Time::new(i)).await {
                    let ref_view_number = TYPES::Time::new(i);
                    let is_past = ref_view_number < cur_view;
                    let len = handle
                        .get_replica_receiver_channel_len(ref_view_number)
                        .await;

                    let queue_state = get_queue_len(is_past, len);
                    match queue_state {
                        QueuedMessageTense::Past(Some(len)) => {
                            result = Err(ConsensusTestError::ConsensusSafetyFailed {
                                                    description: format!("Node {node_id}'s past view's replica receiver channel still exists for {ref_view_number:?} with {len} items in it.  We are currenltly in {cur_view:?}")});
                        }
                        QueuedMessageTense::Future(None) => {
                            result = Err(ConsensusTestError::ConsensusSafetyFailed {
                                description: format!("Replica did not properly queue future proposal for {ref_view_number:?}.  We are currently in {cur_view:?}")});
                        }
                        _ => {}
                    }
                }
            }
    }
        result
    }
    .boxed_local()
}

/// Submits proposals for 0..NUM_VIEWS rounds where `node_id` is the leader
fn test_validating_proposal_queueing_round_setup<
    'a,
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    I: TestableNodeImplementation<TYPES, Leaf = ValidatingLeaf<TYPES>>,
>(
    runner: &'a mut AppliedValidatingTestRunner<TYPES, I>,
    _ctx: &'a RoundCtx<TYPES, I>,
) -> LocalBoxFuture<'a, Vec<TYPES::Transaction>>
where
    I::QuorumExchange: QuorumExchangeType<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
        Proposal = ValidatingProposal<TYPES, I::Leaf>,
        Certificate = QuorumCertificate<TYPES, I::Leaf>,
        Vote = QuorumVote<TYPES, I::Leaf>,
    >,
{
    async move {
        let node_id = DEFAULT_NODE_ID;

        for i in 0..NUM_VIEWS {
            if is_upcoming_validating_leader(runner, node_id, TYPES::Time::new(i)).await {
                submit_validating_proposal(runner, node_id, TYPES::Time::new(i)).await;
            }
        }

        Vec::new()
    }
    .boxed_local()
}

/// Submits proposals for views where `node_id` is not the leader, and submits multiple proposals for views where `node_id` is the leader
fn test_bad_validating_proposal_round_setup<
    'a,
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    I: TestableNodeImplementation<TYPES, Leaf = ValidatingLeaf<TYPES>>,
>(
    runner: &'a mut AppliedValidatingTestRunner<TYPES, I>,
    _ctx: &'a RoundCtx<TYPES, I>,
) -> LocalBoxFuture<'a, Vec<TYPES::Transaction>>
where
    I::QuorumExchange: QuorumExchangeType<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
        Proposal = ValidatingProposal<TYPES, I::Leaf>,
        Certificate = QuorumCertificate<TYPES, I::Leaf>,
        Vote = QuorumVote<TYPES, I::Leaf>,
    >,
{
    async move {
        let node_id = DEFAULT_NODE_ID;
        for i in 0..NUM_VIEWS {
            if !is_upcoming_validating_leader(runner, node_id, TYPES::Time::new(i)).await {
                submit_validating_proposal(runner, node_id, TYPES::Time::new(i)).await;
            } else {
                submit_validating_proposal(runner, node_id, TYPES::Time::new(i)).await;
                submit_validating_proposal(runner, node_id, TYPES::Time::new(i)).await;
                submit_validating_proposal(runner, node_id, TYPES::Time::new(i)).await;
            }
        }
        Vec::new()
    }
    .boxed_local()
}

/// Checks nodes do not queue bad proposal messages
fn test_bad_validating_proposal_post_safety_check<
    'a,
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    I: TestableNodeImplementation<TYPES, Leaf = ValidatingLeaf<TYPES>>,
>(
    runner: &'a AppliedValidatingTestRunner<TYPES, I>,
    _ctx: &'a mut RoundCtx<TYPES, I>,
    _results: RoundResult<TYPES, ValidatingLeaf<TYPES>>,
) -> LocalBoxFuture<'a, Result<(), ConsensusTestError>>
where
    I::QuorumExchange: QuorumExchangeType<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
        Proposal = ValidatingProposal<TYPES, I::Leaf>,
        Certificate = QuorumCertificate<TYPES, I::Leaf>,
        Vote = QuorumVote<TYPES, I::Leaf>,
    >,
{
    async move {
        let node_id = DEFAULT_NODE_ID;
        let mut result = Ok(());

        for node in runner.nodes() {

            let handle = node;
            let cur_view = handle.get_current_view().await;

            for i in 0..NUM_VIEWS {
                let is_upcoming_validating_leader = is_upcoming_validating_leader(runner, node_id, TYPES::Time::new(i)).await;
                let ref_view_number = TYPES::Time::new(i);
                let is_past = ref_view_number < cur_view;
                let len = handle
                    .get_replica_receiver_channel_len(ref_view_number)
                    .await;

                let queue_state = get_queue_len(is_past, len);
                match queue_state {
                    QueuedMessageTense::Past(Some(len)) => {
                        result = Err(ConsensusTestError::ConsensusSafetyFailed {
                            description: format!("Past view's replica receiver channel still exists for {ref_view_number:?} with {len} items in it.  We are currently in {cur_view:?}")});
                    }
                    QueuedMessageTense::Future(Some(len)) => {
                        if !is_upcoming_validating_leader && ref_view_number != cur_view {
                            result = Err(ConsensusTestError::ConsensusSafetyFailed {
                                description: format!("Replica queued invalid Proposal message that was not sent from the leader for {ref_view_number:?}.  We are currently in {cur_view:?}")});
                        }
                        else if len > 1 {
                            result = Err(ConsensusTestError::ConsensusSafetyFailed {
                                description: format!("Replica queued too many Proposal messages for {ref_view_number:?}.  We are currently in {cur_view:?}")});

                        }
                    }
                    _ => {}
                }
            }
    }
        result
    }
    .boxed_local()
}

/// Tests that replicas receive and queue valid Proposal messages properly
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn test_validating_proposal_queueing() {
    let num_rounds = 10;
    let builder: TestBuilder<VrfTestTypes, StandardNodeImplType> = TestBuilder {
        metadata: TestMetadata {
            total_nodes: 4,
            start_nodes: 4,
            num_succeeds: num_rounds,
            failure_threshold: 0,
            ..TestMetadata::default()
        },
        over_ride: Some(RoundBuilder {
            hooks: Vec::new(),
            setup: Either::Left(RoundSetup(Arc::new(
                test_validating_proposal_queueing_round_setup,
            ))),
            check: Either::Left(RoundSafetyCheck(Arc::new(
                test_validating_proposal_queueing_post_safety_check,
            ))),
        }),
    };
    builder.build().launch().run_test().await.unwrap();
}

/// Tests that next leaders receive and queue valid vote messages properly
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn test_vote_queueing() {
    let num_rounds = 10;
    let setup = Left(RoundSetup(Arc::new(
        test_validating_vote_queueing_round_setup,
    )));
    let check = Left(RoundSafetyCheck(Arc::new(
        test_validating_vote_queueing_post_safety_check,
    )));
    let builder: TestBuilder<VrfTestTypes, StandardNodeImplType> = TestBuilder {
        metadata: TestMetadata {
            total_nodes: 4,
            start_nodes: 4,
            num_succeeds: num_rounds,
            failure_threshold: 0,
            ..TestMetadata::default()
        },
        over_ride: Some(RoundBuilder {
            setup,
            check,
            hooks: Vec::new(),
        }),
    };

    builder.build().launch().run_test().await.unwrap();
}

/// Tests that replicas handle bad Proposal messages properly
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn test_bad_proposal() {
    let num_rounds = 10;
    let setup = Left(RoundSetup(Arc::new(
        test_bad_validating_proposal_round_setup,
    )));
    let check = Left(RoundSafetyCheck(Arc::new(
        test_bad_validating_proposal_post_safety_check::<VrfTestTypes, StandardNodeImplType>,
    )));
    let builder: TestBuilder<VrfTestTypes, StandardNodeImplType> = TestBuilder {
        metadata: TestMetadata {
            total_nodes: 4,
            start_nodes: 4,
            num_succeeds: num_rounds,
            failure_threshold: 0,
            ..TestMetadata::default()
        },
        over_ride: Some(RoundBuilder {
            setup,
            check,
            hooks: vec![],
        }),
    };
    builder.build().launch().run_test().await.unwrap();
}

/// Tests a single node network, which also tests when a node is leader in consecutive views
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn test_single_node_network() {
    let num_rounds = 100;
    let builder: TestBuilder<StaticCommitteeTestTypes, StaticNodeImplType> = TestBuilder {
        metadata: TestMetadata {
            total_nodes: 1,
            start_nodes: 1,
            num_succeeds: num_rounds,
            failure_threshold: 0,
            ..TestMetadata::default()
        },
        over_ride: None,
    };
    builder.build().launch().run_test().await.unwrap();
}

/// Tests that min propose time works as expected
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn test_min_propose() {
    let num_rounds = 5;
    let propose_min_round_time = Duration::new(1, 0);
    let propose_max_round_time = Duration::new(5, 0);
    let builder: TestBuilder<VrfTestTypes, StandardNodeImplType> = TestBuilder {
        metadata: TestMetadata {
            total_nodes: 5,
            start_nodes: 5,
            num_succeeds: num_rounds,
            failure_threshold: 0,
            timing_data: TimingData {
                propose_min_round_time,
                propose_max_round_time,
                next_view_timeout: 10000,
                ..TimingData::default()
            },
            ..TestMetadata::default()
        },
        over_ride: None,
    };
    let start_time = Instant::now();
    builder.build().launch().run_test().await.unwrap();
    let duration = Instant::now() - start_time;
    let min_duration = num_rounds as u128 * propose_min_round_time.as_millis();
    let max_duration = num_rounds as u128 * propose_max_round_time.as_millis();

    assert!(duration.as_millis() >= min_duration);
    // Since we are submitting transactions each round, we should never hit the max round timeout
    assert!(duration.as_millis() < max_duration);
}

/// Tests that max propose time works as expected
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn test_max_propose() {
    let num_rounds = 5;
    let propose_min_round_time = Duration::new(0, 0);
    let propose_max_round_time = Duration::new(1, 0);
    let min_transactions: usize = 10;
    let builder: TestBuilder<VrfTestTypes, StandardNodeImplType> = TestBuilder {
        metadata: TestMetadata {
            total_nodes: 5,
            start_nodes: 5,
            num_succeeds: num_rounds,
            failure_threshold: 0,
            min_transactions,
            timing_data: TimingData {
                propose_min_round_time,
                propose_max_round_time,
                next_view_timeout: 10000,
                ..TimingData::default()
            },
            ..TestMetadata::default()
        },
        over_ride: None,
    };
    let start_time = Instant::now();
    builder.build().launch().run_test().await.unwrap();
    let duration = Instant::now() - start_time;
    let max_duration = num_rounds as u128 * propose_max_round_time.as_millis();
    // Since we are not submitting enough transactions, we should hit the max timeout every round
    assert!(duration.as_millis() > max_duration);
}

/// Tests that the chain heights are sequential
// #[cfg_attr(
//     feature = "tokio-executor",
//     tokio::test(flavor = "multi_thread", worker_threads = 2)
// )]
// #[cfg_attr(feature = "async-std-executor", async_std::test)]
// #[instrument]
// async fn test_chain_height() {
//     let _num_rounds = 10;

// Only start a subset of the nodes, ensuring there will be failed views so that height is
// different from view number.
// let total_nodes = 6;
// let start_nodes = 4;
//
// let mut test = TestMetadata {
//     total_nodes,
//     start_nodes,
//     num_succeeds: num_rounds,
//     failure_threshold: num_rounds,
//     ..Default::default()
// }
// .build::<StaticCommitteeTestTypes, StaticNodeImplType>();

// let heights = Arc::new(Mutex::new(vec![0; start_nodes]));
// for i in 0..num_rounds {
//     let heights = heights.clone();
//     test.round[i].safety_check_post = Some(Box::new(move |runner, _| {
//         async move {
//             let mut heights = heights.lock().await;
//             for (i, handle) in runner.nodes().enumerate() {
//                 let leaf = handle.get_decided_leaf().await;
//                 if leaf.justify_qc.is_genesis() {
//                     ensure!(
//                         leaf.get_height() == 0,
//                         SafetyFailedSnafu {
//                             description: format!(
//                                 "node {} has non-zero height {} for genesis leaf",
//                                 i,
//                                 leaf.get_height()
//                             ),
//                         }
//                     );
//                 } else {
//                     ensure!(
//                         leaf.get_height() == heights[i] + 1,
//                         SafetyFailedSnafu {
//                             description: format!(
//                                 "node {} has incorrect height {} for previous height {}",
//                                 i,
//                                 leaf.get_height(),
//                                 heights[i]
//                             ),
//                         }
//                     );
//                     heights[i] = leaf.get_height();
//                 }
//             }
//             Ok(())
//         }
//         .boxed_local()
//     }));
// }

// test.execute().await.unwrap();
// }

/// Tests that the leaf chains in decide events are always consistent.
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn test_decide_leaf_chain() {
    let mut round = Round::<StaticCommitteeTestTypes, StaticNodeImplType>::default();

    // Collection of (handle, leaf) pairs collected at the start of the round. The leaf is the
    // last decided leaf before the round, so that after the round we can check that the new
    // leaf chain extends from it. The handle must be copied out of the round runner before the
    // round starts so that it will buffer events emitted during the round.
    #[allow(clippy::type_complexity)]
    let handles: Arc<
        Mutex<
            Vec<(
                HotShotHandle<StaticCommitteeTestTypes, StaticNodeImplType>,
                <StaticNodeImplType as NodeImplementation<StaticCommitteeTestTypes>>::Leaf,
            )>,
        >,
    > = Arc::new(Mutex::new(vec![]));

    // Initialize `handles` at the start of the round.
    {
        let handles = Arc::new(Mutex::new(vec![]));
        round.hooks = vec![RoundHook(Arc::new(move |runner, _ctx| {
            let handles = handles.clone();
            async move {
                *handles.lock().await = join_all(
                    runner
                        .nodes()
                        .map(|handle| async { (handle.clone(), handle.get_decided_leaf().await) }),
                )
                .await;
                Ok(())
            }
            .boxed_local()
        }))];
    }
    round.safety_check = RoundSafetyCheck(Arc::new(move |_, _ctx, _| {
        let handles = handles.clone();
        async move {
            for (mut handle, last_leaf) in std::mem::take(&mut *handles.lock().await) {
                // Get the decide event from this round.
                let (leaf_chain, qc) = loop {
                    let event = handle
                        .try_next_event()
                        .map_err(|_| {
                            ConsensusSafetyFailedSnafu {
                                description: "HotShot shut down",
                            }
                            .build()
                        })?
                        .context(ConsensusSafetyFailedSnafu {
                            description: "round did not produce a Decide or ViewFinished event",
                        })?;
                    match event.event {
                        EventType::Decide { leaf_chain, qc } => break (leaf_chain, qc),
                        EventType::ViewFinished { view_number } => {
                            tracing::warn!(
                                "round {:?} did not produce a decide, skipping safety check",
                                view_number
                            );
                            return Ok(());
                        }
                        _ => continue,
                    }
                };
                tracing::info!("got decide {:?} {:?}", qc, leaf_chain);

                // Starting from `qc` and continuing with the `justify_qc` of each leaf in the
                // chain, the chain of QCs should justify the chain of leaves.
                let qcs = once(&*qc).chain(leaf_chain.iter().map(|leaf| &leaf.justify_qc));
                // The new leaf chain should extend from the previously decided leaf.
                let leaves = leaf_chain.iter().chain(once(&last_leaf));
                for (i, (qc, leaf)) in qcs.zip(leaves).enumerate() {
                    if qc.is_genesis() {
                        tracing::warn!("skipping validation of genesis QC");
                        continue;
                    }
                    ensure!(
                        qc.leaf_commitment() == leaf.commit(),
                        ConsensusSafetyFailedSnafu {
                            description: format!(
                                "QC {}/{} justifies {}, but the parent leaf is {}",
                                i + 1,
                                leaf_chain.len() + 1,
                                qc.leaf_commitment(),
                                leaf.commit()
                            ),
                        }
                    );
                    //TODO (da) QC doesn't have block_commitment anymore. Should we check some
                    // other field?
                    // ensure!(
                    //     qc.block_commitment == leaf.deltas.commit(),
                    //     SafetyFailedSnafu {
                    //         description: format!(
                    //             "QC {}/{} justifies block {}, but parent leaf has block {}",
                    //             i + 1,
                    //             leaf_chain.len() + 1,
                    //             qc.leaf_commitment,
                    //             leaf.commit()
                    //         ),
                    //     }
                    // );
                }
            }

            Ok(())
        }
        .boxed_local()
    }));
    let builder = TestBuilder::<StaticCommitteeTestTypes, StaticNodeImplType> {
        metadata: TestMetadata {
            num_succeeds: 10,
            failure_threshold: 0,
            ..Default::default()
        },
        over_ride: Some(RoundBuilder::default()),
    };

    builder.build().launch().run_test().await.unwrap();
}
