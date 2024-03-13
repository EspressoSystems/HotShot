#![allow(clippy::panic)]
use hotshot::tasks::{inject_consensus_polls, task_state::CreateTaskState};
use hotshot::types::SystemContextHandle;
use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
use hotshot_task_impls::events::HotShotEvent;
use hotshot_task_impls::{consensus::ConsensusTaskState, events::HotShotEvent::*};
use hotshot_testing::task_helpers::{build_quorum_proposal, build_vote, key_pair_for_id};
use hotshot_testing::test_helpers::permute_input_with_index_order;
use hotshot_testing::{
    predicates::{exact, is_at_view_number, quorum_vote_send},
    script::{run_test_script, TestScriptStage},
    task_helpers::build_system_handle,
    view_generator::TestViewGenerator,
};
use hotshot_types::simple_vote::ViewSyncFinalizeData;
use hotshot_types::traits::{consensus_api::ConsensusApi, election::Membership};
use hotshot_types::{
    data::ViewNumber, message::GeneralConsensusMessage, traits::node_implementation::ConsensusTime,
};
use jf_primitives::vid::VidScheme;
use std::collections::HashMap;

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_consensus_task() {
    use hotshot::tasks::{inject_consensus_polls, task_state::CreateTaskState};
    use hotshot_task_impls::{consensus::ConsensusTaskState, events::HotShotEvent::*};
    use hotshot_testing::{
        predicates::{exact, is_at_view_number, quorum_proposal_send},
        script::{run_test_script, TestScriptStage},
        task_helpers::{build_system_handle, vid_scheme_from_view_number},
        view_generator::TestViewGenerator,
    };

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(2).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();

    // Make some empty encoded transactions, we just care about having a commitment handy for the
    // later calls. We need the VID commitment to be able to propose later.
    let vid = vid_scheme_from_view_number::<TestTypes>(&quorum_membership, ViewNumber::new(2));
    let encoded_transactions = Vec::new();
    let vid_disperse = vid.disperse(&encoded_transactions).unwrap();
    let payload_commitment = vid_disperse.commit;

    let mut generator = TestViewGenerator::generate(quorum_membership.clone());

    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut votes = Vec::new();
    for view in (&mut generator).take(2) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        votes.push(view.create_quorum_vote(&handle));
    }

    // Run view 1 (the genesis stage).
    let view_1 = TestScriptStage {
        inputs: vec![QuorumProposalRecv(proposals[0].clone(), leaders[0])],
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
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
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
    for view in (&mut generator).take(2) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        votes.push(view.create_quorum_vote(&handle));
    }

    // Send a proposal, vote on said proposal, update view based on proposal QC, receive vote as next leader
    let view_1 = TestScriptStage {
        inputs: vec![
            QuorumProposalRecv(proposals[0].clone(), leaders[0]),
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
        inputs: vec![QuorumProposalRecv(proposals[0].clone(), leaders[0])],
        outputs: vec![
            exact(ViewChange(ViewNumber::new(1))),
            exact(QuorumProposalValidated(proposals[0].data.clone())),
            exact(QuorumVoteSend(votes[0].clone())),
        ],
        asserts: vec![is_at_view_number(1)],
    };

    let inputs = vec![
        // We need a VID share for view 2 otherwise we cannot vote at view 2 (as node 2).
        VidDisperseRecv(vids[1].0.clone(), vids[1].1),
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
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
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
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
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
    let vid = vid_scheme_from_view_number::<TestTypes>(&quorum_membership, ViewNumber::new(4));
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
    // for view in (&mut generator).take(1) {
    proposals.push(view.quorum_proposal.clone());
    leaders.push(view.leader_public_key);
    votes.push(view.create_quorum_vote(&handle));
    vids.push(view.vid_proposal.clone());
    dacs.push(view.da_certificate.clone());
    // }

    // Each call to `take` moves us to the next generated view. We advance to view
    // 3 and then add the finalize cert for checking there.
    // Skip two views
    generator.advance_view_number_by(2);
    generator.add_view_sync_finalize(view_sync_finalize_data);
    generator.next_from_anscestor_view(view.clone());
    let view = generator.current_view.unwrap();
    // for view in (&mut generator).take(1) {
    proposals.push(view.quorum_proposal.clone());
    leaders.push(view.leader_public_key);
    votes.push(view.create_quorum_vote(&handle));
    // }

    // This is a bog standard view and covers the situation where everything is going normally.
    let view_1 = TestScriptStage {
        inputs: vec![QuorumProposalRecv(proposals[0].clone(), leaders[0])],
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
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
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
        inputs: vec![QuorumProposalRecv(proposals[0].clone(), leaders[0])],
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
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
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
        inputs: vec![QuorumProposalRecv(proposals[0].clone(), leaders[0])],
        outputs: vec![
            exact(ViewChange(ViewNumber::new(1))),
            exact(QuorumProposalValidated(proposals[0].data.clone())),
            exact(QuorumVoteSend(votes[0].clone())),
        ],
        asserts: vec![is_at_view_number(1)],
    };

    let view_2 = TestScriptStage {
        inputs: vec![
            VidDisperseRecv(vids[1].0.clone(), vids[1].1),
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
            VidDisperseRecv(vids[2].0.clone(), vids[2].1),
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

/// TODO (jparr721): Nuke these old tests. Tracking: https://github.com/EspressoSystems/HotShot/issues/2727
#[cfg(test)]
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_consensus_task_old() {
    use hotshot::tasks::{inject_consensus_polls, task_state::CreateTaskState};
    use hotshot_task_impls::{consensus::ConsensusTaskState, harness::run_harness};
    use hotshot_testing::task_helpers::build_system_handle;
    use hotshot_types::simple_certificate::QuorumCertificate;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(1).await.0;
    // We assign node's key pair rather than read from config file since it's a test
    let (private_key, public_key) = key_pair_for_id(1);

    let mut input = Vec::new();
    let mut output = HashMap::new();

    // Trigger a proposal to send by creating a new QC.  Then recieve that proposal and update view based on the valid QC in the proposal
    let qc = QuorumCertificate::<TestTypes>::genesis();
    let proposal = build_quorum_proposal(&handle, &private_key, 1).await;

    input.push(HotShotEvent::QCFormed(either::Left(qc.clone())));
    input.push(HotShotEvent::QuorumProposalRecv(
        proposal.clone(),
        public_key,
    ));

    input.push(HotShotEvent::Shutdown);

    output.insert(
        HotShotEvent::QuorumProposalSend(proposal.clone(), public_key),
        1,
    );
    output.insert(
        HotShotEvent::QuorumProposalValidated(proposal.data.clone()),
        1,
    );

    output.insert(HotShotEvent::ViewChange(ViewNumber::new(1)), 1);

    if let GeneralConsensusMessage::Vote(vote) = build_vote(&handle, proposal.data).await {
        output.insert(HotShotEvent::QuorumVoteSend(vote.clone()), 1);
        input.push(HotShotEvent::QuorumVoteRecv(vote.clone()));
    }

    let consensus_state = ConsensusTaskState::<
        TestTypes,
        MemoryImpl,
        SystemContextHandle<TestTypes, MemoryImpl>,
    >::create_from(&handle)
    .await;

    inject_consensus_polls(&consensus_state).await;

    run_harness(input, output, consensus_state, false).await;
}

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_consensus_vote_old() {
    use hotshot::tasks::{inject_consensus_polls, task_state::CreateTaskState};
    use hotshot_task_impls::{consensus::ConsensusTaskState, harness::run_harness};
    use hotshot_testing::task_helpers::build_system_handle;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(2).await.0;
    // We assign node's key pair rather than read from config file since it's a test
    let (private_key, public_key) = key_pair_for_id(1);

    let mut input = Vec::new();
    let mut output = HashMap::new();

    let proposal = build_quorum_proposal(&handle, &private_key, 1).await;

    // Send a proposal, vote on said proposal, update view based on proposal QC, receive vote as next leader
    input.push(HotShotEvent::QuorumProposalRecv(
        proposal.clone(),
        public_key,
    ));

    let proposal = proposal.data;
    output.insert(HotShotEvent::QuorumProposalValidated(proposal.clone()), 1);
    if let GeneralConsensusMessage::Vote(vote) = build_vote(&handle, proposal).await {
        output.insert(HotShotEvent::QuorumVoteSend(vote.clone()), 1);
        input.push(HotShotEvent::QuorumVoteRecv(vote.clone()));
    }

    output.insert(HotShotEvent::ViewChange(ViewNumber::new(1)), 1);

    input.push(HotShotEvent::Shutdown);

    let consensus_state = ConsensusTaskState::<
        TestTypes,
        MemoryImpl,
        SystemContextHandle<TestTypes, MemoryImpl>,
    >::create_from(&handle)
    .await;

    inject_consensus_polls(&consensus_state).await;

    run_harness(input, output, consensus_state, false).await;
}

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
// TODO: re-enable this when HotShot/the sequencer needs the shares for something
// issue: https://github.com/EspressoSystems/HotShot/issues/2236
#[ignore]
async fn test_consensus_with_vid_old() {
    use hotshot::tasks::{inject_consensus_polls, task_state::CreateTaskState};
    use hotshot::traits::BlockPayload;
    use hotshot::types::SignatureKey;
    use hotshot_example_types::block_types::TestBlockPayload;
    use hotshot_example_types::block_types::TestTransaction;
    use hotshot_task_impls::{consensus::ConsensusTaskState, harness::run_harness};
    use hotshot_testing::task_helpers::build_cert;
    use hotshot_testing::task_helpers::build_system_handle;
    use hotshot_testing::task_helpers::vid_scheme_from_view_number;
    use hotshot_types::simple_certificate::DACertificate;
    use hotshot_types::simple_vote::DAData;
    use hotshot_types::simple_vote::DAVote;
    use hotshot_types::traits::block_contents::{vid_commitment, TestableBlock};
    use hotshot_types::{
        data::VidDisperse, message::Proposal, traits::node_implementation::NodeType,
    };
    use std::marker::PhantomData;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let (handle, _tx, _rx) = build_system_handle(2).await;
    // We assign node's key pair rather than read from config file since it's a test
    // In view 2, node 2 is the leader.
    let (private_key_view2, public_key_view2) = key_pair_for_id(2);

    // For the test of vote logic with vid
    let pub_key = *handle.public_key();
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let vid = vid_scheme_from_view_number::<TestTypes>(&quorum_membership, ViewNumber::new(2));
    let transactions = vec![TestTransaction(vec![0])];
    let encoded_transactions = TestTransaction::encode(transactions.clone()).unwrap();
    let vid_disperse = vid.disperse(&encoded_transactions).unwrap();
    let payload_commitment = vid_disperse.commit;

    let vid_signature = <TestTypes as NodeType>::SignatureKey::sign(
        handle.private_key(),
        payload_commitment.as_ref(),
    )
    .expect("Failed to sign payload commitment");
    let vid_disperse = vid.disperse(&encoded_transactions).unwrap();
    let vid_disperse_inner = VidDisperse::from_membership(
        ViewNumber::new(2),
        vid_disperse,
        &quorum_membership.clone().into(),
    );
    // TODO for now reuse the same block payload commitment and signature as DA committee
    // https://github.com/EspressoSystems/jellyfish/issues/369
    let vid_proposal = Proposal {
        data: vid_disperse_inner.clone(),
        signature: vid_signature,
        _pd: PhantomData,
    };

    let mut input = Vec::new();
    let mut output = HashMap::new();

    // Do a view change, so that it's not the genesis view, and vid vote is needed
    input.push(HotShotEvent::ViewChange(ViewNumber::new(1)));

    // For the test of vote logic with vid, starting view 2 we need vid share
    input.push(HotShotEvent::ViewChange(ViewNumber::new(2)));
    let proposal_view2 = build_quorum_proposal(&handle, &private_key_view2, 2).await;
    let block = <TestBlockPayload as TestableBlock>::genesis();
    let da_payload_commitment = vid_commitment(
        &block.encode().unwrap().collect(),
        quorum_membership.total_nodes(),
    );
    let da_data = DAData {
        payload_commit: da_payload_commitment,
    };
    let created_dac_view2 =
        build_cert::<TestTypes, DAData, DAVote<TestTypes>, DACertificate<TestTypes>>(
            da_data,
            &quorum_membership,
            ViewNumber::new(2),
            &public_key_view2,
            &private_key_view2,
        );
    input.push(HotShotEvent::DACRecv(created_dac_view2.clone()));
    input.push(HotShotEvent::VidDisperseRecv(vid_proposal.clone(), pub_key));

    // Send a proposal, vote on said proposal, update view based on proposal QC, receive vote as next leader
    input.push(HotShotEvent::QuorumProposalRecv(
        proposal_view2.clone(),
        public_key_view2,
    ));

    if let GeneralConsensusMessage::Vote(vote) = build_vote(&handle, proposal_view2.data).await {
        output.insert(HotShotEvent::QuorumVoteSend(vote.clone()), 1);
    }

    output.insert(HotShotEvent::ViewChange(ViewNumber::new(1)), 1);
    output.insert(HotShotEvent::ViewChange(ViewNumber::new(2)), 1);

    input.push(HotShotEvent::Shutdown);
    output.insert(HotShotEvent::Shutdown, 1);

    let consensus_state = ConsensusTaskState::<
        TestTypes,
        MemoryImpl,
        SystemContextHandle<TestTypes, MemoryImpl>,
    >::create_from(&handle)
    .await;

    inject_consensus_polls(&consensus_state).await;

    run_harness(input, output, consensus_state, false).await;
}
