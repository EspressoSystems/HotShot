use std::{sync::Arc, time::Duration};

use futures::StreamExt;
use hotshot::tasks::task_state::CreateTaskState;
use hotshot_example_types::{
    block_types::TestTransaction,
    node_types::{MemoryImpl, TestTypes, TestVersions},
};
use hotshot_macros::{run_test, test_scripts};
use hotshot_task_impls::{da::DaTaskState, events::HotShotEvent};
use hotshot_testing::{
    helpers::{build_system_handle_from_launcher, check_external_events},
    predicates::event::{exact, expect_external_events, ext_event_exact},
    script::{Expectations, InputOrder, TaskScript},
    serial,
    test_builder::TestDescription,
    view_generator::TestViewGenerator,
};
use hotshot_types::{
    data::ViewNumber,
    event::{Event, EventType},
    simple_vote::DaData,
    traits::{
        block_contents::precompute_vid_commitment, election::Membership,
        node_implementation::ConsensusTime,
    },
};


/// Test the DA Task for handling an outdated proposal.
/// 
/// This test checks that when an outdated DA proposal is received, it doesn't produce 
/// any output, while a current proposal triggers appropriate actions (validation and voting).
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_da_task_outdated_proposal() {
    // Setup logging and backtrace for debugging
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    // Parameters for the test
    let node_id: u64 = 2;
    let num_nodes: usize = 10;
    let da_committee_size: usize = 7;

    // Initialize test description with node and committee details
    let test_description = TestDescription {
        num_nodes_with_stake: num_nodes,
        da_staked_committee_size: da_committee_size,
        start_nodes: num_nodes,
        ..TestDescription::default()
    };

    // Generate a launcher for the test system with a custom configuration
    let launcher = test_description
        .gen_launcher(node_id)
        .modify_default_config(|config| {
            config.next_view_timeout = 1000;
            config.timeout_ratio = (12, 10);
            config.da_staked_committee_size = da_committee_size;
        });

    // Build the system handle using the launcher configuration
    let handle =
        build_system_handle_from_launcher::<TestTypes, MemoryImpl, TestVersions>(node_id, launcher, None)
            .await
            .expect("Failed to initialize HotShot");

    // Prepare empty transactions and compute a commitment for later use
    let transactions = vec![TestTransaction::new(vec![0])];
    let encoded_transactions = Arc::from(TestTransaction::encode(&transactions));
    let (payload_commit, _precompute) = precompute_vid_commitment(
        &encoded_transactions,
        handle.hotshot.memberships.quorum_membership.total_nodes(),
    );

    // Initialize a view generator using the current memberships
    let mut view_generator = TestViewGenerator::generate(
        handle.hotshot.memberships.quorum_membership.clone(),
        handle.hotshot.memberships.da_membership.clone(),
    );

    // Generate views for the test
    let view1 = view_generator.next().await.unwrap();
    let _view2 = view_generator.next().await.unwrap();
    view_generator.add_transactions(transactions);
    let view3 = view_generator.next().await.unwrap();

    // Define input events for the test:
    // 1. Three view changes and an outdated proposal for view 1 when in view 3
    // 2. A current proposal for view 3
    let inputs = vec![
        serial![
            HotShotEvent::ViewChange(ViewNumber::new(1)),
            HotShotEvent::ViewChange(ViewNumber::new(2)),
            HotShotEvent::ViewChange(ViewNumber::new(3)),
            // Send an outdated proposal (view 1) when we're in view 3
            HotShotEvent::DaProposalRecv(view1.da_proposal.clone(), view1.leader_public_key),
        ],
        serial![
            // Send a current proposal (view 3)
            HotShotEvent::DaProposalRecv(view3.da_proposal.clone(), view3.leader_public_key),
        ],
    ];

    // Define expectations:
    // 1. No output for the outdated proposal
    // 2. Validation and voting actions for the current proposal
    let expectations = vec![
        Expectations::from_outputs(vec![]),
        Expectations::from_outputs(vec![
            exact(HotShotEvent::DaProposalValidated(
                view3.da_proposal.clone(),
                view3.leader_public_key,
            )),
            exact(HotShotEvent::DaVoteSend(view3.create_da_vote(
                DaData {
                    payload_commit,
                },
                &handle,
            ))),
        ]),
    ];

    // Define expectations for external events triggered by the system
    let external_event_expectations = vec![expect_external_events(vec![ext_event_exact(Event {
        view_number: view3.view_number,
        event: EventType::DaProposal {
            proposal: view3.da_proposal.clone(),
            sender: view3.leader_public_key,
        },
    })])];

    // Create DA task state and script for the test
    let da_state = DaTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;
    let mut da_script = TaskScript {
        timeout: Duration::from_millis(100),
        state: da_state,
        expectations,
    };

    // Run the test with the inputs and check the resulting events
    let output_event_stream_recv = handle.event_stream();
    run_test![inputs, da_script].await;

    // Validate the external events against expectations
    let result = check_external_events(
        output_event_stream_recv,
        &external_event_expectations,
        da_script.timeout,
    )
    .await;
    assert!(result.is_ok(), "{}", result.err().unwrap());
}

