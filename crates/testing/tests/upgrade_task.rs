use hotshot::tasks::{inject_consensus_polls, task_state::CreateTaskState};
use hotshot::types::SystemContextHandle;
use hotshot_constants::Version;
use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
use hotshot_task_impls::{consensus::ConsensusTaskState, events::HotShotEvent::*};
use hotshot_testing::task_helpers::{
    build_quorum_proposals_with_upgrade, key_pair_for_id,
};
use hotshot_types::{
    data::ViewNumber,
    simple_vote::UpgradeProposalData,
    traits::{election::Membership, node_implementation::ConsensusTime},
};

#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_upgrade_task() {
    use hotshot_task_impls::harness::run_test_script;
    use hotshot_testing::task_helpers::build_system_handle;

    std::env::set_var("RUST_LOG", "debug");

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(1).await.0;

    let (_private_key, public_key) = key_pair_for_id(1);

    let current_version = Version { major: 0, minor: 1 };
    let new_version = Version { major: 0, minor: 2 };

    let upgrade_data: UpgradeProposalData<TestTypes> = UpgradeProposalData {
        old_version: current_version,
        new_version,
        new_version_hash: [0u8; 12].to_vec(),
        old_version_last_block: ViewNumber::new(5),
        new_version_first_block: ViewNumber::new(7),
    };

    let (proposals, votes) =
        build_quorum_proposals_with_upgrade(&handle, Some(upgrade_data), &public_key, 2, 4).await;

    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();

    let view_1 = (
        vec![
            ViewChange(ViewNumber::new(1)),
            QuorumProposalRecv(
                proposals[0].clone(),
                quorum_membership.get_leader(ViewNumber::new(1)),
            ),
        ],
        vec![
            ViewChange(ViewNumber::new(1)),
            QuorumVoteSend(votes[0].clone()),
        ],
    );

    let view_2 = (
        vec![
            ViewChange(ViewNumber::new(2)),
            QuorumProposalRecv(
                proposals[1].clone(),
                quorum_membership.get_leader(ViewNumber::new(2)),
            ),
        ],
        vec![ViewChange(ViewNumber::new(2))],
    );

    let view_3 = (
        vec![
            ViewChange(ViewNumber::new(3)),
            QuorumProposalRecv(
                proposals[2].clone(),
                quorum_membership.get_leader(ViewNumber::new(3)),
            ),
        ],
        vec![ViewChange(ViewNumber::new(3))],
    );

    let view_4 = (
        vec![
            ViewChange(ViewNumber::new(4)),
            QuorumProposalRecv(
                proposals[3].clone(),
                quorum_membership.get_leader(ViewNumber::new(4)),
            ),
        ],
        vec![ViewChange(ViewNumber::new(4))],
    );

    let script = vec![view_1, view_2, view_3, view_4];

    let consensus_state = ConsensusTaskState::<
        TestTypes,
        MemoryImpl,
        SystemContextHandle<TestTypes, MemoryImpl>,
    >::create_from(&handle);

    inject_consensus_polls(&consensus_state).await;

    run_test_script(script, consensus_state).await;
}
