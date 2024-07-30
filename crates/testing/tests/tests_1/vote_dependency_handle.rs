#![cfg(feature = "dependency-tasks")]

use std::time::Duration;

use async_compatibility_layer::art::async_timeout;
use futures::StreamExt;
use hotshot::tasks::task_state::CreateTaskState;
use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
use hotshot_macros::{run_test, test_scripts};
use hotshot_task_impls::{
    events::HotShotEvent::*,
    quorum_vote::{QuorumVoteTaskState, VoteDependencyHandle},
};
use hotshot_testing::{
    all_predicates,
    helpers::{build_fake_view_with_leaf, build_system_handle, vid_share},
    predicates::{event::*, Predicate, PredicateResult},
    random,
    script::{Expectations, InputOrder, TaskScript},
    serial,
    view_generator::TestViewGenerator,
};
use hotshot_types::{
    consensus::OuterConsensus, data::ViewNumber, traits::node_implementation::ConsensusTime,
    vote::HasViewNumber,
};

const TIMEOUT: Duration = Duration::from_millis(35);

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_vote_dependency_handle() {
    use std::sync::Arc;

    use hotshot_task_impls::helpers::broadcast_event;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let node_id = 2;
    let handle = build_system_handle::<TestTypes, MemoryImpl>(node_id)
        .await
        .0;
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
        consensus_writer
            .update_validated_state_map(
                view.quorum_proposal.data.view_number(),
                build_fake_view_with_leaf(view.leaf.clone()),
            )
            .unwrap();
        consensus_writer.update_saved_leaves(view.leaf.clone());
    }
    drop(consensus_writer);

    // 1,2,3
    // 1,3,2
    // 2,1,3
    // 2,3,1
    // 3,1,2
    // 3,2,1

    let all_inputs = vec![
        vec![
            DaCertificateValidated(dacs[1].clone()),
            QuorumProposalValidated(proposals[1].data.clone(), leaves[0].clone()),
            VidShareValidated(vids[1].0[0].clone()),
        ],
        vec![
            DaCertificateValidated(dacs[1].clone()),
            VidShareValidated(vids[1].0[0].clone()),
            QuorumProposalValidated(proposals[1].data.clone(), leaves[0].clone()),
        ],
        vec![
            QuorumProposalValidated(proposals[1].data.clone(), leaves[0].clone()),
            DaCertificateValidated(dacs[1].clone()),
            VidShareValidated(vids[1].0[0].clone()),
        ],
        vec![
            QuorumProposalValidated(proposals[1].data.clone(), leaves[0].clone()),
            VidShareValidated(vids[1].0[0].clone()),
            DaCertificateValidated(dacs[1].clone()),
        ],
        vec![
            VidShareValidated(vids[1].0[0].clone()),
            DaCertificateValidated(dacs[1].clone()),
            QuorumProposalValidated(proposals[1].data.clone(), leaves[0].clone()),
        ],
        vec![
            VidShareValidated(vids[1].0[0].clone()),
            QuorumProposalValidated(proposals[1].data.clone(), leaves[0].clone()),
            DaCertificateValidated(dacs[1].clone()),
        ],
    ];

    for inputs in all_inputs.into_iter() {
        let outputs = vec![
            exact(QuorumVoteDependenciesValidated(ViewNumber::new(2))),
            validated_state_updated(),
            quorum_vote_send(),
        ];

        let qv = QuorumVoteTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;

        let event_sender = handle.internal_event_stream_sender();
        let mut event_receiver = handle.internal_event_stream_receiver_known_impl();
        let view_number = ViewNumber::new(node_id);

        let vote_dependency_handle_state = VoteDependencyHandle::<TestTypes, MemoryImpl> {
            public_key: qv.public_key.clone(),
            private_key: qv.private_key.clone(),
            consensus: OuterConsensus::new(Arc::clone(&qv.consensus.inner_consensus)),
            instance_state: Arc::clone(&qv.instance_state),
            quorum_membership: Arc::clone(&qv.quorum_membership),
            storage: Arc::clone(&qv.storage),
            view_number,
            sender: event_sender.clone(),
            receiver: event_receiver.clone(),
            decided_upgrade_certificate: Arc::clone(&qv.decided_upgrade_certificate),
            id: qv.id,
        };

        let inputs_len = inputs.len();
        for event in inputs.into_iter() {
            broadcast_event(event.into(), &event_sender).await;
        }

        let mut i = 0;
        let mut output_events = vec![];
        while let Ok(Ok(received_output)) =
            async_timeout(TIMEOUT, event_receiver.recv_direct()).await
        {
            if i < inputs_len {
                i += 1;
                continue;
            }

            output_events.push(received_output);
        }

        for (check, real) in outputs.into_iter().zip(output_events) {
            if check.evaluate(&real).await == PredicateResult::Fail {
                panic!("Output {real} did not match expected output {check:?}");
            }
        }
    }
}
