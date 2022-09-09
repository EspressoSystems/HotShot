mod common;
use commit::Committable;
use common::{
    AppliedTestRunner, DetailedTestDescriptionBuilder, GeneralTestDescriptionBuilder, TestNetwork,
    TestRoundResult, TestTransaction,
};
use futures::{future::LocalBoxFuture, FutureExt};
use hotshot::{
    demos::dentry::{random_leaf, DEntryBlock, DEntryState},
    traits::implementations::MemoryStorage,
    types::Vote,
};
use hotshot_testing::ConsensusRoundError;
use hotshot_types::{
    data::ViewNumber,
    message::{ConsensusMessage, Proposal},
};
use hotshot_utils::async_std_or_tokio::async_test;
use tracing::{instrument, warn};

const NUM_VIEWS: u64 = 100;
const DEFAULT_NODE_ID: u64 = 0;

enum QueuedMessageTense {
    Past(Option<usize>),
    Future(Option<usize>),
}

/// Returns true if `node_id` is the leader of `view_number`
async fn is_upcoming_leader(
    runner: &AppliedTestRunner,
    node_id: u64,
    view_number: ViewNumber,
) -> bool {
    let handle = runner.get_handle(node_id).unwrap();
    let leader = handle.get_leader(view_number).await;
    leader == handle.get_public_key()
}

/// Builds and submits a random proposal for the specified view number
async fn submit_proposal(runner: &AppliedTestRunner, sender_node_id: u64, view_number: ViewNumber) {
    let handle = runner.get_handle(sender_node_id).unwrap();

    // Build proposal
    let mut leaf = random_leaf(DEntryBlock::genesis());
    leaf.view_number = view_number;
    let signature = handle.sign_proposal(&leaf.commit(), leaf.view_number);
    let msg = ConsensusMessage::Proposal(Proposal { leaf, signature });

    // Send proposal
    handle.send_broadcast_consensus_message(msg.clone()).await;
}

