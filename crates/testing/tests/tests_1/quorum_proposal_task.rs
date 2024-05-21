#![cfg(feature = "dependency-tasks")]

use committable::Committable;
use hotshot::tasks::task_state::CreateTaskState;
use hotshot_example_types::{
    block_types::TestMetadata,
    node_types::{MemoryImpl, TestTypes},
    state_types::{TestInstanceState, TestValidatedState},
};
use hotshot_task_impls::{events::HotShotEvent::*, quorum_proposal::QuorumProposalTaskState};
use hotshot_testing::{
    predicates::event::{exact, leaf_decided, quorum_proposal_send},
    script::{run_test_script, TestScriptStage},
    task_helpers::{build_cert, key_pair_for_id, vid_share},
    task_helpers::{build_system_handle, vid_scheme_from_view_number},
    view_generator::TestViewGenerator,
};
use hotshot_types::{
    consensus::{CommitmentAndMetadata, ProposalDependencyData},
    data::{null_block, Leaf, ViewChangeEvidence, ViewNumber},
    simple_certificate::{TimeoutCertificate, ViewSyncFinalizeCertificate2},
    simple_vote::{TimeoutData, TimeoutVote, ViewSyncFinalizeData, ViewSyncFinalizeVote},
    traits::{
        election::Membership,
        node_implementation::{ConsensusTime, NodeType},
    },
    utils::{BuilderCommitment, View, ViewInner},
    vid::VidSchemeType,
};
use jf_vid::VidScheme;
use sha2::Digest;

fn make_payload_commitment(
    membership: &<TestTypes as NodeType>::Membership,
    view: ViewNumber,
) -> <VidSchemeType as VidScheme>::Commit {
    // Make some empty encoded transactions, we just care about having a commitment handy for the
    // later calls. We need the VID commitment to be able to propose later.
    let mut vid = vid_scheme_from_view_number::<TestTypes>(membership, view);
    let encoded_transactions = Vec::new();
    vid.commit_only(&encoded_transactions).unwrap()
}

