mod common;

use ark_bls12_381::Parameters as Param381;
use async_lock::Mutex;
use blake3::Hasher;
use commit::{Commitment, Committable};
use common::{
    AppliedTestNodeImpl, AppliedTestRunner, DetailedTestDescriptionBuilder,
    GeneralTestDescriptionBuilder, StandardNodeImplType, StaticCommitteeTestTypes,
    StaticNodeImplType, VrfTestTypes,
};
use either::Right;
use futures::{
    future::{join_all, LocalBoxFuture},
    FutureExt,
};
use hotshot::{
    demos::dentry::random_leaf,
    tasks::{TaskHandler, TaskHandlerType},
    traits::election::vrf::{VRFStakeTableConfig, VrfImpl},
    types::Vote,
};
use hotshot_testing::{ConsensusRoundError, RoundResult, SafetyFailedSnafu};
use hotshot_types::{
    data::{LeafType, ProposalType, ValidatingLeaf, ValidatingProposal},
    event::EventType,
    message::{ConsensusMessage, Proposal},
    traits::{
        election::{Election, TestableElection},
        node_implementation::NodeTypes,
        signature_key::TestableSignatureKey,
        state::{ConsensusTime, TestableBlock, TestableState, ValidatingConsensus},
    },
};
use jf_primitives::{signatures::BLSSignatureScheme, vrf::blsvrf::BLSVRFScheme};
use snafu::{ensure, OptionExt};
use std::iter::once;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tracing::{instrument, warn};

const NUM_VIEWS: u64 = 100;
const DEFAULT_NODE_ID: u64 = 0;

enum QueuedMessageTense {
    Past(Option<usize>),
    Future(Option<usize>),
}

// TODO (da) rename to submit_validating_proposal
/// Returns true if `node_id` is the leader of `view_number`
async fn is_upcoming_leader<
    TYPES: NodeTypes<ConsensusType = ValidatingConsensus>,
    ELECTION: Election<TYPES, LeafType = ValidatingLeaf<TYPES>>,
>(
    runner: &AppliedTestRunner<
        TYPES,
        ValidatingLeaf<TYPES>,
        ValidatingProposal<TYPES, ELECTION>,
        ELECTION,
    >,
    node_id: u64,
    view_number: TYPES::Time,
) -> bool
where
    TYPES::SignatureKey: TestableSignatureKey,
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType>,
    TaskHandler<<TYPES as NodeTypes>::ConsensusType>: TaskHandlerType<
        TYPES,
        AppliedTestNodeImpl<
            TYPES,
            ValidatingLeaf<TYPES>,
            ValidatingProposal<TYPES, ELECTION>,
            ELECTION,
        >,
    >,
{
    let handle = runner.get_handle(node_id).unwrap();
    let leader = handle.get_leader(view_number).await;
    leader == handle.get_public_key()
}

// TODO (da) rename to submit_validating_proposal
/// Builds and submits a random proposal for the specified view number
async fn submit_proposal<
    TYPES: NodeTypes<ConsensusType = ValidatingConsensus>,
    ELECTION: Election<TYPES, LeafType = ValidatingLeaf<TYPES>>,
>(
    runner: &AppliedTestRunner<
        TYPES,
        ValidatingLeaf<TYPES>,
        ValidatingProposal<TYPES, ELECTION>,
        ELECTION,
    >,
    sender_node_id: u64,
    view_number: TYPES::Time,
) where
    TYPES::SignatureKey: TestableSignatureKey,
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType>,
    TaskHandler<<TYPES as NodeTypes>::ConsensusType>: TaskHandlerType<
        TYPES,
        AppliedTestNodeImpl<
            TYPES,
            ValidatingLeaf<TYPES>,
            ValidatingProposal<TYPES, ELECTION>,
            ELECTION,
        >,
    >,
{
    let mut rng = rand::thread_rng();
    let handle = runner.get_handle(sender_node_id).unwrap();

    // Build proposal
    let mut leaf = random_leaf(TYPES::BlockType::genesis(), &mut rng);
    leaf.view_number = view_number;
    leaf.set_height(handle.get_decided_leaf().await.get_height() + 1);
    let signature = handle.sign_proposal(&leaf.commit(), leaf.view_number);
    let msg = ConsensusMessage::Proposal(Proposal {
        leaf: leaf.into(),
        signature,
    });

    // Send proposal
    handle.send_broadcast_consensus_message(msg.clone()).await;
}