/// Test the DA Task for handling duplicate votes.
///
/// This test ensures that when duplicate votes are received in the DA Task,
/// they are correctly ignored without producing any output. The original
/// proposal and vote are processed as expected, validating the proposal
/// and sending the first vote, while the duplicate vote is disregarded.
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_da_task_duplicate_votes() {
    // Setup logging and backtrace for debugging
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    // Parameters for the test
    let node_id: u64 = 2;
    let num_nodes: usize = 10;
    let da_committee_size: usize = 7;

    // Initialize test description with node and committee details
    let test_description = TestDescription {
        num_nodes_with_stake: num_nodes,
        da_staked_committee_size: da_committee_size,
        start_nodes: num_nodes,
        ..TestDescription::default()
    };

    // Generate a launcher for the test system with a custom configuration
    let launcher = test_description
        .gen_launcher(node_id)
        .modify_default_config(|config| {
            config.next_view_timeout = 1000;
            config.timeout_ratio = (12, 10);
            config.da_staked_committee_size = da_committee_size;
        });

    // Build the system handle using the launcher configuration
    let handle =
        build_system_handle_from_launcher::<TestTypes, MemoryImpl, TestVersions>(node_id, launcher, None)
            .await
            .expect("Failed to initialize HotShot");

    // Prepare empty transactions and compute a commitment for later use
    let transactions = vec![TestTransaction::new(vec![0])];
    let encoded_transactions = Arc::from(TestTransaction::encode(&transactions));
    let (payload_commit, _precompute) = precompute_vid_commitment(
        &encoded_transactions,
        handle.hotshot.memberships.quorum_membership.total_nodes(),
    );

    // Initialize a view generator using the current memberships
    let mut view_generator = TestViewGenerator::generate(
        handle.hotshot.memberships.quorum_membership.clone(),
        handle.hotshot.memberships.da_membership.clone(),
    );

    // Generate views for the test
    let _view1 = view_generator.next().await.unwrap();
    view_generator.add_transactions(transactions);
    let view2 = view_generator.next().await.unwrap();

    // Create a duplicate vote
    let duplicate_vote = view2.create_da_vote(
        DaData {
            payload_commit,
        },
        &handle,
    );

    let inputs = vec![
        serial![
            HotShotEvent::ViewChange(ViewNumber::new(1)),
            HotShotEvent::ViewChange(ViewNumber::new(2)),
            HotShotEvent::DaProposalRecv(view2.da_proposal.clone(), view2.leader_public_key),
        ],
        serial![
            // Send the original vote
            HotShotEvent::DaVoteRecv(duplicate_vote.clone()),
            // Send the duplicate vote
            HotShotEvent::DaVoteRecv(duplicate_vote.clone()),
        ],
    ];

    // We expect the task to process the proposal and the first vote, but ignore the duplicate
    let expectations = vec![
        Expectations::from_outputs(vec![
            exact(HotShotEvent::DaProposalValidated(
                view2.da_proposal.clone(),
                view2.leader_public_key,
            )),
            exact(HotShotEvent::DaVoteSend(duplicate_vote.clone())),
        ]),
        Expectations::from_outputs(vec![
            // No output expected for the duplicate vote
        ]),
    ];

    // We expect to see an external event for the proposal, but not for individual votes
    let external_event_expectations = vec![expect_external_events(vec![ext_event_exact(Event {
        view_number: view2.view_number,
        event: EventType::DaProposal {
            proposal: view2.da_proposal.clone(),
            sender: view2.leader_public_key,
        },
    })])];

    // Create DA task state and script for the test
    let da_state = DaTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;
    let mut da_script = TaskScript {
        timeout: Duration::from_millis(100),
        state: da_state,
        expectations,
    };

    // Run the test with the inputs and check the resulting events
    let output_event_stream_recv = handle.event_stream();
    run_test![inputs, da_script].await;

    // Validate the external events against expectations
    let result = check_external_events(
        output_event_stream_recv,
        &external_event_expectations,
        da_script.timeout,
    )
    .await;
    assert!(result.is_ok(), "{}", result.err().unwrap());
}