fn create_fake_view_with_leaf(leaf: Leaf<TestTypes>) -> View<TestTypes> {
    View {
        view_inner: ViewInner::Leaf {
            leaf: leaf.commit(),
            state: TestValidatedState::default().into(),
            delta: None,
        },
    }
}

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_quorum_proposal_task_quorum_proposal_view_1() {
    use hotshot_types::simple_certificate::QuorumCertificate;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let node_id = 1;
    let handle = build_system_handle(node_id).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let da_membership = handle.hotshot.memberships.da_membership.clone();

    let payload_commitment = make_payload_commitment(&quorum_membership, ViewNumber::new(node_id));

    let mut generator = TestViewGenerator::generate(quorum_membership.clone(), da_membership);

    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut leaves = Vec::new();
    let mut vids = Vec::new();
    let consensus = handle.hotshot.consensus();
    let mut consensus_writer = consensus.write().await;
    for view in (&mut generator).take(2) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        leaves.push(view.leaf.clone());
        vids.push(view.vid_proposal.clone());

        // We don't have a `QuorumProposalRecv` task handler, so we'll just manually insert the proposals
        // to make sure they show up during tests.
        consensus_writer
            .update_saved_leaves(Leaf::from_quorum_proposal(&view.quorum_proposal.data));
    }
    drop(consensus_writer);

    // We must send the genesis cert here to initialize hotshot successfully.
    let genesis_cert = QuorumCertificate::genesis(&*handle.hotshot.instance_state());
    let genesis_leaf = Leaf::genesis(&*handle.hotshot.instance_state());
    let builder_commitment = BuilderCommitment::from_raw_digest(sha2::Sha256::new().finalize());

    let view = TestScriptStage {
        inputs: vec![
            QcFormed(either::Left(genesis_cert.clone())),
            SendPayloadCommitmentAndMetadata(
                payload_commitment,
                builder_commitment,
                TestMetadata,
                ViewNumber::new(1),
                null_block::builder_fee(quorum_membership.total_nodes(), &TestInstanceState {})
                    .unwrap(),
            ),
            VidShareValidated(vid_share(&vids[0].0, handle.public_key())),
            ValidatedStateUpdated(ViewNumber::new(0), create_fake_view_with_leaf(genesis_leaf)),
        ],
        outputs: vec![
            exact(UpdateHighQc(genesis_cert.clone())),
            quorum_proposal_send(),
        ],
        asserts: vec![],
    };

    let quorum_proposal_task_state =
        QuorumProposalTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;

    let script = vec![view];
    run_test_script(script, quorum_proposal_task_state).await;
}

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_quorum_proposal_task_quorum_proposal_view_gt_1() {
    use hotshot_types::vote::HasViewNumber;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let node_id = 3;
    let handle = build_system_handle(node_id).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let da_membership = handle.hotshot.memberships.da_membership.clone();

    let mut generator = TestViewGenerator::generate(quorum_membership.clone(), da_membership);

    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut leaves = Vec::new();
    let mut vids = Vec::new();
    let consensus = handle.hotshot.consensus();
    let mut consensus_writer = consensus.write().await;
    for view in (&mut generator).take(5) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        leaves.push(view.leaf.clone());
        vids.push(view.vid_proposal.clone());

        // We don't have a `QuorumProposalRecv` task handler, so we'll just manually insert the proposals
        // to make sure they show up during tests.
        consensus_writer
            .update_saved_leaves(Leaf::from_quorum_proposal(&view.quorum_proposal.data));
    }
    drop(consensus_writer);

    let builder_commitment = BuilderCommitment::from_raw_digest(sha2::Sha256::new().finalize());

    // We need to handle the views where we aren't the leader to ensure that the states are
    // updated properly.

    let genesis_cert = proposals[0].data.justify_qc.clone();
    let genesis_leaf = Leaf::genesis(&*handle.hotshot.instance_state());

    let genesis_view = TestScriptStage {
        inputs: vec![
            QcFormed(either::Left(genesis_cert.clone())),
            SendPayloadCommitmentAndMetadata(
                make_payload_commitment(&quorum_membership, ViewNumber::new(1)),
                builder_commitment.clone(),
                TestMetadata,
                ViewNumber::new(1),
                null_block::builder_fee(quorum_membership.total_nodes(), &TestInstanceState {})
                    .unwrap(),
            ),
            VidShareValidated(vid_share(&vids[0].0, handle.public_key())),
            ValidatedStateUpdated(
                ViewNumber::new(0),
                create_fake_view_with_leaf(genesis_leaf.clone()),
            ),
        ],
        outputs: vec![exact(UpdateHighQc(genesis_cert.clone()))],
        asserts: vec![],
    };

    // We send all the events that we'd have otherwise received to ensure the states are updated.
    let view_1 = TestScriptStage {
        inputs: vec![
            QuorumProposalValidated(proposals[0].data.clone(), leaves[0].clone()),
            QcFormed(either::Left(proposals[1].data.justify_qc.clone())),
            SendPayloadCommitmentAndMetadata(
                make_payload_commitment(&quorum_membership, ViewNumber::new(2)),
                builder_commitment.clone(),
                TestMetadata,
                ViewNumber::new(2),
                null_block::builder_fee(quorum_membership.total_nodes(), &TestInstanceState {})
                    .unwrap(),
            ),
            VidShareValidated(vid_share(&vids[1].0, handle.public_key())),
            ValidatedStateUpdated(
                proposals[0].data.view_number(),
                create_fake_view_with_leaf(leaves[0].clone()),
            ),
        ],
        outputs: vec![exact(UpdateHighQc(proposals[1].data.justify_qc.clone()))],
        asserts: vec![],
    };

    // Proposing for this view since we've received a proposal for view 2.
    let view_2 = TestScriptStage {
        inputs: vec![
            QuorumProposalValidated(proposals[1].data.clone(), leaves[1].clone()),
            QcFormed(either::Left(proposals[2].data.justify_qc.clone())),
            SendPayloadCommitmentAndMetadata(
                make_payload_commitment(&quorum_membership, ViewNumber::new(3)),
                builder_commitment.clone(),
                TestMetadata,
                ViewNumber::new(3),
                null_block::builder_fee(quorum_membership.total_nodes(), &TestInstanceState {})
                    .unwrap(),
            ),
            VidShareValidated(vid_share(&vids[2].0, handle.public_key())),
            ValidatedStateUpdated(
                proposals[1].data.view_number(),
                create_fake_view_with_leaf(leaves[1].clone()),
            ),
        ],
        outputs: vec![
            exact(LockedViewUpdated(ViewNumber::new(1))),
            exact(UpdateHighQc(proposals[2].data.justify_qc.clone())),
            quorum_proposal_send(),
        ],
        asserts: vec![],
    };

    // Now, let's verify that we get the decide on the 3-chain.
    let view_3 = TestScriptStage {
        inputs: vec![
            QuorumProposalValidated(proposals[2].data.clone(), leaves[2].clone()),
            QcFormed(either::Left(proposals[3].data.justify_qc.clone())),
            SendPayloadCommitmentAndMetadata(
                make_payload_commitment(&quorum_membership, ViewNumber::new(4)),
                builder_commitment.clone(),
                TestMetadata,
                ViewNumber::new(4),
                null_block::builder_fee(quorum_membership.total_nodes(), &TestInstanceState {})
                    .unwrap(),
            ),
            VidShareValidated(vid_share(&vids[3].0, handle.public_key())),
            ValidatedStateUpdated(
                proposals[2].data.view_number(),
                create_fake_view_with_leaf(leaves[2].clone()),
            ),
        ],
        outputs: vec![exact(UpdateHighQc(proposals[3].data.justify_qc.clone()))],
        asserts: vec![],
    };

    let view_4 = TestScriptStage {
        inputs: vec![
            QuorumProposalValidated(proposals[3].data.clone(), leaves[3].clone()),
            QcFormed(either::Left(proposals[4].data.justify_qc.clone())),
            SendPayloadCommitmentAndMetadata(
                make_payload_commitment(&quorum_membership, ViewNumber::new(5)),
                builder_commitment,
                TestMetadata,
                ViewNumber::new(5),
                null_block::builder_fee(quorum_membership.total_nodes(), &TestInstanceState {})
                    .unwrap(),
            ),
            VidShareValidated(vid_share(&vids[4].0, handle.public_key())),
            ValidatedStateUpdated(
                proposals[3].data.view_number(),
                create_fake_view_with_leaf(leaves[3].clone()),
            ),
        ],
        outputs: vec![
            exact(LockedViewUpdated(ViewNumber::new(3))),
            exact(LastDecidedViewUpdated(ViewNumber::new(2))),
            leaf_decided(),
            exact(UpdateHighQc(proposals[4].data.justify_qc.clone())),
        ],
        asserts: vec![],
    };

    let quorum_proposal_task_state =
        QuorumProposalTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;

    let script = vec![genesis_view, view_1, view_2, view_3, view_4];
    run_test_script(script, quorum_proposal_task_state).await;
}

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_quorum_proposal_task_qc_timeout() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let node_id = 3;
    let handle = build_system_handle(node_id).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let da_membership = handle.hotshot.memberships.da_membership.clone();

    let payload_commitment = make_payload_commitment(&quorum_membership, ViewNumber::new(node_id));
    let builder_commitment = BuilderCommitment::from_raw_digest(sha2::Sha256::new().finalize());

    let mut generator = TestViewGenerator::generate(quorum_membership.clone(), da_membership);

    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut vids = Vec::new();
    let mut leaves = Vec::new();
    for view in (&mut generator).take(1) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        vids.push(view.vid_proposal.clone());
        leaves.push(view.leaf.clone());
    }
    let timeout_data = TimeoutData {
        view: ViewNumber::new(1),
    };
    generator.add_timeout(timeout_data);
    for view in (&mut generator).take(2) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        vids.push(view.vid_proposal.clone());
        leaves.push(view.leaf.clone());
    }

    // Get the proposal cert out for the view sync input
    let cert = match proposals[1].data.proposal_certificate.clone().unwrap() {
        ViewChangeEvidence::Timeout(tc) => tc,
        _ => panic!("Found a View Sync Cert when there should have been a Timeout cert"),
    };

    // Run at view 2, propose at view 3.
    let view_2 = TestScriptStage {
        inputs: vec![
            QcFormed(either::Right(cert.clone())),
            SendPayloadCommitmentAndMetadata(
                payload_commitment,
                builder_commitment,
                TestMetadata,
                ViewNumber::new(3),
                null_block::builder_fee(quorum_membership.total_nodes(), &TestInstanceState {})
                    .unwrap(),
            ),
            VidShareValidated(vid_share(&vids[2].0.clone(), handle.public_key())),
            ValidatedStateUpdated(
                ViewNumber::new(2),
                create_fake_view_with_leaf(leaves[1].clone()),
            ),
        ],
        outputs: vec![quorum_proposal_send()],
        asserts: vec![],
    };

    let quorum_proposal_task_state =
        QuorumProposalTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;

    let script = vec![view_2];
    run_test_script(script, quorum_proposal_task_state).await;
}

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_quorum_proposal_task_view_sync() {
    use hotshot_example_types::block_types::TestMetadata;
    use hotshot_types::data::null_block;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    // We need to propose as the leader for view 2, otherwise we get caught up with the special
    // case in the genesis view.
    let node_id = 2;
    let handle = build_system_handle(node_id).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let da_membership = handle.hotshot.memberships.da_membership.clone();

    let payload_commitment = make_payload_commitment(&quorum_membership, ViewNumber::new(node_id));
    let builder_commitment = BuilderCommitment::from_raw_digest(sha2::Sha256::new().finalize());

    let mut generator = TestViewGenerator::generate(quorum_membership.clone(), da_membership);

    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut vids = Vec::new();
    let mut leaves = Vec::new();
    for view in (&mut generator).take(1) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        vids.push(view.vid_proposal.clone());
        leaves.push(view.leaf.clone());
    }

    let view_sync_finalize_data = ViewSyncFinalizeData {
        relay: 2,
        round: ViewNumber::new(node_id),
    };
    generator.add_view_sync_finalize(view_sync_finalize_data);
    for view in (&mut generator).take(2) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        vids.push(view.vid_proposal.clone());
        leaves.push(view.leaf.clone());
    }

    // Get the proposal cert out for the view sync input
    let cert = match proposals[1].data.proposal_certificate.clone().unwrap() {
        ViewChangeEvidence::ViewSync(vsc) => vsc,
        _ => panic!("Found a TC when there should have been a view sync cert"),
    };

    // Run at view 2, the quorum vote task shouldn't care as long as the bookkeeping is correct
    let view_2 = TestScriptStage {
        inputs: vec![
            ViewSyncFinalizeCertificate2Recv(cert.clone()),
            SendPayloadCommitmentAndMetadata(
                payload_commitment,
                builder_commitment,
                TestMetadata,
                ViewNumber::new(2),
                null_block::builder_fee(quorum_membership.total_nodes(), &TestInstanceState {})
                    .unwrap(),
            ),
            VidShareValidated(vid_share(&vids[1].0.clone(), handle.public_key())),
            ValidatedStateUpdated(
                ViewNumber::new(1),
                create_fake_view_with_leaf(leaves[1].clone()),
            ),
        ],
        outputs: vec![quorum_proposal_send()],
        asserts: vec![],
    };

    let quorum_proposal_task_state =
        QuorumProposalTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;

    let script = vec![view_2];
    run_test_script(script, quorum_proposal_task_state).await;
}

