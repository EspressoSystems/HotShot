use hotshot::tasks::{inject_consensus_polls, task_state::CreateTaskState};
use hotshot::types::SystemContextHandle;
use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
use hotshot_task_impls::{consensus::ConsensusTaskState, events::HotShotEvent::*};
use hotshot_testing::task_helpers::key_pair_for_id;
use hotshot_testing::test_helpers::permute_input_with_index_order;
use hotshot_testing::{
    predicates::{exact, is_at_view_number, quorum_proposal_send, quorum_vote_send},
    script::{run_test_script, TestScriptStage},
    task_helpers::{build_system_handle, vid_scheme_from_view_number},
    view_generator::TestViewGenerator,
};
use hotshot_types::simple_vote::ViewSyncFinalizeData;
use hotshot_types::{data::ViewNumber, traits::node_implementation::ConsensusTime};
use jf_primitives::vid::VidScheme;

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_consensus_task() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(2).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();

    // Make some empty encoded transactions, we just care about having a commitment handy for the
    // later calls. We need the VID commitment to be able to propose later.
    let mut vid = vid_scheme_from_view_number::<TestTypes>(&quorum_membership, ViewNumber::new(2));
    let encoded_transactions = Vec::new();
    let vid_disperse = vid.disperse(&encoded_transactions).unwrap();
    let payload_commitment = vid_disperse.commit;

    let mut generator = TestViewGenerator::generate(quorum_membership.clone());

    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut votes = Vec::new();
    let mut dacs = Vec::new();
    let mut vids = Vec::new();
    for view in (&mut generator).take(2) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        votes.push(view.create_quorum_vote(&handle));
        dacs.push(view.da_certificate.clone());
        vids.push(view.vid_proposal.clone());
    }

    // Run view 1 (the genesis stage).
    let view_1 = TestScriptStage {
        inputs: vec![
            QuorumProposalRecv(proposals[0].clone(), leaders[0]),
            DACRecv(dacs[0].clone()),
            VidDisperseRecv(vids[0].0.clone()),
        ],
        outputs: vec![
            exact(ViewChange(ViewNumber::new(1))),
            exact(QuorumProposalValidated(proposals[0].data.clone())),
            exact(QuorumVoteSend(votes[0].clone())),
        ],
        asserts: vec![is_at_view_number(1)],
    };

    let cert = proposals[1].data.justify_qc.clone();

    // Run view 2 and propose.
    let view_2 = TestScriptStage {
        inputs: vec![
            QuorumProposalRecv(proposals[1].clone(), leaders[1]),
            QCFormed(either::Left(cert)),
            // We must have a payload commitment and metadata to propose.
            SendPayloadCommitmentAndMetadata(payload_commitment, (), ViewNumber::new(2)),
        ],
        outputs: vec![
            exact(ViewChange(ViewNumber::new(2))),
            exact(QuorumProposalValidated(proposals[1].data.clone())),
            quorum_proposal_send(),
        ],
        asserts: vec![is_at_view_number(2)],
    };

    let consensus_state = ConsensusTaskState::<
        TestTypes,
        MemoryImpl,
        SystemContextHandle<TestTypes, MemoryImpl>,
    >::create_from(&handle)
    .await;

    inject_consensus_polls(&consensus_state).await;

    run_test_script(vec![view_1, view_2], consensus_state).await;
}

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_consensus_vote() {
    use hotshot::tasks::{inject_consensus_polls, task_state::CreateTaskState};
    use hotshot_task_impls::{consensus::ConsensusTaskState, events::HotShotEvent::*};
    use hotshot_testing::{
        predicates::exact,
        script::{run_test_script, TestScriptStage},
        task_helpers::build_system_handle,
        view_generator::TestViewGenerator,
    };

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(2).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();

    let mut generator = TestViewGenerator::generate(quorum_membership.clone());

    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut votes = Vec::new();
    let mut dacs = Vec::new();
    let mut vids = Vec::new();
    for view in (&mut generator).take(2) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        votes.push(view.create_quorum_vote(&handle));
        dacs.push(view.da_certificate.clone());
        vids.push(view.vid_proposal.clone());
    }

    // Send a proposal, vote on said proposal, update view based on proposal QC, receive vote as next leader
    let view_1 = TestScriptStage {
        inputs: vec![
            QuorumProposalRecv(proposals[0].clone(), leaders[0]),
            DACRecv(dacs[0].clone()),
            VidDisperseRecv(vids[0].0.clone()),
            QuorumVoteRecv(votes[0].clone()),
        ],
        outputs: vec![
            exact(ViewChange(ViewNumber::new(1))),
            exact(QuorumProposalValidated(proposals[0].data.clone())),
            exact(QuorumVoteSend(votes[0].clone())),
        ],
        asserts: vec![],
    };

    let consensus_state = ConsensusTaskState::<
        TestTypes,
        MemoryImpl,
        SystemContextHandle<TestTypes, MemoryImpl>,
    >::create_from(&handle)
    .await;

    inject_consensus_polls(&consensus_state).await;
    run_test_script(vec![view_1], consensus_state).await;
}

