use futures::StreamExt;
use hotshot::tasks::task_state::CreateTaskState;
use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
use hotshot_task_impls::quorum_proposal::{handlers::visit_leaf_chain, QuorumProposalTaskState};
use hotshot_testing::{
    helpers::{build_fake_view_with_leaf, build_system_handle},
    view_generator::TestViewGenerator,
};
use hotshot_types::{data::Leaf, vote::HasViewNumber};

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn visit_valid_chain() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let node_id = 1;
    let handle = build_system_handle(node_id).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let da_membership = handle.hotshot.memberships.da_membership.clone();

    let mut generator = TestViewGenerator::generate(quorum_membership.clone(), da_membership);
    let consensus = handle.hotshot.consensus();
    let mut consensus_writer = consensus.write().await;

    let mut proposals = Vec::new();
    for view in (&mut generator).take(5).collect::<Vec<_>>().await {
        let proposal = view.quorum_proposal;
        let leaf = view.leaf;
        consensus_writer.update_validated_state_map(
            proposal.data.view_number(),
            build_fake_view_with_leaf(leaf),
        );

        consensus_writer.update_saved_leaves(Leaf::from_quorum_proposal(&proposal.data));

        proposals.push(proposal);
    }
    drop(consensus_writer);

    let quorum_proposal_task_state =
        QuorumProposalTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;

    for (ii, proposal) in proposals.into_iter().enumerate() {
        let ret = visit_leaf_chain(&proposal.data, &quorum_proposal_task_state)
            .await
            .unwrap();

        if ii == 0 {
            assert_eq!(ret.current_chain_length, 0);
        }

        if ii == 1 {
            assert_eq!(ret.current_chain_length, 1);
        }

        if ii == 2 {
            assert_eq!(ret.current_chain_length, 2);
        }

        if ii > 2 {
            assert_eq!(ret.current_chain_length, 3);
        }
    }
}