/// Tests the DA Task for collecting and processing valid votes.
///
/// This test verifies that the DA Task correctly handles the proposal and votes
/// from DA committee members. It does the following:
///
/// 1. Initializes a test environment with multiple nodes, configuring one as the leader.
/// 2. Creates and sends a DA proposal followed by valid votes from the committee.
/// 3. Asserts that the proposal is validated and a vote is sent in response by the leader.
///
/// The test ensures that only the intended vote is processed.
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_da_task_vote_collection() {
    // Initialize logging and backtrace for debugging
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    // Setup: Create system handles for multiple nodes (at least 5 for DA committee)
    let leader_node_id: usize = 4;
    let num_nodes: usize = 6;
    let da_committee_size: usize = 5;

    let test_description = TestDescription {
        num_nodes_with_stake: num_nodes,
        da_staked_committee_size: da_committee_size,
        start_nodes: num_nodes,
        ..TestDescription::default()
    };

    // Create handles and view generators for all nodes
    let mut handles = Vec::new();
    let mut view_generators = Vec::new();

    for node_id in 0..num_nodes as u64 {
        let launcher = test_description
            .clone()
            .gen_launcher(node_id)
            .modify_default_config(|config| {
                config.next_view_timeout = 1000;
                config.timeout_ratio = (12, 10);
                config.da_staked_committee_size = da_committee_size;
            });

        let handle =
            build_system_handle_from_launcher::<TestTypes, MemoryImpl, TestVersions>(node_id, launcher, None)
                .await
                .expect("Failed to initialize HotShot");

        // Create the scenario with necessary views
        let view_generator = TestViewGenerator::generate(
            handle.hotshot.memberships.quorum_membership.clone(),
            handle.hotshot.memberships.da_membership.clone(),
        );

        handles.push(handle);
        view_generators.push(view_generator);
    }

    // Prepare empty transactions and compute a commitment for later use
    let transactions = vec![TestTransaction::new(vec![0])];
    let encoded_transactions = Arc::from(TestTransaction::encode(&transactions));
    let (payload_commit, _precompute) = precompute_vid_commitment(
        &encoded_transactions,
        handles[leader_node_id]
            .hotshot
            .memberships
            .quorum_membership
            .total_nodes(),
    );

    let _view1 = view_generators[leader_node_id].next().await.unwrap();
    let _view2 = view_generators[leader_node_id].next().await.unwrap();
    let _view3 = view_generators[leader_node_id].next().await.unwrap();
    view_generators[leader_node_id].add_transactions(transactions);
    let view4 = view_generators[leader_node_id].next().await.unwrap();

    // Create votes for all nodes
    let votes: Vec<_> = handles
        .iter()
        .map(|handle| view4.create_da_vote(DaData { payload_commit }, handle))
        .collect();

    // Simulate sending valid votes
    let inputs = vec![
        serial![
            HotShotEvent::ViewChange(ViewNumber::new(1)),
            HotShotEvent::ViewChange(ViewNumber::new(2)),
            HotShotEvent::ViewChange(ViewNumber::new(3)),
            HotShotEvent::ViewChange(ViewNumber::new(4)),
            HotShotEvent::DaProposalRecv(view4.da_proposal.clone(), view4.leader_public_key),
        ],
        serial![
            // Send votes from the DA committee members
            HotShotEvent::DaVoteRecv(votes[0].clone()),
            HotShotEvent::DaVoteRecv(votes[1].clone()),
            HotShotEvent::DaVoteRecv(votes[2].clone()),
            HotShotEvent::DaVoteRecv(votes[3].clone()),
        ],
    ];

    // Assert the outcome
    let expectations = vec![
        Expectations::from_outputs(vec![
            exact(HotShotEvent::DaProposalValidated(
                view4.da_proposal.clone(),
                view4.leader_public_key,
            )),
            exact(HotShotEvent::DaVoteSend(votes[leader_node_id].clone())),
        ]),
        Expectations::from_outputs(vec![exact(HotShotEvent::DacSend(
            view4.da_certificate,
            view4.leader_public_key,
        ))]),
    ];

    // Create DA task state and script
    let da_state =
        DaTaskState::<TestTypes, MemoryImpl>::create_from(&handles[leader_node_id]).await;
    let mut da_script = TaskScript {
        timeout: Duration::from_millis(100),
        state: da_state,
        expectations,
    };

    // Run the test
    let output_event_stream_recv = handles[leader_node_id].event_stream();
    run_test![inputs, da_script].await;

    // Check for DacSend event in the output stream
    let result = check_external_events(
        output_event_stream_recv,
        &[expect_external_events(vec![ext_event_exact(Event {
            view_number: view4.view_number,
            event: EventType::DaProposal {
                proposal: view4.da_proposal.clone(),
                sender: view4.leader_public_key,
            },
        })])],
        da_script.timeout,
    )
    .await;
    assert!(result.is_ok(), "{}", result.err().unwrap());
}

