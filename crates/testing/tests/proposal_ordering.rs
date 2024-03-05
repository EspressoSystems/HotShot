use hotshot::{tasks::task_state::CreateTaskState, types::SystemContextHandle};
use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
use hotshot_task_impls::{consensus::ConsensusTaskState, events::HotShotEvent::*};
use hotshot_testing::{
    predicates::{exact, quorum_proposal_send},
    task_helpers::vid_scheme_from_view_number,
    view_generator::TestViewGenerator,
};
use hotshot_types::{data::ViewNumber, traits::node_implementation::ConsensusTime};
use jf_primitives::vid::VidScheme;

/// Runs the test specified in this file with a boolean flag that determines whether or not to make
/// the `QCFormed` event come first in the inputs, or last. Since there's only two possible cases
/// to check, the code simply swaps the order of them in the input vector.
async fn test_ordering_with_specific_order(qc_formed_first: bool) {
    use hotshot_testing::script::{run_test_script, TestScriptStage};
    use hotshot_testing::task_helpers::build_system_handle;
    use hotshot_types::simple_certificate::QuorumCertificate;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let node_id = 1;
    let handle = build_system_handle(node_id).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();

    let vid =
        vid_scheme_from_view_number::<TestTypes>(&quorum_membership, ViewNumber::new(node_id));

    // Make some empty encoded transactions, we just care about having a commitment handy for the
    // later calls.
    let encoded_transactions = Vec::new();
    let vid_disperse = vid.disperse(&encoded_transactions).unwrap();
    let payload_commitment = vid_disperse.commit;

    let mut proposals = Vec::new();
    let mut votes = Vec::new();
    let mut leaders = Vec::new();

    let mut generator = TestViewGenerator::generate(quorum_membership.clone());
    let qc = QuorumCertificate::<TestTypes>::genesis();

    for view in (&mut generator).take(2) {
        proposals.push(view.quorum_proposal.clone());
        votes.push(view.create_vote(&handle));
        leaders.push(view.leader_public_key);
    }

    // We need a couple of events to occur for the leader to propose:
    // `SendPayloadCommitmentAndMetadata`, `QCFormed` (either QC or TC), and `QuorumProposalRecv`
    // for the previous view if a QC was formed by us for it. So, we first receive the proposal and
    // change the view to node 1, the node we are targeting as the leader.
    let view_0 = TestScriptStage {
        inputs: vec![QuorumProposalRecv(proposals[0].clone(), leaders[0])],
        outputs: vec![
            exact(ViewChange(ViewNumber::new(1))),
            exact(QuorumVoteSend(votes[0].clone())),
        ],
        asserts: vec![],
    };

    let view_1_inputs = if qc_formed_first {
        vec![
            QCFormed(either::Left(qc.clone())),
            SendPayloadCommitmentAndMetadata(payload_commitment, (), ViewNumber::new(node_id)),
        ]
    } else {
        vec![
            SendPayloadCommitmentAndMetadata(payload_commitment, (), ViewNumber::new(node_id)),
            QCFormed(either::Left(qc.clone())),
        ]
    };

    let view_1 = TestScriptStage {
        inputs: view_1_inputs,
        outputs: vec![quorum_proposal_send()],
        asserts: vec![],
    };

    let script = vec![view_0, view_1];

    let consensus_state = ConsensusTaskState::<
        TestTypes,
        MemoryImpl,
        SystemContextHandle<TestTypes, MemoryImpl>,
    >::create_from(&handle)
    .await;

    run_test_script(script, consensus_state).await;
}

#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
/// A leader node may receive one of a couple of possible events which can trigger a proposal. This
/// test ensures that, no matter what order these events are received in, the node will still
/// trigger the proposal event regardless. This is to catch a regression in which
/// `SendPayloadCommitmentAndMetadata`, when received last, resulted in no proposal occurring.
async fn test_proposal_ordering() {
    test_ordering_with_specific_order(true /* qc_formed_first */).await;
    test_ordering_with_specific_order(false /* qc_formed_first */).await;
}