#[ignore]
#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_quorum_proposal_task_propose_now() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let node_id = 2;
    let handle = build_system_handle(node_id).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let da_membership = handle.hotshot.memberships.da_membership.clone();

    let payload_commitment = make_payload_commitment(&quorum_membership, ViewNumber::new(node_id));

    let mut generator = TestViewGenerator::generate(quorum_membership.clone(), da_membership);

    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut vids = Vec::new();
    for view in (&mut generator).take(1) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        vids.push(view.vid_proposal.clone());
    }
    for view in (&mut generator).take(1) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        vids.push(view.vid_proposal.clone());
    }

    let builder_commitment = BuilderCommitment::from_raw_digest(sha2::Sha256::new().finalize());
    // proposal dependency data - quorum proposal and cert
    let pdd_qp = ProposalDependencyData {
        commitment_and_metadata: CommitmentAndMetadata {
            commitment: payload_commitment,
            builder_commitment: builder_commitment.clone(),
            metadata: TestMetadata,
            fee: null_block::builder_fee(quorum_membership.total_nodes(), &TestInstanceState {})
                .unwrap(),
            block_view: ViewNumber::new(2),
        },
        secondary_proposal_information:
            hotshot_types::consensus::SecondaryProposalInformation::QuorumProposalAndCertificate(
                proposals[1].data.clone(),
                proposals[1].data.justify_qc.clone(),
            ),
    };

    let view_qp = TestScriptStage {
        inputs: vec![ProposeNow(ViewNumber::new(node_id), pdd_qp)],
        outputs: vec![quorum_proposal_send()],
        asserts: vec![],
    };

    let quorum_proposal_task_state =
        QuorumProposalTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;

    let script = vec![view_qp];
    run_test_script(script, quorum_proposal_task_state).await;
}

