#![allow(clippy::panic)]
use futures::StreamExt;
use hotshot::tasks::task_state::CreateTaskState;
use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
use hotshot_macros::{run_test, test_scripts};
use hotshot_testing::{
    all_predicates,
    helpers::{build_fake_view_with_leaf, build_system_handle,vid_share},
    predicates::{Predicate,event::{exact,leaf_decided,all_predicates}},
    random,
    script::{Expectations, InputOrder, TaskScript},
    serial,
    view_generator::TestViewGenerator
};
use hotshot_types::{
    data::ViewNumber, traits::node_implementation::ConsensusTime, vote::HasViewNumber,
};
use std::sync::Arc;
use hotshot_task_impls::{
    events::HotShotEvent::{*,self},
    quorum_vote::QuorumVoteTaskState,
};

use std::time::Duration;

const TIMEOUT: Duration = Duration::from_millis(35);

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_quorum_vote_task_success() {
    use hotshot_task_impls::{events::HotShotEvent::*, quorum_vote::QuorumVoteTaskState};
    use hotshot_testing::{
        helpers::build_system_handle,
        predicates::event::{exact, quorum_vote_send, validated_state_updated},
        view_generator::TestViewGenerator,
    };

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
        );
        consensus_writer.update_saved_leaves(view.leaf.clone());
    }
    drop(consensus_writer);

    // Send the quorum proposal, DAC, VID share data, and validated state, in which case a dummy
    // vote can be formed and the view number will be updated.
    let inputs = vec![random![
        QuorumProposalValidated(proposals[1].data.clone(), leaves[0].clone()),
        DaCertificateRecv(dacs[1].clone()),
        VidShareRecv(vids[1].0[0].clone()),
    ]];

    let expectations = vec![Expectations::from_outputs(all_predicates![
        exact(LockedViewUpdated(ViewNumber::new(1))),
        exact(DaCertificateValidated(dacs[1].clone())),
        exact(VidShareValidated(vids[1].0[0].clone())),
        exact(QuorumVoteDependenciesValidated(ViewNumber::new(2))),
        validated_state_updated(),
        quorum_vote_send(),
    ])];

    let quorum_vote_state =
        QuorumVoteTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;

    let mut script = TaskScript {
        timeout: TIMEOUT,
        state: quorum_vote_state,
        expectations,
    };
    run_test![inputs, script].await;
}

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_quorum_vote_task_vote_now() {
    use hotshot_task_impls::{events::HotShotEvent::*, quorum_vote::QuorumVoteTaskState};
    use hotshot_testing::{
        helpers::build_system_handle,
        predicates::event::{exact, quorum_vote_send, validated_state_updated},
        view_generator::TestViewGenerator,
    };
    use hotshot_types::vote::VoteDependencyData;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(2).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let da_membership = handle.hotshot.memberships.da_membership.clone();

    let mut generator = TestViewGenerator::generate(quorum_membership.clone(), da_membership);

    generator.next().await;
    let view = generator.current_view.clone().unwrap();

    let vote_dependency_data = VoteDependencyData {
        quorum_proposal: view.quorum_proposal.data.clone(),
        parent_leaf: view.leaf.clone(),
        vid_share: view.vid_proposal.0[0].clone(),
        da_cert: view.da_certificate.clone(),
    };

    // Submit an event with just the `VoteNow` event which should successfully send a vote.
    let inputs = vec![serial![VoteNow(view.view_number, vote_dependency_data),]];

    let expectations = vec![Expectations::from_outputs(vec![
        exact(QuorumVoteDependenciesValidated(ViewNumber::new(1))),
        validated_state_updated(),
        quorum_vote_send(),
    ])];

    let quorum_vote_state =
        QuorumVoteTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;

    let mut script = TaskScript {
        timeout: TIMEOUT,
        state: quorum_vote_state,
        expectations,
    };
    run_test![inputs, script].await;
}

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_quorum_vote_task_miss_dependency() {
    use hotshot_task_impls::{events::HotShotEvent::*, quorum_vote::QuorumVoteTaskState};
    use hotshot_testing::{
        helpers::build_system_handle, predicates::event::exact, view_generator::TestViewGenerator,
    };

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(2).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let da_membership = handle.hotshot.memberships.da_membership.clone();

    let mut generator = TestViewGenerator::generate(quorum_membership.clone(), da_membership);

    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut votes = Vec::new();
    let mut dacs = Vec::new();
    let mut vids = Vec::new();
    let mut leaves = Vec::new();
    let consensus = handle.hotshot.consensus().clone();
    let mut consensus_writer = consensus.write().await;
    for view in (&mut generator).take(5).collect::<Vec<_>>().await {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        votes.push(view.create_quorum_vote(&handle));
        dacs.push(view.da_certificate.clone());
        vids.push(view.vid_proposal.clone());
        leaves.push(view.leaf.clone());

        consensus_writer.update_validated_state_map(
            view.quorum_proposal.data.view_number(),
            build_fake_view_with_leaf(view.leaf.clone()),
        );
        consensus_writer.update_saved_leaves(view.leaf.clone());
    }
    drop(consensus_writer);

    // Send three of quorum proposal, DAC, VID share data, and validated state, in which case
    // there's no vote.
    let inputs = vec![
        random![
            QuorumProposalValidated(proposals[1].data.clone(), leaves[0].clone()),
            VidShareRecv(vid_share(&vids[1].0, handle.public_key())),
        ],
        random![
            QuorumProposalValidated(proposals[2].data.clone(), leaves[1].clone()),
            DaCertificateRecv(dacs[2].clone()),
        ],
        random![
            DaCertificateRecv(dacs[3].clone()),
            VidShareRecv(vid_share(&vids[3].0, handle.public_key())),
        ],
    ];

    let expectations = vec![
        Expectations::from_outputs(all_predicates![exact(LockedViewUpdated(ViewNumber::new(1))), exact(VidShareValidated(vids[1].0[0].clone()))]),
        Expectations::from_outputs(all_predicates![exact(LockedViewUpdated(ViewNumber::new(2))), exact(LastDecidedViewUpdated(ViewNumber::new(
            1,
        ))),leaf_decided(),exact(DaCertificateValidated(dacs[2].clone()))]),
        Expectations::from_outputs(all_predicates![
            exact(DaCertificateValidated(dacs[3].clone())),
            exact(VidShareValidated(vids[3].0[0].clone())),
        ]),
    ];

    let quorum_vote_state =
        QuorumVoteTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;

    let mut script = TaskScript {
        timeout: TIMEOUT,
        state: quorum_vote_state,
        expectations,
    };
    run_test![inputs, script].await;
}

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_quorum_vote_task_incorrect_dependency() {
    use hotshot_task_impls::{events::HotShotEvent::*, quorum_vote::QuorumVoteTaskState};
    use hotshot_testing::{
        helpers::build_system_handle, predicates::event::exact, view_generator::TestViewGenerator,
    };

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
    for view in (&mut generator).take(2).collect::<Vec<_>>().await {
        proposals.push(view.quorum_proposal.clone());
        leaves.push(view.leaf.clone());
        dacs.push(view.da_certificate.clone());
        vids.push(view.vid_proposal.clone());
    }

    // Send the correct quorum proposal and DAC, and incorrect VID share data.
    let inputs = vec![random![
        QuorumProposalValidated(proposals[1].data.clone(), leaves[0].clone()),
        DaCertificateRecv(dacs[1].clone()),
        VidShareRecv(vids[0].0[0].clone()),
    ]];

    let expectations = vec![Expectations::from_outputs(all_predicates![
        exact(DaCertificateValidated(dacs[1].clone())),
        exact(VidShareValidated(vids[0].0[0].clone())),
    ])];

    let quorum_vote_state =
        QuorumVoteTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;

    let mut script = TaskScript {
        timeout: TIMEOUT,
        state: quorum_vote_state,
        expectations,
    };
    run_test![inputs, script].await;
}

