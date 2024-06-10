use futures::StreamExt;
use hotshot::tasks::task_state::CreateTaskState;
use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
use hotshot_task_impls::{
    consensus::helpers::{visit_leaf_chain, visit_leaf_chain2, LeafChainTraversalOutcome},
    quorum_proposal::QuorumProposalTaskState,
};
use hotshot_testing::{
    helpers::{build_fake_view_with_leaf, build_system_handle},
    view_generator::TestViewGenerator,
};
use hotshot_types::{data::Leaf, vote::HasViewNumber};

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn visit_valid_chain() {
    use std::sync::Arc;

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
    for view in (&mut generator).take(10).collect::<Vec<_>>().await {
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
        let ret1 = visit_leaf_chain(
            &proposal.data,
            Arc::clone(&consensus),
            quorum_proposal_task_state.public_key,
            None,
        )
        .await
        .unwrap();

        let ret2 = visit_leaf_chain2(
            &proposal.data,
            Arc::clone(&consensus),
            quorum_proposal_task_state.public_key,
            None,
        )
        .await
        .unwrap();

        tracing::info!("Running view {ii}");
        assert_eq!(
            ret1.new_decided_view_number, ret2.new_decided_view_number,
            "Decided View Number"
        );
        assert_eq!(
            ret1.new_locked_view_number, ret2.new_locked_view_number,
            "Locked View Number"
        );
        assert_eq!(ret1.new_decide_qc, ret2.new_decide_qc, "Decided QC");
        assert!(ret1
            .leaf_views
            .iter()
            .zip(ret2.leaf_views.iter())
            .all(|(v1, v2)| v1.leaf.view_number() == v2.leaf.view_number()));
        assert!(ret1
            .leaves_decided
            .iter()
            .zip(ret2.leaves_decided.iter())
            .all(|(v1, v2)| v1.view_number() == v2.view_number()));
    }
}