// TODO (da) rename to submit_validating_vote
/// Builds and submits a random vote for the specified view number from the specified node
async fn submit_vote<
    TYPES: NodeTypes<ConsensusType = ValidatingConsensus>,
    ELECTION: Election<TYPES, LeafType = ValidatingLeaf<TYPES>> + TestableElection<TYPES>,
>(
    runner: &AppliedTestRunner<
        TYPES,
        ValidatingLeaf<TYPES>,
        ValidatingProposal<TYPES, ELECTION>,
        ELECTION,
    >,
    sender_node_id: u64,
    view_number: TYPES::Time,
    recipient_node_id: u64,
) where
    TYPES::SignatureKey: TestableSignatureKey,
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType>,
    TaskHandler<<TYPES as NodeTypes>::ConsensusType>: TaskHandlerType<
        TYPES,
        AppliedTestNodeImpl<
            TYPES,
            ValidatingLeaf<TYPES>,
            ValidatingProposal<TYPES, ELECTION>,
            ELECTION,
        >,
    >,
{
    let mut rng = rand::thread_rng();
    let handle = runner.get_handle(sender_node_id).unwrap();

    // Build vote
    let mut leaf = random_leaf(TYPES::BlockType::genesis(), &mut rng);
    leaf.view_number = view_number;
    let signature = handle.sign_vote(&leaf.commit(), leaf.view_number);
    let msg = ConsensusMessage::Vote(Vote {
        signature,
        justify_qc_commitment: leaf.justify_qc.commit(),
        current_view: leaf.view_number,
        block_commitment: leaf.deltas.commit(),
        leaf_commitment: leaf.commit(),
        // TODO placeholder below
        vote_token: ELECTION::generate_test_vote_token(),
    });

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

// TODO (da) rename to test_validating_vote_queueing_post_safety_check
/// Checks that votes are queued correctly for views 1..NUM_VIEWS
fn test_vote_queueing_post_safety_check<
    TYPES: NodeTypes<ConsensusType = ValidatingConsensus>,
    ELECTION: Election<TYPES, LeafType = ValidatingLeaf<TYPES>>,
>(
    runner: &AppliedTestRunner<
        TYPES,
        ValidatingLeaf<TYPES>,
        ValidatingProposal<TYPES, ELECTION>,
        ELECTION,
    >,
    _results: RoundResult<TYPES, ValidatingLeaf<TYPES>>,
) -> LocalBoxFuture<Result<(), ConsensusRoundError>>
where
    TYPES::SignatureKey: TestableSignatureKey,
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType>,
    TaskHandler<<TYPES as NodeTypes>::ConsensusType>: TaskHandlerType<
        TYPES,
        AppliedTestNodeImpl<
            TYPES,
            ValidatingLeaf<TYPES>,
            ValidatingProposal<TYPES, ELECTION>,
            ELECTION,
        >,
    >,
{
    async move {
        let node_id = DEFAULT_NODE_ID;
        let mut result = Ok(());

        let handle = runner.get_handle(node_id).unwrap();
        let cur_view = handle.get_current_view().await;


        for i in 1..NUM_VIEWS {
            if is_upcoming_leader(runner, node_id, TYPES::Time::new(i)).await {
                let ref_view_number = TYPES::Time::new(i - 1);
                let is_past = ref_view_number < cur_view;
                let len = handle
                    .get_next_leader_receiver_channel_len(ref_view_number)
                    .await;

                let queue_state = get_queue_len(is_past, len);
                match queue_state {
                    QueuedMessageTense::Past(Some(len)) => {
                        result = Err(ConsensusRoundError::SafetyFailed {
                                                description: format!("Past view's next leader receiver channel for node {} still exists for {:?} with {} items in it.  We are currenltly in {:?}", node_id, ref_view_number, len, cur_view)});
                    }
                    QueuedMessageTense::Future(None) => {
                        result = Err(ConsensusRoundError::SafetyFailed {
                            description: format!("Next Leader did not properly queue future vote for {:?}.  We are currently in {:?}", ref_view_number, cur_view)});
                    }
                    _ => {}
                }
            }
    }
        result
    }
    .boxed_local()
}

// TODO (da) rename to test_validating_vote_queueing_round_setup
/// For 1..NUM_VIEWS submit votes to each node in the network
fn test_vote_queueing_round_setup<
    TYPES: NodeTypes<ConsensusType = ValidatingConsensus>,
    ELECTION: Election<TYPES, LeafType = ValidatingLeaf<TYPES>> + TestableElection<TYPES>,
>(
    runner: &mut AppliedTestRunner<
        TYPES,
        ValidatingLeaf<TYPES>,
        ValidatingProposal<TYPES, ELECTION>,
        ELECTION,
    >,
) -> LocalBoxFuture<Vec<TYPES::Transaction>>
where
    TYPES::SignatureKey: TestableSignatureKey,
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType>,
    TaskHandler<<TYPES as NodeTypes>::ConsensusType>: TaskHandlerType<
        TYPES,
        AppliedTestNodeImpl<
            TYPES,
            ValidatingLeaf<TYPES>,
            ValidatingProposal<TYPES, ELECTION>,
            ELECTION,
        >,
    >,
{
    async move {
        let node_id = DEFAULT_NODE_ID;
        for j in runner.ids() {
            for i in 1..NUM_VIEWS {
                if is_upcoming_leader(runner, node_id, TYPES::Time::new(i)).await {
                    submit_vote(runner, j, TYPES::Time::new(i - 1), node_id).await;
                }
            }
        }
        Vec::new()
    }
    .boxed_local()
}

// TODO (da) rename to test_validaitng_proposal_queueing_post_safety_check
/// Checks views 0..NUM_VIEWS for whether proposal messages are properly queued
fn test_proposal_queueing_post_safety_check<
    TYPES: NodeTypes<ConsensusType = ValidatingConsensus>,
    ELECTION: Election<TYPES, LeafType = ValidatingLeaf<TYPES>>,
>(
    runner: &AppliedTestRunner<
        TYPES,
        ValidatingLeaf<TYPES>,
        ValidatingProposal<TYPES, ELECTION>,
        ELECTION,
    >,
    _results: RoundResult<TYPES, ValidatingLeaf<TYPES>>,
) -> LocalBoxFuture<Result<(), ConsensusRoundError>>
where
    TYPES::SignatureKey: TestableSignatureKey,
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType>,
    TaskHandler<<TYPES as NodeTypes>::ConsensusType>: TaskHandlerType<
        TYPES,
        AppliedTestNodeImpl<
            TYPES,
            ValidatingLeaf<TYPES>,
            ValidatingProposal<TYPES, ELECTION>,
            ELECTION,
        >,
    >,
{
    async move {
        let node_id = DEFAULT_NODE_ID;
        let mut result = Ok(());

        for node in runner.nodes() {

            let handle = node;
            let cur_view = handle.get_current_view().await;

            for i in 0..NUM_VIEWS {
                if is_upcoming_leader(runner, node_id, TYPES::Time::new(i)).await {
                    let ref_view_number = TYPES::Time::new(i);
                    let is_past = ref_view_number < cur_view;
                    let len = handle
                        .get_replica_receiver_channel_len(ref_view_number)
                        .await;

                    let queue_state = get_queue_len(is_past, len);
                    match queue_state {
                        QueuedMessageTense::Past(Some(len)) => {
                            result = Err(ConsensusRoundError::SafetyFailed {
                                                    description: format!("Node {}'s past view's replica receiver channel still exists for {:?} with {} items in it.  We are currenltly in {:?}", node_id, ref_view_number, len, cur_view)});
                        }
                        QueuedMessageTense::Future(None) => {
                            result = Err(ConsensusRoundError::SafetyFailed {
                                description: format!("Replica did not properly queue future proposal for {:?}.  We are currently in {:?}", ref_view_number, cur_view)});
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

// TODO (da) rename to test_validating_proposal_queueing_round_setup
/// Submits proposals for 0..NUM_VIEWS rounds where `node_id` is the leader
fn test_proposal_queueing_round_setup<
    TYPES: NodeTypes<ConsensusType = ValidatingConsensus>,
    ELECTION: Election<TYPES, LeafType = ValidatingLeaf<TYPES>>,
>(
    runner: &mut AppliedTestRunner<
        TYPES,
        ValidatingLeaf<TYPES>,
        ValidatingProposal<TYPES, ELECTION>,
        ELECTION,
    >,
) -> LocalBoxFuture<Vec<TYPES::Transaction>>
where
    TYPES::SignatureKey: TestableSignatureKey,
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType>,
    TaskHandler<<TYPES as NodeTypes>::ConsensusType>: TaskHandlerType<
        TYPES,
        AppliedTestNodeImpl<
            TYPES,
            ValidatingLeaf<TYPES>,
            ValidatingProposal<TYPES, ELECTION>,
            ELECTION,
        >,
    >,
{
    async move {
        let node_id = DEFAULT_NODE_ID;

        for i in 0..NUM_VIEWS {
            if is_upcoming_leader(runner, node_id, TYPES::Time::new(i)).await {
                submit_proposal(runner, node_id, TYPES::Time::new(i)).await;
            }
        }

        Vec::new()
    }
    .boxed_local()
}

// TODO (da) rename to test_bad_validating_proposal_round_setup
/// Submits proposals for views where `node_id` is not the leader, and submits multiple proposals for views where `node_id` is the leader
fn test_bad_proposal_round_setup<
    TYPES: NodeTypes<ConsensusType = ValidatingConsensus>,
    ELECTION: Election<TYPES, LeafType = ValidatingLeaf<TYPES>>,
>(
    runner: &mut AppliedTestRunner<
        TYPES,
        ValidatingLeaf<TYPES>,
        ValidatingProposal<TYPES, ELECTION>,
        ELECTION,
    >,
) -> LocalBoxFuture<Vec<TYPES::Transaction>>
where
    TYPES::SignatureKey: TestableSignatureKey,
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType>,
    TaskHandler<<TYPES as NodeTypes>::ConsensusType>: TaskHandlerType<
        TYPES,
        AppliedTestNodeImpl<
            TYPES,
            ValidatingLeaf<TYPES>,
            ValidatingProposal<TYPES, ELECTION>,
            ELECTION,
        >,
    >,
{
    async move {
        let node_id = DEFAULT_NODE_ID;
        for i in 0..NUM_VIEWS {
            if !is_upcoming_leader(runner, node_id, TYPES::Time::new(i)).await {
                submit_proposal(runner, node_id, TYPES::Time::new(i)).await;
            } else {
                submit_proposal(runner, node_id, TYPES::Time::new(i)).await;
                submit_proposal(runner, node_id, TYPES::Time::new(i)).await;
                submit_proposal(runner, node_id, TYPES::Time::new(i)).await;
            }
        }
        Vec::new()
    }
    .boxed_local()
}

// TODO (da) rename to test_bad_validatingproposal_post_safety_check
/// Checks nodes do not queue bad proposal messages
fn test_bad_proposal_post_safety_check<
    TYPES: NodeTypes<ConsensusType = ValidatingConsensus>,
    ELECTION: Election<TYPES, LeafType = ValidatingLeaf<TYPES>>,
>(
    runner: &AppliedTestRunner<
        TYPES,
        ValidatingLeaf<TYPES>,
        ValidatingProposal<TYPES, ELECTION>,
        ELECTION,
    >,
    _results: RoundResult<TYPES, ValidatingLeaf<TYPES>>,
) -> LocalBoxFuture<Result<(), ConsensusRoundError>>
where
    TYPES::SignatureKey: TestableSignatureKey,
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType>,
    TaskHandler<<TYPES as NodeTypes>::ConsensusType>: TaskHandlerType<
        TYPES,
        AppliedTestNodeImpl<
            TYPES,
            ValidatingLeaf<TYPES>,
            ValidatingProposal<TYPES, ELECTION>,
            ELECTION,
        >,
    >,
{
    async move {
        let node_id = DEFAULT_NODE_ID;
        let mut result = Ok(());

        for node in runner.nodes() {

            let handle = node;
            let cur_view = handle.get_current_view().await;

            for i in 0..NUM_VIEWS {
                let is_upcoming_leader = is_upcoming_leader(runner, node_id, TYPES::Time::new(i)).await;
                let ref_view_number = TYPES::Time::new(i);
                let is_past = ref_view_number < cur_view;
                let len = handle
                    .get_replica_receiver_channel_len(ref_view_number)
                    .await;

                let queue_state = get_queue_len(is_past, len);
                match queue_state {
                    QueuedMessageTense::Past(Some(len)) => {
                        result = Err(ConsensusRoundError::SafetyFailed {
                            description: format!("Past view's replica receiver channel still exists for {:?} with {} items in it.  We are currenltly in {:?}", ref_view_number, len, cur_view)});
                    }
                    QueuedMessageTense::Future(Some(len)) => {
                        if !is_upcoming_leader && ref_view_number != cur_view {
                        result = Err(ConsensusRoundError::SafetyFailed {
                            description: format!("Replica queued invalid Proposal message that was not sent from the leader for {:?}.  We are currently in {:?}", ref_view_number, cur_view)});
                    }
                    else if len > 1 {
                        result = Err(ConsensusRoundError::SafetyFailed {
                            description: format!("Replica queued too many Proposal messages for {:?}.  We are currently in {:?}", ref_view_number, cur_view)});

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

// TODO (da) rename to test_bad_validating_ote_round_setup
/// Submits votes to non-leaders and submits too many votes from a singular node
fn test_bad_vote_round_setup<
    TYPES: NodeTypes<ConsensusType = ValidatingConsensus>,
    ELECTION: Election<TYPES, LeafType = ValidatingLeaf<TYPES>> + TestableElection<TYPES>,
>(
    runner: &mut AppliedTestRunner<
        TYPES,
        ValidatingLeaf<TYPES>,
        ValidatingProposal<TYPES, ELECTION>,
        ELECTION,
    >,
) -> LocalBoxFuture<Vec<TYPES::Transaction>>
where
    TYPES::SignatureKey: TestableSignatureKey,
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType>,
    TaskHandler<<TYPES as NodeTypes>::ConsensusType>: TaskHandlerType<
        TYPES,
        AppliedTestNodeImpl<
            TYPES,
            ValidatingLeaf<TYPES>,
            ValidatingProposal<TYPES, ELECTION>,
            ELECTION,
        >,
    >,
{
    async move {
        let node_id = DEFAULT_NODE_ID;
        for j in runner.ids() {
            for i in 1..NUM_VIEWS {
                submit_vote(runner, j, TYPES::Time::new(i - 1), node_id).await;
            }
        }
        Vec::new()
    }
    .boxed_local()
}

// TODO (da) rename to test_bad_validating_vote_post_safety_check
/// Checks that non-leaders do not queue votes, and that leaders do not queue more than 1 vote per node
fn test_bad_vote_post_safety_check<
    TYPES: NodeTypes<ConsensusType = ValidatingConsensus>,
    ELECTION: Election<TYPES, LeafType = ValidatingLeaf<TYPES>>,
>(
    runner: &AppliedTestRunner<
        TYPES,
        ValidatingLeaf<TYPES>,
        ValidatingProposal<TYPES, ELECTION>,
        ELECTION,
    >,
    _results: RoundResult<TYPES, ValidatingLeaf<TYPES>>,
) -> LocalBoxFuture<Result<(), ConsensusRoundError>>
where
    TYPES::SignatureKey: TestableSignatureKey,
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType>,
    TaskHandler<<TYPES as NodeTypes>::ConsensusType>: TaskHandlerType<
        TYPES,
        AppliedTestNodeImpl<
            TYPES,
            ValidatingLeaf<TYPES>,
            ValidatingProposal<TYPES, ELECTION>,
            ELECTION,
        >,
    >,
{
    async move {
        let node_id = DEFAULT_NODE_ID;
        let mut result = Ok(());
        let handle = runner.get_handle(node_id).unwrap();
        let cur_view = handle.get_current_view().await;

        for i in 1..NUM_VIEWS {
            let is_upcoming_leader = is_upcoming_leader(runner, node_id, TYPES::Time::new(i)).await;
            let ref_view_number = TYPES::Time::new(i - 1);
            let is_past = ref_view_number < cur_view;
            let len = handle
                .get_next_leader_receiver_channel_len(ref_view_number)
                .await;

            let state = get_queue_len(is_past, len);
            match state {
                QueuedMessageTense::Past(Some(len)) => {
                    result = Err(ConsensusRoundError::SafetyFailed {
                        description: format!("Past view's next leader receiver channel still exists for {:?} with {} items in it.  We are currenltly in {:?}", ref_view_number, len, cur_view)});
                }
                QueuedMessageTense::Future(Some(len)) => {
                    if !is_upcoming_leader {
                    result = Err(ConsensusRoundError::SafetyFailed {
                        description: format!("Non-leader queued invalid vote message for {:?}.  We are currently in {:?}", ref_view_number, cur_view)                                            });
                }
                else if len > runner.ids().len() {
                    result = Err(ConsensusRoundError::SafetyFailed {
                        description: format!("Next leader queued too many vote messages for {:?}.  We are currently in {:?}", ref_view_number, cur_view)                                            });
                    // Assert here to fail the test without failed rounds
                    assert!(len <= runner.ids().len());
                }
            }
                _ => {}
            }
        }
        result
    }
    .boxed_local()
}

// TODO (da) rename to test_validating_proposal_queueing
/// Tests that replicas receive and queue valid Proposal messages properly
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn test_proposal_queueing() {
    let num_rounds = 10;
    let description: DetailedTestDescriptionBuilder<VrfTestTypes, StandardNodeImplType> =
        DetailedTestDescriptionBuilder {
            general_info: GeneralTestDescriptionBuilder {
                total_nodes: 4,
                start_nodes: 4,
                num_succeeds: num_rounds,
                failure_threshold: 0,
                ..GeneralTestDescriptionBuilder::default()
            },
            rounds: None,
            gen_runner: None,
        };
    let mut test = description.build();

    for i in 0..num_rounds {
        test.rounds[i].setup_round = Some(Box::new(test_proposal_queueing_round_setup));
        test.rounds[i].safety_check_post = Some(Box::new(test_proposal_queueing_post_safety_check));
    }

    test.execute().await.unwrap();
}

/// Tests that next leaders receive and queue valid Vote messages properly
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn test_vote_queueing() {
    let num_rounds = 10;
    let description: DetailedTestDescriptionBuilder<VrfTestTypes, StandardNodeImplType> =
        DetailedTestDescriptionBuilder {
            general_info: GeneralTestDescriptionBuilder {
                total_nodes: 4,
                start_nodes: 4,
                num_succeeds: num_rounds,
                failure_threshold: 0,
                txn_ids: Right(1),
                ..GeneralTestDescriptionBuilder::default()
            },
            rounds: None,
            gen_runner: None,
        };
    let mut test = description.build();

    for i in 0..num_rounds {
        test.rounds[i].setup_round = Some(Box::new(test_vote_queueing_round_setup));
        test.rounds[i].safety_check_post = Some(Box::new(test_vote_queueing_post_safety_check));
    }

    test.execute().await.unwrap();
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
    let description: DetailedTestDescriptionBuilder<VrfTestTypes, StandardNodeImplType> =
        DetailedTestDescriptionBuilder {
            general_info: GeneralTestDescriptionBuilder {
                total_nodes: 4,
                start_nodes: 4,
                num_succeeds: num_rounds,
                failure_threshold: 0,
                ..GeneralTestDescriptionBuilder::default()
            },
            rounds: None,
            gen_runner: None,
        };
    let mut test = description.build();

    for i in 0..num_rounds {
        test.rounds[i].setup_round = Some(Box::new(test_bad_proposal_round_setup));
        test.rounds[i].safety_check_post = Some(Box::new(
            test_bad_proposal_post_safety_check::<
                VrfTestTypes,
                VrfImpl<
                    VrfTestTypes,
                    ValidatingLeaf<VrfTestTypes>,
                    BLSSignatureScheme<Param381>,
                    BLSVRFScheme<Param381>,
                    Hasher,
                    Param381,
                >,
            >,
        ));
    }

    test.execute().await.unwrap();
}

/// Tests that next leaders handle bad Votes properly.  We allow `num_rounds` of failures because replicas will not be able to come to consensus with the bad votes we submit to them
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn test_bad_vote() {
    let num_rounds = 10;
    let description: DetailedTestDescriptionBuilder<VrfTestTypes, StandardNodeImplType> =
        DetailedTestDescriptionBuilder {
            general_info: GeneralTestDescriptionBuilder {
                total_nodes: 4,
                start_nodes: 4,
                num_succeeds: num_rounds,
                failure_threshold: num_rounds,
                ..GeneralTestDescriptionBuilder::default()
            },
            rounds: None,
            gen_runner: None,
        };
    let mut test = description.build();

    for i in 0..num_rounds {
        test.rounds[i].setup_round = Some(Box::new(test_bad_vote_round_setup));
        test.rounds[i].safety_check_post = Some(Box::new(test_bad_vote_post_safety_check));
    }

    test.execute().await.unwrap();
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
    let description: DetailedTestDescriptionBuilder<StaticCommitteeTestTypes, StaticNodeImplType> =
        DetailedTestDescriptionBuilder {
            general_info: GeneralTestDescriptionBuilder {
                total_nodes: 1,
                start_nodes: 1,
                num_succeeds: num_rounds,
                failure_threshold: 0,
                txn_ids: Right(1),
                ..GeneralTestDescriptionBuilder::default()
            },
            rounds: None,
            gen_runner: None,
        };
    description.build().execute().await.unwrap();
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
    let description: DetailedTestDescriptionBuilder<VrfTestTypes, StandardNodeImplType> =
        DetailedTestDescriptionBuilder {
            general_info: GeneralTestDescriptionBuilder {
                total_nodes: 5,
                start_nodes: 5,
                num_succeeds: num_rounds,
                failure_threshold: 0,
                propose_min_round_time,
                propose_max_round_time,
                next_view_timeout: 10000,
                txn_ids: Right(10),
                ..GeneralTestDescriptionBuilder::default()
            },
            rounds: None,
            gen_runner: None,
        };
    let start_time = Instant::now();
    description.build().execute().await.unwrap();
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
    let description: DetailedTestDescriptionBuilder<VrfTestTypes, StandardNodeImplType> =
        DetailedTestDescriptionBuilder {
            general_info: GeneralTestDescriptionBuilder {
                total_nodes: 5,
                start_nodes: 5,
                num_succeeds: num_rounds,
                failure_threshold: 0,
                propose_min_round_time,
                propose_max_round_time,
                next_view_timeout: 10000,
                min_transactions,
                txn_ids: Right(1),
                ..GeneralTestDescriptionBuilder::default()
            },
            rounds: None,
            gen_runner: None,
        };
    let start_time = Instant::now();
    description.build().execute().await.unwrap();
    let duration = Instant::now() - start_time;
    let max_duration = num_rounds as u128 * propose_max_round_time.as_millis();
    // Since we are not submitting enough transactions, we should hit the max timeout every round
    assert!(duration.as_millis() > max_duration);
}

/// Tests that the chain heights are sequential
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn test_chain_height() {
    let num_rounds = 10;

    // Only start a subset of the nodes, ensuring there will be failed views so that height is
    // different from view number.
    let total_nodes = 6;
    let start_nodes = 4;

    let mut test = GeneralTestDescriptionBuilder {
        total_nodes,
        start_nodes,
        num_succeeds: num_rounds,
        failure_threshold: num_rounds,
        ..Default::default()
    }
    .build::<StaticCommitteeTestTypes, StaticNodeImplType>();

    let heights = Arc::new(Mutex::new(vec![0; start_nodes]));
    for i in 0..num_rounds {
        let heights = heights.clone();
        test.rounds[i].safety_check_post = Some(Box::new(move |runner, _| {
            async move {
                let mut heights = heights.lock().await;
                for (i, handle) in runner.nodes().enumerate() {
                    let leaf = handle.get_decided_leaf().await;
                    if leaf.justify_qc.genesis {
                        ensure!(
                            leaf.get_height() == 0,
                            SafetyFailedSnafu {
                                description: format!(
                                    "node {} has non-zero height {} for genesis leaf",
                                    i,
                                    leaf.get_height()
                                ),
                            }
                        );
                    } else {
                        ensure!(
                            leaf.get_height() == heights[i] + 1,
                            SafetyFailedSnafu {
                                description: format!(
                                    "node {} has incorrect height {} for previous height {}",
                                    i,
                                    leaf.get_height(),
                                    heights[i]
                                ),
                            }
                        );
                        heights[i] = leaf.get_height();
                    }
                }
                Ok(())
            }
            .boxed_local()
        }));
    }

    test.execute().await.unwrap();
}

/// Tests that the leaf chains in decide events are always consistent.
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn test_decide_leaf_chain() {
    let mut test = GeneralTestDescriptionBuilder {
        num_succeeds: 10,
        failure_threshold: 0,
        ..Default::default()
    }
    .build::<StaticCommitteeTestTypes, StaticNodeImplType>();
    for round in &mut test.rounds {
        // Collection of (handle, leaf) pairs collected at the start of the round. The leaf is the
        // last decided leaf before the round, so that after the round we can check that the new
        // leaf chain extends from it. The handle must be copied out of the round runner before the
        // round starts so that it will buffer events emitted during the round.
        let handles = Arc::new(Mutex::new(vec![]));

        // Initialize `handles` at the start of the round.
        {
            let handles = handles.clone();
            round.safety_check_pre = Some(Box::new(move |runner| {
                async move {
                    *handles.lock().await =
                        join_all(runner.nodes().map(|handle| async {
                            (handle.clone(), handle.get_decided_leaf().await)
                        }))
                        .await;
                    Ok(())
                }
                .boxed_local()
            }));
        }
        round.safety_check_post = Some(Box::new(move |_, _| {
            async move {
                for (mut handle, last_leaf) in std::mem::take(&mut *handles.lock().await) {
                    // Get the decide event from this round.
                    let (leaf_chain, qc) = loop {
                        let event = handle
                            .try_next_event()
                            .map_err(|_| {
                                SafetyFailedSnafu {
                                    description: "HotShot shut down",
                                }
                                .build()
                            })?
                            .context(SafetyFailedSnafu {
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
                        if qc.genesis {
                            tracing::warn!("skipping validation of genesis QC");
                            continue;
                        }
                        ensure!(
                            qc.leaf_commitment == leaf.commit(),
                            SafetyFailedSnafu {
                                description: format!(
                                    "QC {}/{} justifies {}, but the parent leaf is {}",
                                    i + 1,
                                    leaf_chain.len() + 1,
                                    qc.leaf_commitment,
                                    leaf.commit()
                                ),
                            }
                        );
                        //TODO (da) what field should be chacked?
                        // ensure!(
                        //     leaf.block_commitment == leaf.deltas.commit(),
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
    }
    test.execute().await.unwrap();
}
