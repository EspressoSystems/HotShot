mod common;

use async_trait::async_trait;
use hotshot_types::{
    data::ViewNumber,
    message::{ConsensusMessage, Proposal},
};
use std::{collections::HashSet, sync::Arc};

use common::{
    AppliedTestRunner, DetailedTestDescriptionBuilder, GeneralTestDescriptionBuilder, TestNetwork,
    TestRoundResult, TestTransaction,
};
use either::Either::Right;
use futures::{future::LocalBoxFuture, FutureExt};
use hotshot::{
    demos::dentry::{random_leaf, DEntryBlock, State as DemoState},
    traits::{implementations::MemoryStorage, BlockContents, NodeImplementation, Transaction},
    types::{HotShotHandle, Vote},
    HotShot, HotShotInner, H_256,
};
use hotshot_consensus;
use hotshot_testing::{
    network_reliability::{AsynchronousNetwork, PartiallySynchronousNetwork, SynchronousNetwork},
    ConsensusRoundError, Round, TestNodeImpl,
};
use tracing::{error, info, instrument, warn};

const TEST_VIEW_NUMBERS: [u64; 4] = [0, 3, 5, 10];
const NUM_VIEWS: u64 = 100;

enum QueuedMessageTense {
    PastEmpty,
    PastNonEmpty(usize),
    FutureEmpty,
    FutureNonEmpty(usize),
}

// Passing in `runner` here to avoid long type parameter
async fn is_upcoming_leader(
    runner: &AppliedTestRunner,
    node_id: u64,
    view_number: ViewNumber,
) -> bool {
    let handle = runner.get_handle(node_id).unwrap();

    let upcoming_view = view_number; // TODO delete this line

    let leader = handle.get_leader(upcoming_view).await;
    leader == handle.get_public_key()
}

async fn submit_proposal(runner: &AppliedTestRunner, sender_node_id: u64, view_number: ViewNumber) {
    let handle = runner.get_handle(sender_node_id).unwrap();

    // Build proposal
    let mut leaf = random_leaf(DEntryBlock::default());
    leaf.view_number = view_number;

    let signature = handle.sign_proposal(&leaf.hash(), leaf.view_number);

    let msg = ConsensusMessage::Proposal(Proposal { leaf, signature });

    // Send proposal
    let _results = handle.send_broadcast_consensus_message(msg.clone()).await;
}

async fn submit_vote(
    runner: &AppliedTestRunner,
    sender_node_id: u64,
    view_number: ViewNumber,
    recipient_node_id: u64,
) {
    let handle = runner.get_handle(sender_node_id).unwrap();

    // Build proposal
    let mut leaf = random_leaf(DEntryBlock::default());
    leaf.view_number = view_number;

    let signature = handle.sign_vote(&leaf.hash(), leaf.view_number);

    let msg = ConsensusMessage::Vote(Vote {
        signature,
        block_hash: leaf.deltas.hash(),
        justify_qc: leaf.clone().justify_qc,
        leaf_hash: leaf.hash(),
        current_view: leaf.view_number,
    });

    let recipient = runner
        .get_handle(recipient_node_id)
        .unwrap()
        .get_public_key();
    // Send proposal
    let _results = handle
        .send_direct_consensus_message(msg.clone(), recipient)
        .await;
}

fn get_queue_len(is_past: bool, len: Option<usize>) -> QueuedMessageTense {
    let result;
    match is_past {
        true => match len {
            None => result = QueuedMessageTense::PastEmpty,
            Some(len) => result = QueuedMessageTense::PastNonEmpty(len),
        },
        false => match len {
            None => result = QueuedMessageTense::FutureEmpty,
            Some(len) => result = QueuedMessageTense::FutureNonEmpty(len),
        },
    }
    result
}

/// Checks view 0..NUM_VIEWS where node 0 is the leader for Proposal messages
fn test_vote_queueing_post_safety_check(
    runner: &AppliedTestRunner,
    _results: TestRoundResult,
) -> LocalBoxFuture<Result<(), ConsensusRoundError>> {
    async move {
        let node_id = 0;
        let mut result = Ok(());

       

        let handle = runner.get_handle(node_id).unwrap();
        // let handle = node;
        let cur_view = handle.get_current_view().await;

        for i in 1..NUM_VIEWS {
            if is_upcoming_leader(runner, node_id, ViewNumber::new(i)).await {
                let ref_view_number = ViewNumber::new(i - 1);
                let is_past = ref_view_number < cur_view;
                let len = handle
                    .get_next_leader_receiver_channel_len(ref_view_number)
                    .await;

                let state = get_queue_len(is_past, len);
                match state {
                    QueuedMessageTense::PastNonEmpty(len) => {
                        result = Err(ConsensusRoundError::SafetyFailed {
                            // TODO add node id? 
                                                description: format!("Past view's next leader receiver channel still exists for {:?} with {} items in it.  We are currenltly in {:?}", ref_view_number, len, cur_view)
                                            });
                    }
                    QueuedMessageTense::FutureEmpty => {
                        result = Err(ConsensusRoundError::SafetyFailed {
                            description: format!("Next Leader did not properly queue future proposal for {:?}.  We are currently in {:?}", ref_view_number, cur_view)                                            });
                    }
                    _ => {}
                }
            }
        
    }

        result
    }
    .boxed_local()
}