#[ignore]
#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_quorum_proposal_task_propose_now_timeout() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let node_id = 2;
    let handle = build_system_handle(node_id).await.0;
    let (private_key, public_key) = key_pair_for_id(node_id);
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let da_membership = handle.hotshot.memberships.da_membership.clone();

    let payload_commitment = make_payload_commitment(&quorum_membership, ViewNumber::new(node_id));

    let mut generator = TestViewGenerator::generate(quorum_membership.clone(), da_membership);

    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut vids = Vec::new();
    for view in (&mut generator).take(1) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        vids.push(view.vid_proposal.clone());
    }
    for view in (&mut generator).take(1) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        vids.push(view.vid_proposal.clone());
    }

    let builder_commitment = BuilderCommitment::from_raw_digest(sha2::Sha256::new().finalize());
    // proposal dependency data - timeout cert
    let pdd_timeout = ProposalDependencyData {
        commitment_and_metadata: CommitmentAndMetadata {
            commitment: payload_commitment,
            builder_commitment: builder_commitment.clone(),
            metadata: TestMetadata,
            fee: null_block::builder_fee(quorum_membership.total_nodes(), &TestInstanceState {})
                .unwrap(),
            block_view: ViewNumber::new(2),
        },
        secondary_proposal_information:
            hotshot_types::consensus::SecondaryProposalInformation::Timeout(build_cert::<
                TestTypes,
                TimeoutData<TestTypes>,
                TimeoutVote<TestTypes>,
                TimeoutCertificate<TestTypes>,
            >(
                TimeoutData {
                    view: ViewNumber::new(1),
                },
                &quorum_membership,
                ViewNumber::new(2),
                &public_key,
                &private_key,
            )),
    };

    let view_timeout = TestScriptStage {
        inputs: vec![ProposeNow(ViewNumber::new(node_id), pdd_timeout)],
        outputs: vec![quorum_proposal_send()],
        asserts: vec![],
    };

    let quorum_proposal_task_state =
        QuorumProposalTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;

    let script = vec![view_timeout];
    run_test_script(script, quorum_proposal_task_state).await;
}

