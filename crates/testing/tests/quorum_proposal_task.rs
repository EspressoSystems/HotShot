use hotshot::tasks::task_state::CreateTaskState;
use hotshot_task_impls::events::HotShotEvent::*;
use hotshot_testing::predicates::exact;
use hotshot_testing::{
    script::{run_test_script, TestScriptStage},
    task_helpers::build_system_handle,
    view_generator::TestViewGenerator,
};
use hotshot_types::data::ViewNumber;
use hotshot_types::traits::node_implementation::ConsensusTime;
use jf_primitives::vid::VidScheme;

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_quorum_proposal_task() {
    use hotshot::tasks::inject_quorum_proposal_polls;
    use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
    use hotshot_task_impls::quorum_proposal::QuorumProposalTaskState;
    use hotshot_testing::{
        predicates::is_at_view_number, task_helpers::vid_scheme_from_view_number,
    };

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    // We need to propose as the leader for view 2, otherwise we get caught up with the special
    // case in the genesis view.
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

    // Run at view 2, the quorum vote task shouldn't care as long as the bookkeeping is correct
    let view_2 = TestScriptStage {
        inputs: vec![
            QuorumProposalRecv(proposals[1].clone(), leaders[1]),
            SendPayloadCommitmentAndMetadata(payload_commitment, (), ViewNumber::new(2)),
        ],
        outputs: vec![exact(QuorumProposalValidated(proposals[1].data.clone()))],
        asserts: vec![],
    };

    let quorum_proposal_task_state =
        QuorumProposalTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;
    inject_quorum_proposal_polls(&quorum_proposal_task_state).await;

    let script = vec![view_2];
    run_test_script(script, quorum_proposal_task_state).await;
}