/// Test that non-leader nodes correctly handle DA (Data Availability) votes and
/// ignore the certificate event when the node is not the leader during the voting process.
///
/// This test sets up a scenario where a specific node (not the leader) processes
/// multiple views and receives votes from DA committee members. The test checks
/// that the expected outcome occurs, verifying that a `DacSend` event is not
/// generated since the node is not the leader.
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_da_task_non_leader_vote_collection_ignore() {
    // Initialize logging and backtrace for debugging
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    // Setup: Create system handles for multiple nodes (at least 5 for DA committee)
    let leader_node_id: usize = 3;
    let num_nodes: usize = 6;
    let da_committee_size: usize = 5;

    let test_description = TestDescription {
        num_nodes_with_stake: num_nodes,
        da_staked_committee_size: da_committee_size,
        start_nodes: num_nodes,
        ..TestDescription::default()
    };

    // Create handles and view generators for all nodes
    let mut handles = Vec::new();
    let mut view_generators = Vec::new();

    for node_id in 0..num_nodes as u64 {
        let launcher = test_description
            .clone()
            .gen_launcher(node_id)
            .modify_default_config(|config| {
                config.next_view_timeout = 1000;
                config.timeout_ratio = (12, 10);
                config.da_staked_committee_size = da_committee_size;
            });

        let handle =
            build_system_handle_from_launcher::<TestTypes, MemoryImpl, TestVersions>(node_id, launcher, None)
                .await
                .expect("Failed to initialize HotShot");

        // Create the scenario with necessary views
        let view_generator = TestViewGenerator::generate(
            handle.hotshot.memberships.quorum_membership.clone(),
            handle.hotshot.memberships.da_membership.clone(),
        );

        handles.push(handle);
        view_generators.push(view_generator);
    }

    // Prepare empty transactions and compute a commitment for later use
    let transactions = vec![TestTransaction::new(vec![0])];
    let encoded_transactions = Arc::from(TestTransaction::encode(&transactions));
    let (payload_commit, _precompute) = precompute_vid_commitment(
        &encoded_transactions,
        handles[leader_node_id]
            .hotshot
            .memberships
            .quorum_membership
            .total_nodes(),
    );

    // Generate multiple views and add transactions for the leader node
    let _view1 = view_generators[leader_node_id].next().await.unwrap();
    let _view2 = view_generators[leader_node_id].next().await.unwrap();
    let _view3 = view_generators[leader_node_id].next().await.unwrap();
    view_generators[leader_node_id].add_transactions(transactions);
    let view4 = view_generators[leader_node_id].next().await.unwrap();

    // Create votes for all nodes
    let votes: Vec<_> = handles
        .iter()
        .map(|handle| view4.create_da_vote(DaData { payload_commit }, handle))
        .collect();

    // Simulate sending valid votes and a proposal to the DA committee
    let inputs = vec![
        serial![
            HotShotEvent::ViewChange(ViewNumber::new(1)),
            HotShotEvent::ViewChange(ViewNumber::new(2)),
            HotShotEvent::ViewChange(ViewNumber::new(3)),
            HotShotEvent::ViewChange(ViewNumber::new(4)),
            HotShotEvent::DaProposalRecv(view4.da_proposal.clone(), view4.leader_public_key),
        ],
        serial![
            // Send votes from the DA committee members
            HotShotEvent::DaVoteRecv(votes[0].clone()),
            HotShotEvent::DaVoteRecv(votes[1].clone()),
            HotShotEvent::DaVoteRecv(votes[2].clone()),
            HotShotEvent::DaVoteRecv(votes[3].clone()),
        ],
    ];

    // Define the expected outcome for the non-leader node
    let expectations = vec![
        Expectations::from_outputs(vec![
            exact(HotShotEvent::DaProposalValidated(
                view4.da_proposal.clone(),
                view4.leader_public_key,
            )),
            exact(HotShotEvent::DaVoteSend(votes[leader_node_id].clone())),
        ]),
        Expectations::from_outputs(vec![]),
    ];

    // Create DA task state and script for the test
    let da_state =
        DaTaskState::<TestTypes, MemoryImpl>::create_from(&handles[leader_node_id]).await;
    let mut da_script = TaskScript {
        timeout: Duration::from_millis(100),
        state: da_state,
        expectations,
    };

    // Run the test with the prepared inputs and script
    let output_event_stream_recv = handles[leader_node_id].event_stream();
    run_test![inputs, da_script].await;

    // Verify that the non-leader node did not generate the DacSend event
    let result = check_external_events(
        output_event_stream_recv,
        &[expect_external_events(vec![ext_event_exact(Event {
            view_number: view4.view_number,
            event: EventType::DaProposal {
                proposal: view4.da_proposal.clone(),
                sender: view4.leader_public_key,
            },
        })])],
        da_script.timeout,
    )
    .await;
    assert!(result.is_ok(), "{}", result.err().unwrap());
}
