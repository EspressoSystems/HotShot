use hotshot::{tasks::task_state::CreateTaskState, types::SystemContextHandle};
use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
use hotshot_task_impls::{consensus::ConsensusTaskState, events::HotShotEvent::*};
use hotshot_testing::{
    predicates::exact, task_helpers::vid_scheme_from_view_number, view_generator::TestViewGenerator,
};
use hotshot_types::{
    data::ViewNumber,
    traits::{consensus_api::ConsensusApi, node_implementation::ConsensusTime},
};
use jf_primitives::vid::VidScheme;

#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
/// Test to ensure that the consensus task proposes when it receives an event that initiates a
/// proposal, no matter what order it receives such an event.
async fn test_proposal_ordering() {
    use hotshot_testing::script::{run_test_script, TestScriptStage};
    use hotshot_testing::task_helpers::build_system_handle;
    use hotshot_types::simple_certificate::QuorumCertificate;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let node_id = 1;
    let handle = build_system_handle(node_id).await.0;
    let public_key = *handle.public_key();
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();

    let vid =
        vid_scheme_from_view_number::<TestTypes>(&quorum_membership, ViewNumber::new(node_id));

    // Make some empty encoded transactions, we just care about having a commitment handy for the
    // later calls.
    let encoded_transactions = Vec::new();
    let vid_disperse = vid.disperse(&encoded_transactions).unwrap();
    let _payload_commitment = vid_disperse.commit;

    let mut proposals = Vec::new();
    let mut votes = Vec::new();
    let mut dacs = Vec::new();
    let mut vids = Vec::new();
    let mut leaders = Vec::new();

    let mut generator = TestViewGenerator::generate(quorum_membership.clone());
    let qc = QuorumCertificate::<TestTypes>::genesis();

    for view in (&mut generator).take(2) {
        proposals.push(view.quorum_proposal.clone());
        votes.push(view.create_vote(&handle));
        dacs.push(view.da_certificate.clone());
        vids.push(view.vid_proposal.clone());
        leaders.push(view.leader_public_key);
    }

    // We need a couple of events to occur for the leader to propose:
    // `SendPayloadCommitmentAndMetadata`, `QCFormed` (either QC or TC), and `QuorumProposalRecv`
    // for the previous view if a QC was formed by us for it.

    // First, receive a proposal and move to view 1, this should correspond with the node id.
    let view_1 = TestScriptStage {
        inputs: vec![
            QCFormed(either::Left(qc.clone())),
            QuorumProposalRecv(proposals[0].clone(), leaders[0]),
        ],
        outputs: vec![
            exact(QuorumProposalSend(proposals[0].clone(), public_key)),
            exact(ViewChange(ViewNumber::new(node_id))),
            exact(QuorumVoteSend(votes[0].clone())),
        ],
        asserts: vec![],
    };

    let script = vec![view_1];

    let consensus_state = ConsensusTaskState::<
        TestTypes,
        MemoryImpl,
        SystemContextHandle<TestTypes, MemoryImpl>,
    >::create_from(&handle)
    .await;

    run_test_script(script, consensus_state).await;
}