/// This function generates the outputs to the quorum proposal task (i.e. the emitted events).
/// This happens depending on the view and chain length.
fn generate_outputs(
    chain_length: i32,
    current_view_number: u64,
) -> Vec<Box<dyn Predicate<Arc<HotShotEvent<TestTypes>>>>> {
    match chain_length {
        // This is not - 2 because we start from the parent
        2 => vec![exact(LockedViewUpdated(ViewNumber::new(
            current_view_number - 1,
        )))],
        // This is not - 3 because we start from the parent
        3 => vec![
            exact(LockedViewUpdated(ViewNumber::new(current_view_number - 1))),
            exact(LastDecidedViewUpdated(ViewNumber::new(
                current_view_number - 2,
            ))),
            leaf_decided(),
        ],
        _ => vec![],
    }
}

/// This test validates the the ascension of the leaf chain across a large input space with
/// consistently increasing inputs to ensure that decides and locked view updates
/// occur as expected.
///
/// This test will never propose, instead, we focus exclusively on the processing of the
/// [`HotShotEvent::QuorumProposalValidated`] event in a number of different circumstances. We want to
/// guarantee that a particular space of outputs is generated.
///
/// These outputs should be easy to run since we'll be deterministically incrementing our iterator from
/// 0..100 proposals, inserting the valid state into the map (hence "happy path"). Since we'll know ahead
/// of time, we can essentially anticipate the formation of a valid chain.
///
/// The output sequence is essentially:
/// view 0/1 = No outputs
/// view 2
/// ```rust
/// LockedViewUpdated(1)
/// ```
///
/// view 3
/// ```rust
/// LockedViewUpdated(2)
/// LastDecidedViewUpdated(1)
/// LeafDecided()
/// ```
///
/// view i in 4..n
/// ```rust
/// LockedViewUpdated(i - 1)
/// LastDecidedViewUpdated(i - 2)
/// LeafDecided()
/// ```
///
/// Because we've inserted all of the valid data, the traversals should go exactly as we expect them to.
#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_quorum_vote_task_happy_path_leaf_ascension() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let node_id: usize = 1;
    let handle = build_system_handle(node_id.try_into().unwrap()).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let da_membership = handle.hotshot.memberships.da_membership.clone();
    let mut generator = TestViewGenerator::generate(quorum_membership, da_membership);

    let mut current_chain_length = 0;
    let mut inputs = Vec::new();
    let mut expectations = Vec::new();
    for view_number in 1..100u64 {
        current_chain_length += 1;
        if current_chain_length > 3 {
            current_chain_length = 3;
        }
        // This unwrap is safe here
        let view = generator.next().await.unwrap();
        let proposal = view.quorum_proposal.clone();

        // This intentionally grabs the wrong leaf since it *really* doesn't
        // matter. For the record, this should be view - 1's leaf.
        let leaf = view.leaf.clone();

        // update the consensus shared state
        {
            let consensus = handle.consensus();
            let mut consensus_writer = consensus.write().await;
            consensus_writer.update_validated_state_map(
                ViewNumber::new(view_number),
                build_fake_view_with_leaf(leaf.clone()),
            );
            consensus_writer.update_saved_leaves(leaf.clone());
            consensus_writer.update_vid_shares(
                ViewNumber::new(view_number),
                view.vid_proposal.0[node_id].clone(),
            );
        }

        inputs.push(serial![QuorumProposalValidated(proposal.data, leaf)]);
        expectations.push(Expectations::from_outputs(generate_outputs(
            current_chain_length,
            view_number,
        )));
    }

    let quorum_vote_task_state =
        QuorumVoteTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;

    let mut script = TaskScript {
        timeout: TIMEOUT,
        state: quorum_vote_task_state,
        expectations,
    };
    run_test![inputs, script].await;
}