/// Builds and submits a random vote for the specified view number from the specified node
async fn submit_vote(
    runner: &AppliedTestRunner,
    sender_node_id: u64,
    view_number: ViewNumber,
    recipient_node_id: u64,
) {
    let handle = runner.get_handle(sender_node_id).unwrap();

    // Build vote
    let mut leaf = random_leaf(DEntryBlock::genesis());
    leaf.view_number = view_number;
    let signature = handle.sign_vote(&leaf.commit(), leaf.view_number);
    let msg = ConsensusMessage::Vote(Vote {
        signature,
        justify_qc: leaf.clone().justify_qc,
        current_view: leaf.view_number,
        block_commitment: leaf.deltas.commit(),
        leaf_commitment: leaf.commit(),
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

/// Checks that votes are queued correctly for views 1..NUM_VIEWS
fn test_vote_queueing_post_safety_check(
    runner: &AppliedTestRunner,
    _results: TestRoundResult,
) -> LocalBoxFuture<Result<(), ConsensusRoundError>> {
    async move {
        let node_id = DEFAULT_NODE_ID;
        let mut result = Ok(());

        let handle = runner.get_handle(node_id).unwrap();
        let cur_view = handle.get_current_view().await;


        for i in 1..NUM_VIEWS {
            if is_upcoming_leader(runner, node_id, ViewNumber::new(i)).await {
                let ref_view_number = ViewNumber::new(i - 1);
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

/// For 1..NUM_VIEWS submit votes to each node in the network
fn test_vote_queueing_round_setup(
    runner: &mut AppliedTestRunner,
) -> LocalBoxFuture<Vec<TestTransaction>> {
    async move {
        let node_id = DEFAULT_NODE_ID;
        for j in runner.ids() {
            for i in 1..NUM_VIEWS {
                if is_upcoming_leader(runner, node_id, ViewNumber::new(i)).await {
                    submit_vote(runner, j, ViewNumber::new(i - 1), node_id).await;
                }
            }
        }
        Vec::new()
    }
    .boxed_local()
}

/// Checks views 0..NUM_VIEWS for whether proposal messages are properly queued
fn test_proposal_queueing_post_safety_check(
    runner: &AppliedTestRunner,
    _results: TestRoundResult,
) -> LocalBoxFuture<Result<(), ConsensusRoundError>> {
    async move {
        let node_id = DEFAULT_NODE_ID;
        let mut result = Ok(());

        for node in runner.nodes() {

            let handle = node;
            let cur_view = handle.get_current_view().await;

            for i in 0..NUM_VIEWS {
                if is_upcoming_leader(runner, node_id, ViewNumber::new(i)).await {
                    let ref_view_number = ViewNumber::new(i);
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

/// Submits proposals for 0..NUM_VIEWS rounds where `node_id` is the leader
fn test_proposal_queueing_round_setup(
    runner: &mut AppliedTestRunner,
) -> LocalBoxFuture<Vec<TestTransaction>> {
    async move {
        let node_id = DEFAULT_NODE_ID;

        for i in 0..NUM_VIEWS {
            if is_upcoming_leader(runner, node_id, ViewNumber::new(i)).await {
                submit_proposal(runner, node_id, ViewNumber::new(i)).await;
            }
        }

        Vec::new()
    }
    .boxed_local()
}

/// Submits proposals for views where `node_id` is not the leader, and submits multiple proposals for views where `node_id` is the leader
fn test_bad_proposal_round_setup(
    runner: &mut AppliedTestRunner,
) -> LocalBoxFuture<Vec<TestTransaction>> {
    async move {
        let node_id = DEFAULT_NODE_ID;
        for i in 0..NUM_VIEWS {
            if !is_upcoming_leader(runner, node_id, ViewNumber::new(i)).await {
                submit_proposal(runner, node_id, ViewNumber::new(i)).await;
            } else {
                submit_proposal(runner, node_id, ViewNumber::new(i)).await;
                submit_proposal(runner, node_id, ViewNumber::new(i)).await;
                submit_proposal(runner, node_id, ViewNumber::new(i)).await;
            }
        }
        Vec::new()
    }
    .boxed_local()
}

/// Checks nodes do not queue bad proposal messages
fn test_bad_proposal_post_safety_check(
    runner: &AppliedTestRunner,
    _results: TestRoundResult,
) -> LocalBoxFuture<Result<(), ConsensusRoundError>> {
    async move {
        let node_id = DEFAULT_NODE_ID;
        let mut result = Ok(());

        for node in runner.nodes() {

            let handle = node;
            let cur_view = handle.get_current_view().await;

            for i in 0..NUM_VIEWS {
                let is_upcoming_leader = is_upcoming_leader(runner, node_id, ViewNumber::new(i)).await;
                let ref_view_number = ViewNumber::new(i);
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

/// Submits votes to non-leaders and submits too many votes from a singular node
fn test_bad_vote_round_setup(
    runner: &mut AppliedTestRunner,
) -> LocalBoxFuture<Vec<TestTransaction>> {
    async move {
        let node_id = DEFAULT_NODE_ID;
        for j in runner.ids() {
            for i in 1..NUM_VIEWS {
                submit_vote(runner, j, ViewNumber::new(i - 1), node_id).await;
            }
        }
        Vec::new()
    }
    .boxed_local()
}

/// Checks that non-leaders do not queue votes, and that leaders do not queue more than 1 vote per node
fn test_bad_vote_post_safety_check(
    runner: &AppliedTestRunner,
    _results: TestRoundResult,
) -> LocalBoxFuture<Result<(), ConsensusRoundError>> {
    async move {
        let node_id = DEFAULT_NODE_ID;
        let mut result = Ok(());
        let handle = runner.get_handle(node_id).unwrap();
        let cur_view = handle.get_current_view().await;

        for i in 1..NUM_VIEWS {
            let is_upcoming_leader = is_upcoming_leader(runner, node_id, ViewNumber::new(i)).await;
            let ref_view_number = ViewNumber::new(i - 1);
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

/// Tests that replicas receive and queue valid Proposal messages properly
#[async_test]
#[instrument]
async fn test_proposal_queueing() {
    let num_rounds = 10;
    let description: DetailedTestDescriptionBuilder<
        TestNetwork,
        MemoryStorage<DEntryState>,
        DEntryState,
    > = DetailedTestDescriptionBuilder {
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
#[async_test]
#[instrument]
async fn test_vote_queueing() {
    let num_rounds = 10;
    let description: DetailedTestDescriptionBuilder<
        TestNetwork,
        MemoryStorage<DEntryState>,
        DEntryState,
    > = DetailedTestDescriptionBuilder {
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
        test.rounds[i].setup_round = Some(Box::new(test_vote_queueing_round_setup));
        test.rounds[i].safety_check_post = Some(Box::new(test_vote_queueing_post_safety_check));
    }

    test.execute().await.unwrap();
}

/// Tests that replicas handle bad Proposal messages properly
#[async_test]
#[instrument]
async fn test_bad_proposal() {
    let num_rounds = 10;
    let description: DetailedTestDescriptionBuilder<
        TestNetwork,
        MemoryStorage<DEntryState>,
        DEntryState,
    > = DetailedTestDescriptionBuilder {
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
        test.rounds[i].safety_check_post = Some(Box::new(test_bad_proposal_post_safety_check));
    }

    test.execute().await.unwrap();
}

/// Tests that next leaders handle bad Votes properly.  We allow `num_rounds` of failures because replicas will not be able to come to consensus with the bad votes we submit to them
#[async_test]
#[instrument]
#[ignore]
async fn test_bad_vote() {
    let num_rounds = 10;
    let description: DetailedTestDescriptionBuilder<
        TestNetwork,
        MemoryStorage<DEntryState>,
        DEntryState,
    > = DetailedTestDescriptionBuilder {
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
#[async_test]
#[instrument]
async fn test_single_node_network() {
    let num_rounds = 100;
    let description: DetailedTestDescriptionBuilder<
        TestNetwork,
        MemoryStorage<DEntryState>,
        DEntryState,
    > = DetailedTestDescriptionBuilder {
        general_info: GeneralTestDescriptionBuilder {
            total_nodes: 1,
            start_nodes: 1,
            num_succeeds: num_rounds,
            failure_threshold: 0,
            ..GeneralTestDescriptionBuilder::default()
        },
        rounds: None,
        gen_runner: None,
    };
    description.build().execute().await.unwrap();
}