#[ignore]
#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_quorum_proposal_task_propose_now_view_sync() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let node_id = 2;
    let handle = build_system_handle(node_id).await.0;
    let (private_key, public_key) = key_pair_for_id(node_id);
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let da_membership = handle.hotshot.memberships.da_membership.clone();

    let payload_commitment = make_payload_commitment(&quorum_membership, ViewNumber::new(node_id));

    let mut generator = TestViewGenerator::generate(quorum_membership.clone(), da_membership);

    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut vids = Vec::new();
    for view in (&mut generator).take(1) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        vids.push(view.vid_proposal.clone());
    }
    for view in (&mut generator).take(1) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        vids.push(view.vid_proposal.clone());
    }

    let builder_commitment = BuilderCommitment::from_raw_digest(sha2::Sha256::new().finalize());

    // proposal dependency data - view sync cert
    let pdd_view_sync = ProposalDependencyData {
        commitment_and_metadata: CommitmentAndMetadata {
            commitment: payload_commitment,
            builder_commitment,
            metadata: TestMetadata,
            fee: null_block::builder_fee(quorum_membership.total_nodes(), &TestInstanceState {})
                .unwrap(),
            block_view: ViewNumber::new(2),
        },
        secondary_proposal_information:
            hotshot_types::consensus::SecondaryProposalInformation::ViewSync(build_cert::<
                TestTypes,
                ViewSyncFinalizeData<TestTypes>,
                ViewSyncFinalizeVote<TestTypes>,
                ViewSyncFinalizeCertificate2<TestTypes>,
            >(
                ViewSyncFinalizeData {
                    relay: 1,
                    round: ViewNumber::new(1),
                },
                &quorum_membership,
                ViewNumber::new(node_id),
                &public_key,
                &private_key,
            )),
    };

    let view_view_sync = TestScriptStage {
        inputs: vec![ProposeNow(ViewNumber::new(node_id), pdd_view_sync)],
        outputs: vec![quorum_proposal_send()],
        asserts: vec![],
    };

    let quorum_proposal_task_state =
        QuorumProposalTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;

    let script = vec![view_view_sync];
    run_test_script(script, quorum_proposal_task_state).await;
}

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_quorum_proposal_task_with_incomplete_events() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    // We need to propose as the leader for view 2, otherwise we get caught up with the special
    // case in the genesis view.
    let handle = build_system_handle(2).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let da_membership = handle.hotshot.memberships.da_membership.clone();

    let mut generator = TestViewGenerator::generate(quorum_membership.clone(), da_membership);

    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut leaves = Vec::new();
    for view in (&mut generator).take(2) {
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        leaves.push(view.leaf.clone());
    }

    // We run the task here at view 2, but this time we ignore the crucial piece of evidence: the
    // payload commitment and metadata. Instead we send only one of the three "OR" required fields.
    // This should result in the proposal failing to be sent.
    let view_2 = TestScriptStage {
        inputs: vec![QuorumProposalValidated(
            proposals[1].data.clone(),
            leaves[1].clone(),
        )],
        outputs: vec![],
        asserts: vec![],
    };

    let quorum_proposal_task_state =
        QuorumProposalTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;

    let script = vec![view_2];
    run_test_script(script, quorum_proposal_task_state).await;
}
