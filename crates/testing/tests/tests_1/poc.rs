use futures::StreamExt;
use hotshot_macros::{run_test, test_scripts};
use hotshot::tasks::task_state::CreateTaskState;
use hotshot_task::task::TaskState;
use hotshot_testing::{
    helpers::{
        build_system_handle,
        build_fake_view_with_leaf,
    },
    script::{Expectations, InputOrder, TaskScript},
    view_generator::TestViewGenerator,
    all_predicates,
    predicates::event::{exact, quorum_vote_send, validated_state_updated, all_predicates},
    random,
    serial,
};
use hotshot_example_types::node_types::{

    TestTypes,
    MemoryImpl,
};
use hotshot_types::{
    vid::VidCommitment,
    data::ViewNumber,
    vote::HasViewNumber,
    traits::{
        block_contents::vid_commitment,
        node_implementation::ConsensusTime,
    },
};
use hotshot_example_types::block_types::TestTransaction;
use hotshot_task_impls::{
    events::HotShotEvent::*,
    quorum_vote::QuorumVoteTaskState,
    rewind::RewindTaskState,
};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_millis(35);

fn make_zeroed_commitment() -> VidCommitment {
    let enc = TestTransaction::encode(&Vec::new());
    vid_commitment(&enc, 1)
}

/// This POC is simple:
/// 1. Execute a bogus DaCe
#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn poc_da_vote_block_simple() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(2).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let da_membership = handle.hotshot.memberships.da_membership.clone();

    let mut generator = TestViewGenerator::generate(quorum_membership.clone(), da_membership);

    let mut proposals = Vec::new();
    let mut leaves = Vec::new();
    let mut dacs = Vec::new();
    let mut vids = Vec::new();
    let consensus = handle.hotshot.consensus().clone();
    let mut consensus_writer = consensus.write().await;
    for view in (&mut generator).take(2).collect::<Vec<_>>().await {
        proposals.push(view.quorum_proposal.clone());
        leaves.push(view.leaf.clone());
        dacs.push(view.da_certificate.clone());
        vids.push(view.vid_proposal.clone());
        consensus_writer.update_validated_state_map(
            view.quorum_proposal.data.view_number(),
            build_fake_view_with_leaf(view.leaf.clone()),
        ).unwrap();
        consensus_writer.update_saved_leaves(view.leaf.clone());
    }
    drop(consensus_writer);

    // Send the quorum proposal, DAC, VID share data, and validated state, in which case a dummy
    // vote can be formed and the view number will be updated.

    let bogus_commitment = make_zeroed_commitment();
    dacs[1].data.payload_commit = bogus_commitment;
    let inputs = vec![serial![
        DaCertificateRecv(dacs[1].clone()),
        VidShareRecv(vids[1].0[0].clone()),
        QuorumProposalValidated(proposals[1].data.clone(), leaves[0].clone()),
    ]];


    let expectations = vec![Expectations::from_outputs(all_predicates![
        exact(DaCertificateValidated(dacs[1].clone())),
        exact(VidShareValidated(vids[1].0[0].clone())),
        exact(Dummy(proposals[1].data.clone(), leaves[0].clone())),

        // WE NO LONGER VOTE!!! AHHH
        // exact(QuorumVoteDependenciesValidated(ViewNumber::new(2))),
        // validated_state_updated(),
        // quorum_vote_send(),
    ])];

    let quorum_vote_state =
        QuorumVoteTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;
    let mut rewind_task_state =
        RewindTaskState::<TestTypes>::create_from(&handle).await;

    let mut script = TaskScript {
        timeout: TIMEOUT,
        state: quorum_vote_state,
        expectations,
    };
    let mut rewind_script = TaskScript {
        timeout: TIMEOUT,
        state: rewind_task_state,
        expectations: vec![
            Expectations {
                output_asserts: vec![],
                task_state_asserts: vec![],
            },
        ],
    };
    run_test![inputs, script, rewind_script].await;

    rewind_script.state.cancel_subtasks().await;
}