fn test_vote_queueing_round_setup(
    runner: &mut AppliedTestRunner,
) -> LocalBoxFuture<Vec<TestTransaction>> {
    async move {
        // TODO make node_id random?
        for j in runner.ids() {
            let node_id = 0;
            let handle = runner.get_handle(j).unwrap();

            {
                // create vote
                // To avoid overflow
                for i in 1..NUM_VIEWS {
                    if is_upcoming_leader(runner, node_id, ViewNumber::new(i)).await {
                        submit_vote(runner, j, ViewNumber::new(i - 1), node_id).await;
                    }
                }
            }
        }

        Vec::new()
    }
    .boxed_local()
}

/// Checks view 0..NUM_VIEWS where node 0 is the leader for Proposal messages
fn test_proposal_queueing_post_safety_check(
    runner: &AppliedTestRunner,
    _results: TestRoundResult,
) -> LocalBoxFuture<Result<(), ConsensusRoundError>> {
    async move {
        let node_id = 0;
        let mut result = Ok(());

        for node in runner.nodes() {

        // let handle = runner.get_handle(node).unwrap();
        let handle = node;
        let cur_view = handle.get_current_view().await;

        for i in 0..NUM_VIEWS {
            if is_upcoming_leader(runner, node_id, ViewNumber::new(i)).await {
                let ref_view_number = ViewNumber::new(i);
                let is_past = ref_view_number < cur_view;
                let len = handle
                    .get_replica_receiver_channel_len(ref_view_number)
                    .await;

                let state = get_queue_len(is_past, len);
                match state {
                    QueuedMessageTense::PastNonEmpty(len) => {
                        result = Err(ConsensusRoundError::SafetyFailed {
                            // TODO add node id? 
                                                description: format!("Past view's replica receiver channel still exists for {:?} with {} items in it.  We are currenltly in {:?}", ref_view_number, len, cur_view)
                                            });
                    }
                    QueuedMessageTense::FutureEmpty => {
                        result = Err(ConsensusRoundError::SafetyFailed {
                            description: format!("Replica did not properly queue future proposal for {:?}.  We are currently in {:?}", ref_view_number, cur_view)                                            });
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

fn test_proposal_queueing_round_setup(
    runner: &mut AppliedTestRunner,
) -> LocalBoxFuture<Vec<TestTransaction>> {
    async move {
        // TODO make node_id random?
        let node_id = 0;
        let handle = runner.get_handle(node_id).unwrap();

        {
            // create_proposal
            for i in 0..NUM_VIEWS {
                if is_upcoming_leader(runner, node_id, ViewNumber::new(i)).await {
                    submit_proposal(runner, node_id, ViewNumber::new(i)).await;
                }
            }
        }

        Vec::new()
    }
    .boxed_local()
}

fn test_bad_proposal_round_setup(
    runner: &mut AppliedTestRunner,
) -> LocalBoxFuture<Vec<TestTransaction>> {
    async move {
        // TODO make node_id random?
        let node_id = 0;
        let handle = runner.get_handle(node_id).unwrap();

        {
            // create_proposal
            for i in 0..NUM_VIEWS {
                // send proposal where we are not the leader
                if !is_upcoming_leader(runner, node_id, ViewNumber::new(i)).await {
                    submit_proposal(runner, node_id, ViewNumber::new(i)).await;
                } else {
                    submit_proposal(runner, node_id, ViewNumber::new(i)).await;
                    submit_proposal(runner, node_id, ViewNumber::new(i)).await;
                    submit_proposal(runner, node_id, ViewNumber::new(i)).await;
                }
            }
        }

        Vec::new()
    }
    .boxed_local()
}

fn test_bad_proposal_post_safety_check(
    runner: &AppliedTestRunner,
    _results: TestRoundResult,
) -> LocalBoxFuture<Result<(), ConsensusRoundError>> {
    async move {
        let node_id = 0;
        let mut result = Ok(());

        for node in runner.nodes() {

        // let handle = runner.get_handle(node).unwrap();
        let handle = node;
        let cur_view = handle.get_current_view().await;

        for i in 0..NUM_VIEWS {
            let is_upcoming_leader = is_upcoming_leader(runner, node_id, ViewNumber::new(i)).await;

                let ref_view_number = ViewNumber::new(i);
                let is_past = ref_view_number < cur_view;
                let len = handle
                    .get_replica_receiver_channel_len(ref_view_number)
                    .await;

                let state = get_queue_len(is_past, len);
                match state {
                    QueuedMessageTense::PastNonEmpty(len) => {
                        result = Err(ConsensusRoundError::SafetyFailed {
                            // TODO add node id? 
                                                description: format!("Past view's replica receiver channel still exists for {:?} with {} items in it.  We are currenltly in {:?}", ref_view_number, len, cur_view)
                                            });
                    }
                    QueuedMessageTense::FutureNonEmpty(len) => {
                        if !is_upcoming_leader {
                        result = Err(ConsensusRoundError::SafetyFailed {
                            description: format!("Replica queued invalid Proposal message that was not sent from the leader for {:?}.  We are currently in {:?}", ref_view_number, cur_view)                                            });
                    }
                    else if len > 1 {
                        result = Err(ConsensusRoundError::SafetyFailed {
                            description: format!("Replica queued too many Proposal messages for {:?}.  We are currently in {:?}", ref_view_number, cur_view)                                            });

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

fn test_bad_vote_round_setup(
    runner: &mut AppliedTestRunner,
) -> LocalBoxFuture<Vec<TestTransaction>> {
    async move {
        // TODO make node_id random?
        let node_id = 0;

        for j in runner.ids() {
            let handle = runner.get_handle(j).unwrap();

            {
                // create vote
                for i in 1..NUM_VIEWS {
                    submit_vote(runner, j, ViewNumber::new(i - 1), node_id).await;
                }
            }
        }

        Vec::new()
    }
    .boxed_local()
}

fn test_bad_vote_post_safety_check(
    runner: &AppliedTestRunner,
    _results: TestRoundResult,
) -> LocalBoxFuture<Result<(), ConsensusRoundError>> {
    async move {
        let node_id = 0;
        let mut result = Ok(());


        let handle = runner.get_handle(node_id).unwrap();
        // let handle = node;
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
                    QueuedMessageTense::PastNonEmpty(len) => {
                        result = Err(ConsensusRoundError::SafetyFailed {
                            // TODO add node id? 
                                                description: format!("Past view's next leader receiver channel still exists for {:?} with {} items in it.  We are currenltly in {:?}", ref_view_number, len, cur_view)
                                            });
                    }
                    QueuedMessageTense::FutureNonEmpty(len) => {
                        if !is_upcoming_leader {
                        result = Err(ConsensusRoundError::SafetyFailed {
                            description: format!("Next leader queued invalid vote message for {:?}.  We are currently in {:?}", ref_view_number, cur_view)                                            });
                    }
                    else if len > runner.ids().len() {
                        result = Err(ConsensusRoundError::SafetyFailed {
                            description: format!("Next leader queued too many vote messages for {:?}.  We are currently in {:?}", ref_view_number, cur_view)                                            });

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
#[async_std::test]
#[instrument]
#[ignore]
async fn test_proposal_queueing() {
    let num_rounds = 10;
    let description: DetailedTestDescriptionBuilder<
        TestNetwork,
        MemoryStorage<DEntryBlock, DemoState, H_256>,
        DEntryBlock,
        DemoState,
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

/// Tests that next leaders receive and queue Vote messages properly
#[async_std::test]
#[instrument]
#[ignore]
async fn test_vote_queueing() {
    let num_rounds = 10;
    let description: DetailedTestDescriptionBuilder<
        TestNetwork,
        MemoryStorage<DEntryBlock, DemoState, H_256>,
        DEntryBlock,
        DemoState,
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

/// Tests that replicas handle bad proposals properly
#[async_std::test]
#[instrument]
#[ignore]
async fn test_bad_proposal() {
    let num_rounds = 10;
    let description: DetailedTestDescriptionBuilder<
        TestNetwork,
        MemoryStorage<DEntryBlock, DemoState, H_256>,
        DEntryBlock,
        DemoState,
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

/// Tests that next leaders handle bad votes properly
#[async_std::test]
#[instrument]
async fn test_bad_vote() {
    let num_rounds = 10;
    let description: DetailedTestDescriptionBuilder<
        TestNetwork,
        MemoryStorage<DEntryBlock, DemoState, H_256>,
        DEntryBlock,
        DemoState,
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
        test.rounds[i].setup_round = Some(Box::new(test_bad_vote_round_setup));
        test.rounds[i].safety_check_post = Some(Box::new(test_bad_vote_post_safety_check));
    }

    test.execute().await.unwrap();
}

/// Tests when a malicious leader sends different proposals to different nodes
#[async_std::test]
#[instrument]
#[ignore = "Not implemented"]
async fn test_equivocation() {
    todo!()
}

/// Tests a single node network, which also tests when a node is leader in consecutive views
#[async_std::test]
#[instrument]
#[ignore]
async fn test_single_node_network() {
    let num_rounds = 100;
    let description: DetailedTestDescriptionBuilder<
        TestNetwork,
        MemoryStorage<DEntryBlock, DemoState, H_256>,
        DEntryBlock,
        DemoState,
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
    let mut test = description.build();

    test.execute().await.unwrap();
}
