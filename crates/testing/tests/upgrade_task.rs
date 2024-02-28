use hotshot::tasks::{inject_consensus_polls, task_state::CreateTaskState};
use hotshot::types::SystemContextHandle;
use hotshot_constants::Version;
use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
use hotshot_task_impls::{consensus::ConsensusTaskState, events::HotShotEvent};
use hotshot_testing::task_helpers::{build_cert, build_quorum_proposals_with_upgrade, key_pair_for_id, build_vote};
use hotshot_types::{
    data::ViewNumber,
    simple_certificate::UpgradeCertificate,
    simple_vote::{UpgradeProposalData, UpgradeVote},
    traits::{node_implementation::ConsensusTime, election::Membership},
    message::GeneralConsensusMessage,
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

std::env::set_var("RUST_LOG", "debug");

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
//      &private_key,
      &public_key,
      2,
      4,
    ).await;

    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();

    let mut input = Vec::new();
    let mut output = HashMap::new();

    input.push(HotShotEvent::ViewChange(ViewNumber::new(1)));
    input.push(HotShotEvent::QuorumProposalRecv(
        proposals[0].clone(),
        quorum_membership.get_leader(ViewNumber::new(1)),
    ));
    input.push(HotShotEvent::ViewChange(ViewNumber::new(2)));
    input.push(HotShotEvent::QuorumProposalRecv(
        proposals[1].clone(),
        quorum_membership.get_leader(ViewNumber::new(2)),
    ));
    input.push(HotShotEvent::ViewChange(ViewNumber::new(3)));
    input.push(HotShotEvent::QuorumProposalRecv(
        proposals[2].clone(),
        quorum_membership.get_leader(ViewNumber::new(3)),
    ));
    input.push(HotShotEvent::ViewChange(ViewNumber::new(4)));
    input.push(HotShotEvent::QuorumProposalRecv(
        proposals[3].clone(),
        quorum_membership.get_leader(ViewNumber::new(4)),
    ));
    input.push(HotShotEvent::ViewChange(ViewNumber::new(5)));
    input.push(HotShotEvent::QuorumProposalRecv(
        proposals[4].clone(),
        quorum_membership.get_leader(ViewNumber::new(5)),
    ));
    input.push(HotShotEvent::Shutdown);

    output.insert(HotShotEvent::ViewChange(ViewNumber::new(1)), 1);
    if let GeneralConsensusMessage::Vote(vote) = build_vote(&handle, proposals[0].clone().data).await {
        output.insert(HotShotEvent::QuorumVoteSend(vote.clone()), 1);
    }
    output.insert(HotShotEvent::ViewChange(ViewNumber::new(2)), 1);
    output.insert(HotShotEvent::ViewChange(ViewNumber::new(3)), 1);
    output.insert(HotShotEvent::ViewChange(ViewNumber::new(4)), 1);
    output.insert(HotShotEvent::ViewChange(ViewNumber::new(5)), 1);

//    output.insert(HotShotEvent::ViewChange(ViewNumber::new(5)), 1);
//    output.insert(HotShotEvent::ViewChange(ViewNumber::new(5)), 1);
//    output.insert(HotShotEvent::ViewChange(ViewNumber::new(4)), 1);
//    output.insert(HotShotEvent::ViewChange(ViewNumber::new(5)), 1);
//    output.insert(HotShotEvent::ViewChange(ViewNumber::new(2)), 1);
//    output.insert(HotShotEvent::ViewChange(ViewNumber::new(2)), 1);
//
//    let handle = build_system_handle(1).await.0;

    let consensus_state = ConsensusTaskState::<
        TestTypes,
        MemoryImpl,
        SystemContextHandle<TestTypes, MemoryImpl>,
    >::create_from(&handle);

    inject_consensus_polls(&consensus_state).await;

    run_harness(input, output, consensus_state, false).await;
    panic!();
}
