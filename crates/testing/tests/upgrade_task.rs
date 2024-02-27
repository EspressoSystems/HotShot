use hotshot::tasks::{inject_consensus_polls, task_state::CreateTaskState};
use hotshot::types::SystemContextHandle;
use hotshot_constants::Version;
use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
use hotshot_task_impls::{consensus::ConsensusTaskState, events::HotShotEvent};
use hotshot_testing::task_helpers::{build_cert, build_quorum_proposals_with_upgrade, key_pair_for_id};
use hotshot_types::{
    data::ViewNumber,
    simple_certificate::UpgradeCertificate,
    simple_vote::{UpgradeProposalData, UpgradeVote},
    traits::node_implementation::ConsensusTime,
};
use std::collections::HashMap;

#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_upgrade_task() {
    use hotshot_task_impls::harness::run_harness;
    use hotshot_testing::task_helpers::build_system_handle;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(1).await.0;

    let (private_key, public_key) = key_pair_for_id(1);

    let current_version = Version { major: 0, minor: 1 };
    let new_version = Version { major: 0, minor: 2 };

    let upgrade_data: UpgradeProposalData<TestTypes> = UpgradeProposalData {
        old_version: current_version,
        new_version,
        new_version_hash: [0u8; 12].to_vec(),
        old_version_last_block: ViewNumber::new(5),
        new_version_first_block: ViewNumber::new(7),
    };

    let proposals = build_quorum_proposals_with_upgrade(
      &handle,
      Some(upgrade_data),
      &private_key,
      &public_key,
      2,
      3,
    ).await;

    // Build the API for node 2.
    let handle_2 = build_system_handle(2).await.0;
    let quorum_membership_2 = handle_2.hotshot.memberships.quorum_membership.clone();
//
//
//    let (private_key_2, public_key_2) = key_pair_for_id(2);
//    let upgrade_cert = build_cert::<
//        TestTypes,
//        UpgradeProposalData<TestTypes>,
//        UpgradeVote<TestTypes>,
//        UpgradeCertificate<TestTypes>,
//    >(
//        upgrade_data,
//        &quorum_membership_2,
//        ViewNumber::new(2),
//        &public_key_2,
//        &private_key_2,
//    );
//
//    let quorum_proposal_2 =
//        build_quorum_proposal(&handle_2, Some(upgrade_cert), &private_key_2, 2).await;
//
    // Build the API for node 3.
    let handle_3 = build_system_handle(3).await.0;

    let (private_key_3, public_key_3) = key_pair_for_id(3);
//
//    let quorum_proposal_3 = build_quorum_proposal(&handle_3, None, &private_key_3, 3).await;
//
    // Build the API for node 4.
    let handle_4 = build_system_handle(4).await.0;
    let (private_key_4, public_key_4) = key_pair_for_id(4);
//
//    let quorum_proposal_4 = build_quorum_proposal(&handle_4, None, &private_key_4, 4).await;
//
    let mut input = Vec::new();
    let mut output = HashMap::new();

    input.push(HotShotEvent::ViewChange(ViewNumber::new(1)));
    input.push(HotShotEvent::ViewChange(ViewNumber::new(2)));
    input.push(HotShotEvent::QuorumProposalRecv(
        proposals[2].clone(),
        public_key,
    ));
    input.push(HotShotEvent::ViewChange(ViewNumber::new(2)));
    input.push(HotShotEvent::QuorumProposalRecv(
        proposals[3].clone(),
        public_key,
    ));
    input.push(HotShotEvent::QuorumProposalRecv(
        proposals[4].clone(),
        public_key_4,
    ));

    output.insert(HotShotEvent::ViewChange(ViewNumber::new(1)), 1);
//
//    let handle = build_system_handle(1).await.0;

    let consensus_state = ConsensusTaskState::<
        TestTypes,
        MemoryImpl,
        SystemContextHandle<TestTypes, MemoryImpl>,
    >::create_from(&handle);

    inject_consensus_polls(&consensus_state).await;

    run_harness(input, output, consensus_state, false).await;
}
