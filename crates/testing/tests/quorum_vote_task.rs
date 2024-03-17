#![allow(clippy::panic)]
use hotshot::tasks::{ task_state::CreateTaskState};
use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
use hotshot_types::{data::ViewNumber, traits::node_implementation::ConsensusTime};

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_quorum_vote_task() {
    use hotshot::tasks::{inject_quorum_vote_polls};
    use hotshot_task_impls::{ events::HotShotEvent::*, quorum_vote::QuorumVoteTaskState};
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

    generator.next();
    let view = generator.current_view.clone().unwrap();
        proposals.push(view.quorum_proposal.clone());
        leaders.push(view.leader_public_key);
        votes.push(view.create_quorum_vote(&handle));
        dacs.push(view.da_certificate.clone());
        vids.push(view.vid_proposal.clone());

    // Send a proposal, vote on said proposal, update view based on proposal QC, receive vote as next leader
    let view_1 = TestScriptStage {
        inputs: vec![
            QuorumProposalRecv(proposals[0].clone(), leaders[0]),
            DACRecv(dacs[0].clone()),
            VidDisperseRecv(vids[0].0.clone()),
        ],
        outputs: vec![
            exact(QuorumProposalValidated(proposals[0].data.clone())),
            exact(DACValidated(dacs[0].clone())),            
            exact(VidDisperseValidated(vids[0].0.data.clone())),
            exact(ViewChange(ViewNumber::new(2))),
            exact(DummyQuorumVoteSend(ViewNumber::new(1))),
        ],
        asserts: vec![],
    };

    let quorum_vote_state = QuorumVoteTaskState::<
        TestTypes,
        MemoryImpl,
    >::create_from(&handle)
    .await;

    inject_quorum_vote_polls(&quorum_vote_state).await;
    run_test_script(vec![view_1], quorum_vote_state).await;
}