/// This test non-deterministically injects faults into the leaf ascension process where we randomly
/// drop states, views, etc from the proposals to ensure that we get decide events only when a three
/// chain is detected. This is accomplished by simply looking up in the state map and checking if the
/// parents for a given node indeed exist and, if so, updating the current chain depending on how recent
/// the dropped parent was.
///
/// We utilize the same method to generate the outputs in both cases since it's quite easy to get a predictable
/// output set depending on where we are in the chain. Again, we do *not* propose in this method and instead
/// verify that the leaf ascension is reliable. We also use non-determinism to make sure that our fault
/// injection is randomized to some degree. This helps smoke out corner cases (i.e. the start and end).
#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_quorum_vote_task_fault_injection_leaf_ascension() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let node_id: usize = 1;
    let handle = build_system_handle(node_id.try_into().unwrap()).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let da_membership = handle.hotshot.memberships.da_membership.clone();
    let mut generator = TestViewGenerator::generate(quorum_membership, da_membership);

    let mut current_chain_length = 0;
    let mut dropped_views = Vec::new();

    let mut inputs = Vec::new();
    let mut expectations = Vec::new();
    for view_number in 1..15u64 {
        current_chain_length += 1;
        // If the chain keeps going, then let it keep going
        if current_chain_length > 3 {
            current_chain_length = 3;
        }
        // This unwrap is safe here
        let view = generator.next().await.unwrap();
        let proposal = view.quorum_proposal.clone();

        // This intentionally grabs the wrong leaf since it *really* doesn't
        // matter. For the record, this should be view - 1's leaf.
        let leaf = view.leaf.clone();

        {
            let consensus = handle.consensus();
            let mut consensus_writer = consensus.write().await;

            // Break the chain depending on the prior state. If the immediate parent is not found, we have a chain of
            // 1, but, if it is, and the parent 2 away is not found, we have a 2 chain.
            if consensus_writer
                .validated_state_map()
                .get(&ViewNumber::new(view_number - 1))
                .is_none()
            {
                current_chain_length = 1;
            } else if view_number > 2
                && consensus_writer
                    .validated_state_map()
                    .get(&ViewNumber::new(view_number - 2))
                    .is_none()
            {
                current_chain_length = 2;
            }

            // Update the consensus shared state with a 10% failure rate
            if rand::random::<f32>() < 0.9 {
                consensus_writer.update_validated_state_map(
                    ViewNumber::new(view_number),
                    build_fake_view_with_leaf(leaf.clone()),
                );
                consensus_writer.update_saved_leaves(leaf.clone());
                consensus_writer.update_vid_shares(
                    ViewNumber::new(view_number),
                    view.vid_proposal.0[node_id].clone(),
                );
            } else {
                dropped_views.push(view_number);
            }
        }

        inputs.push(serial![QuorumProposalValidated(proposal.data, leaf)]);
        expectations.push(Expectations::from_outputs(generate_outputs(
            current_chain_length,
            view_number,
        )));
    }

    let quorum_vote_task_state =
        QuorumVoteTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;
    let mut script = TaskScript {
        timeout: TIMEOUT,
        state: quorum_vote_task_state,
        expectations,
    };
    run_test![inputs, script].await;
}