/// Tests the voting behavior by allowing the input to be permuted in any order desired. This
/// assures that, no matter what, a vote is indeed sent no matter what order the precipitating
/// events occur. The permutation is specified as `input_permutation` and is a vector of indices.
async fn test_vote_with_specific_order(input_permutation: Vec<usize>) {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(2).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();

    let mut generator = TestViewGenerator::generate(quorum_membership.clone());

    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut votes = Vec::new();
    let mut dacs = Vec::new();
    let mut vids = Vec::new();
    for view in (&mut generator).take(2) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        votes.push(view.create_quorum_vote(&handle));
        dacs.push(view.da_certificate.clone());
        vids.push(view.vid_proposal.clone());
    }

    // Get out of the genesis view first
    let view_1 = TestScriptStage {
        inputs: vec![
            QuorumProposalRecv(proposals[0].clone(), leaders[0]),
            DACRecv(dacs[0].clone()),
            VidDisperseRecv(vids[0].0.clone()),
        ],
        outputs: vec![
            exact(ViewChange(ViewNumber::new(1))),
            exact(QuorumProposalValidated(proposals[0].data.clone())),
            exact(QuorumVoteSend(votes[0].clone())),
        ],
        asserts: vec![is_at_view_number(1)],
    };

    let inputs = vec![
        // We need a VID share for view 2 otherwise we cannot vote at view 2 (as node 2).
        VidDisperseRecv(vids[1].0.clone()),
        DACRecv(dacs[1].clone()),
        QuorumProposalRecv(proposals[1].clone(), leaders[1]),
    ];
    let view_2_inputs = permute_input_with_index_order(inputs, input_permutation);

    let view_2_outputs = vec![
        exact(ViewChange(ViewNumber::new(2))),
        exact(QuorumProposalValidated(proposals[1].data.clone())),
        exact(QuorumVoteSend(votes[1].clone())),
    ];

    // Use the permuted inputs for view 2 depending on the provided index ordering.
    let view_2 = TestScriptStage {
        inputs: view_2_inputs,
        outputs: view_2_outputs,
        asserts: vec![is_at_view_number(2)],
    };

    let consensus_state = ConsensusTaskState::<
        TestTypes,
        MemoryImpl,
        SystemContextHandle<TestTypes, MemoryImpl>,
    >::create_from(&handle)
    .await;

    inject_consensus_polls(&consensus_state).await;
    run_test_script(vec![view_1, view_2], consensus_state).await;
}

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_consensus_vote_with_permuted_dac() {
    // These tests verify that a vote is indeed sent no matter when it receives a DACRecv
    // event. In particular, we want to verify that receiving events in an unexpected (but still
    // valid) order allows the system to proceed as it normally would.
    test_vote_with_specific_order(vec![0, 1, 2]).await;
    test_vote_with_specific_order(vec![0, 2, 1]).await;
    test_vote_with_specific_order(vec![1, 0, 2]).await;
    test_vote_with_specific_order(vec![2, 0, 1]).await;
    test_vote_with_specific_order(vec![1, 2, 0]).await;
    test_vote_with_specific_order(vec![2, 1, 0]).await;
}

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_view_sync_finalize_propose() {
    use hotshot::tasks::{inject_consensus_polls, task_state::CreateTaskState};
    use hotshot_task_impls::{consensus::ConsensusTaskState, events::HotShotEvent::*};
    use hotshot_testing::{
        predicates::{exact, is_at_view_number, quorum_proposal_send, timeout_vote_send},
        script::{run_test_script, TestScriptStage},
        task_helpers::{build_system_handle, vid_scheme_from_view_number},
        view_generator::TestViewGenerator,
    };
    use hotshot_types::simple_vote::{TimeoutData, TimeoutVote, ViewSyncFinalizeData};

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(4).await.0;
    let (priv_key, pub_key) = key_pair_for_id(4);
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    // Make some empty encoded transactions, we just care about having a commitment handy for the
    // later calls. We need the VID commitment to be able to propose later.
    let mut vid = vid_scheme_from_view_number::<TestTypes>(&quorum_membership, ViewNumber::new(4));
    let encoded_transactions = Vec::new();
    let vid_disperse = vid.disperse(&encoded_transactions).unwrap();
    let payload_commitment = vid_disperse.commit;

    let view_sync_finalize_data: ViewSyncFinalizeData<TestTypes> = ViewSyncFinalizeData {
        relay: 4,
        round: ViewNumber::new(4),
    };

    let mut generator = TestViewGenerator::generate(quorum_membership.clone());
    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut votes = Vec::new();
    let mut vids = Vec::new();
    let mut dacs = Vec::new();

    generator.next();
    let view = generator.current_view.clone().unwrap();
    proposals.push(view.quorum_proposal.clone());
    leaders.push(view.leader_public_key);
    votes.push(view.create_quorum_vote(&handle));
    vids.push(view.vid_proposal.clone());
    dacs.push(view.da_certificate.clone());

    // Skip two views
    generator.advance_view_number_by(2);

    // Initiate a view sync finalize
    generator.add_view_sync_finalize(view_sync_finalize_data);

    // Build the next proposal from view 1
    generator.next_from_anscestor_view(view.clone());
    let view = generator.current_view.unwrap();
    proposals.push(view.quorum_proposal.clone());
    leaders.push(view.leader_public_key);
    votes.push(view.create_quorum_vote(&handle));

    // This is a bog standard view and covers the situation where everything is going normally.
    let view_1 = TestScriptStage {
        inputs: vec![
            QuorumProposalRecv(proposals[0].clone(), leaders[0]),
            DACRecv(dacs[0].clone()),
            VidDisperseRecv(vids[0].0.clone()),
        ],
        outputs: vec![
            exact(ViewChange(ViewNumber::new(1))),
            exact(QuorumProposalValidated(proposals[0].data.clone())),
            exact(QuorumVoteSend(votes[0].clone())),
        ],
        asserts: vec![is_at_view_number(1)],
    };

    // Fail twice here to "trigger" a view sync event. This is accomplished above by advancing the
    // view number in the generator.
    let view_2_3 = TestScriptStage {
        inputs: vec![Timeout(ViewNumber::new(2)), Timeout(ViewNumber::new(3))],
        outputs: vec![timeout_vote_send(), timeout_vote_send()],
        // Times out, so we now have a delayed view
        asserts: vec![is_at_view_number(1)],
    };

    // Handle the view sync finalize cert, get the requisite data, propose.
    let cert = proposals[1].data.view_sync_certificate.clone().unwrap();

    // Generate the timeout votes for the timeouts that just occurred.
    let timeout_vote_view_2 = TimeoutVote::create_signed_vote(
        TimeoutData {
            view: ViewNumber::new(2),
        },
        ViewNumber::new(2),
        &pub_key,
        &priv_key,
    )
    .unwrap();

    let timeout_vote_view_3 = TimeoutVote::create_signed_vote(
        TimeoutData {
            view: ViewNumber::new(3),
        },
        ViewNumber::new(3),
        &pub_key,
        &priv_key,
    )
    .unwrap();

    let view_4 = TestScriptStage {
        inputs: vec![
            TimeoutVoteRecv(timeout_vote_view_2),
            TimeoutVoteRecv(timeout_vote_view_3),
            ViewSyncFinalizeCertificate2Recv(cert),
            QuorumProposalRecv(proposals[1].clone(), leaders[1]),
            SendPayloadCommitmentAndMetadata(payload_commitment, (), ViewNumber::new(4)),
        ],
        outputs: vec![
            exact(ViewChange(ViewNumber::new(4))),
            exact(QuorumProposalValidated(proposals[1].data.clone())),
            quorum_proposal_send(),
        ],
        asserts: vec![is_at_view_number(4)],
    };

    let consensus_state = ConsensusTaskState::<
        TestTypes,
        MemoryImpl,
        SystemContextHandle<TestTypes, MemoryImpl>,
    >::create_from(&handle)
    .await;

    let stages = vec![view_1, view_2_3, view_4];

    inject_consensus_polls(&consensus_state).await;
    run_test_script(stages, consensus_state).await;
}

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
/// Makes sure that, when a valid ViewSyncFinalize certificate is available, the consensus task
/// will indeed vote if the cert is valid and matches the correct view number.
async fn test_view_sync_finalize_vote() {
    use hotshot_testing::predicates::timeout_vote_send;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(5).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();

    let view_sync_finalize_data: ViewSyncFinalizeData<TestTypes> = ViewSyncFinalizeData {
        relay: 4,
        round: ViewNumber::new(5),
    };

    let mut generator = TestViewGenerator::generate(quorum_membership.clone());
    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut votes = Vec::new();
    let mut vids = Vec::new();
    let mut dacs = Vec::new();
    for view in (&mut generator).take(3) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        votes.push(view.create_quorum_vote(&handle));
        vids.push(view.vid_proposal.clone());
        dacs.push(view.da_certificate.clone());
    }

    // Each call to `take` moves us to the next generated view. We advance to view
    // 3 and then add the finalize cert for checking there.
    generator.add_view_sync_finalize(view_sync_finalize_data);
    for view in (&mut generator).take(1) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        votes.push(view.create_quorum_vote(&handle));
        vids.push(view.vid_proposal.clone());
        dacs.push(view.da_certificate.clone());
    }

    let view_1 = TestScriptStage {
        inputs: vec![
            QuorumProposalRecv(proposals[0].clone(), leaders[0]),
            DACRecv(dacs[0].clone()),
            VidDisperseRecv(vids[0].0.clone()),
        ],
        outputs: vec![
            exact(ViewChange(ViewNumber::new(1))),
            exact(QuorumProposalValidated(proposals[0].data.clone())),
            exact(QuorumVoteSend(votes[0].clone())),
        ],
        asserts: vec![is_at_view_number(1)],
    };

    let view_2 = TestScriptStage {
        inputs: vec![Timeout(ViewNumber::new(2)), Timeout(ViewNumber::new(3))],
        outputs: vec![timeout_vote_send(), timeout_vote_send()],
        // Times out, so we now have a delayed view
        asserts: vec![is_at_view_number(1)],
    };

    // Now we're on the latest view. We want to set the quorum
    // certificate to be the previous highest QC (before the timeouts). This will be distinct from
    // the view sync cert, which is saying "hey, I'm _actually_ at view 4, but my highest QC is
    // only for view 1." This forces the QC to be for view 1, and we can move on under this
    // assumption.

    // Try to view sync at view 4.
    let view_sync_cert = proposals[3].data.view_sync_certificate.clone().unwrap();

    // Highest qc so far is actually from view 1, so re-assign proposal 0 to the slot of proposal
    // 3.
    proposals[0].data.proposer_id = proposals[3].data.proposer_id;

    // Now at view 3 we receive the proposal received response.
    let view_3 = TestScriptStage {
        inputs: vec![
            // Multiple timeouts in a row, so we call for a view sync
            ViewSyncFinalizeCertificate2Recv(view_sync_cert),
            // Receive a proposal for view 4, but with the highest qc being from view 1.
            QuorumProposalRecv(proposals[0].clone(), leaders[0]),
        ],
        outputs: vec![
            exact(QuorumProposalValidated(proposals[0].data.clone())),
            quorum_vote_send(),
        ],
        asserts: vec![],
    };

    let consensus_state = ConsensusTaskState::<
        TestTypes,
        MemoryImpl,
        SystemContextHandle<TestTypes, MemoryImpl>,
    >::create_from(&handle)
    .await;

    let stages = vec![view_1, view_2, view_3];

    inject_consensus_polls(&consensus_state).await;
    run_test_script(stages, consensus_state).await;
}

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
/// Makes sure that, when a valid ViewSyncFinalize certificate is available, the consensus task
/// will NOT vote when the certificate matches a different view number.
async fn test_view_sync_finalize_vote_fail_view_number() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(5).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();

    let view_sync_finalize_data: ViewSyncFinalizeData<TestTypes> = ViewSyncFinalizeData {
        relay: 10,
        round: ViewNumber::new(10),
    };

    let mut generator = TestViewGenerator::generate(quorum_membership.clone());
    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut votes = Vec::new();
    let mut vids = Vec::new();
    let mut dacs = Vec::new();
    for view in (&mut generator).take(2) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        votes.push(view.create_quorum_vote(&handle));
        vids.push(view.vid_proposal.clone());
        dacs.push(view.da_certificate.clone());
    }

    // Each call to `take` moves us to the next generated view. We advance to view
    // 3 and then add the finalize cert for checking there.
    generator.add_view_sync_finalize(view_sync_finalize_data);
    for view in (&mut generator).take(1) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        votes.push(view.create_quorum_vote(&handle));
        vids.push(view.vid_proposal.clone());
        dacs.push(view.da_certificate.clone());
    }

    let view_1 = TestScriptStage {
        inputs: vec![
            QuorumProposalRecv(proposals[0].clone(), leaders[0]),
            DACRecv(dacs[0].clone()),
            VidDisperseRecv(vids[0].0.clone()),
        ],
        outputs: vec![
            exact(ViewChange(ViewNumber::new(1))),
            exact(QuorumProposalValidated(proposals[0].data.clone())),
            exact(QuorumVoteSend(votes[0].clone())),
        ],
        asserts: vec![is_at_view_number(1)],
    };

    let view_2 = TestScriptStage {
        inputs: vec![
            VidDisperseRecv(vids[1].0.clone()),
            QuorumProposalRecv(proposals[1].clone(), leaders[1]),
            DACRecv(dacs[1].clone()),
        ],
        outputs: vec![
            exact(ViewChange(ViewNumber::new(2))),
            exact(QuorumProposalValidated(proposals[1].data.clone())),
            exact(QuorumVoteSend(votes[1].clone())),
        ],
        asserts: vec![is_at_view_number(2)],
    };

    let mut cert = proposals[2].data.view_sync_certificate.clone().unwrap();

    // Trigger the timeout cert check
    proposals[2].data.justify_qc.view_number = ViewNumber::new(1);

    // Overwrite the cert view number with something invalid to force the failure. This should
    // result in the vote NOT being sent below in the outputs.
    cert.view_number = ViewNumber::new(10);
    let view_3 = TestScriptStage {
        inputs: vec![
            ViewSyncFinalizeCertificate2Recv(cert),
            QuorumProposalRecv(proposals[2].clone(), leaders[2]),
            VidDisperseRecv(vids[2].0.clone()),
            DACRecv(dacs[2].clone()),
        ],
        outputs: vec![
            /* The entire thing dies */
        ],
        // We are unable to move to the next view.
        asserts: vec![is_at_view_number(2)],
    };

    let consensus_state = ConsensusTaskState::<
        TestTypes,
        MemoryImpl,
        SystemContextHandle<TestTypes, MemoryImpl>,
    >::create_from(&handle)
    .await;

    let stages = vec![view_1, view_2, view_3];

    inject_consensus_polls(&consensus_state).await;
    run_test_script(stages, consensus_state).await;
}

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_vid_disperse_storage_failure() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(2).await.0;

    // Set the error flag here for the system handle. This causes it to emit an error on append.
    handle.get_storage().write().await.should_return_err = true;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let mut generator = TestViewGenerator::generate(quorum_membership.clone());

    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut votes = Vec::new();
    let mut dacs = Vec::new();
    let mut vids = Vec::new();
    for view in (&mut generator).take(1) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        votes.push(view.create_quorum_vote(&handle));
        dacs.push(view.da_certificate.clone());
        vids.push(view.vid_proposal.clone());
    }

    // Run view 1 (the genesis stage).
    let view_1 = TestScriptStage {
        inputs: vec![
            QuorumProposalRecv(proposals[0].clone(), leaders[0]),
            DACRecv(dacs[0].clone()),
            VidDisperseRecv(vids[0].0.clone()),
        ],
        outputs: vec![
            exact(ViewChange(ViewNumber::new(1))),
            exact(QuorumProposalValidated(proposals[0].data.clone())),
            /* Does not vote */
        ],
        asserts: vec![is_at_view_number(1)],
    };

    let consensus_state = ConsensusTaskState::<
        TestTypes,
        MemoryImpl,
        SystemContextHandle<TestTypes, MemoryImpl>,
    >::create_from(&handle)
    .await;

    inject_consensus_polls(&consensus_state).await;

    run_test_script(vec![view_1], consensus_state).await;
}
