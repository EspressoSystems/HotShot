use hotshot::tasks::{inject_consensus_polls, task_state::CreateTaskState};
use hotshot::types::{SignatureKey, SystemContextHandle};
use hotshot_constants::Version;
use hotshot_example_types::{
    block_types::TestTransaction,
    node_types::{MemoryImpl, TestTypes},
};
use hotshot_task_impls::{
    consensus::ConsensusTaskState, events::HotShotEvent, events::HotShotEvent::*,
    harness::Predicate,
};
use hotshot_testing::task_helpers::{
    build_quorum_proposals_with_upgrade, key_pair_for_id, vid_init,
};
use hotshot_types::{
    data::{VidDisperse, VidSchemeTrait, ViewNumber},
    message::Proposal,
    simple_vote::UpgradeProposalData,
    traits::{
        election::Membership,
        node_implementation::{ConsensusTime, NodeType},
    },
};
use std::marker::PhantomData;

pub fn exact<TYPES>(event: HotShotEvent<TYPES>) -> Predicate<HotShotEvent<TYPES>>
where
    TYPES: NodeType,
{
    let info = format!("{:?}", event);
    let function = move |e| e == event;

    Predicate {
        function: Box::new(function),
        info,
    }
}

pub fn leaf_decided<TYPES>() -> Predicate<HotShotEvent<TYPES>>
where
    TYPES: NodeType,
{
    let info = "LeafDecided".to_string();
    let function = |e| match e {
        LeafDecided(_) => true,
        _ => false,
    };

    Predicate {
        function: Box::new(function),
        info,
    }
}

#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_upgrade_task() {
    use hotshot_task_impls::harness::{run_test_script, TestScriptStage};
    use hotshot_testing::task_helpers::build_system_handle;

    std::env::set_var("RUST_LOG", "debug");

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(1).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let vid = vid_init::<TestTypes>(&quorum_membership, ViewNumber::new(2));
    let transactions = vec![TestTransaction(vec![0])];
    let encoded_transactions = TestTransaction::encode(transactions.clone()).unwrap();
    let vid_disperse = vid.disperse(&encoded_transactions).unwrap();
    let payload_commitment = vid_disperse.commit;
    let (private_key, public_key) = key_pair_for_id(1);

    let vid_signature =
        <TestTypes as NodeType>::SignatureKey::sign(&private_key, payload_commitment.as_ref())
            .expect("Failed to sign payload commitment");
    let vid_disperse_inner = VidDisperse::from_membership(
        ViewNumber::new(2),
        vid_disperse,
        &quorum_membership.clone().into(),
    );
    // TODO for now reuse the same block payload commitment and signature as DA committee
    // https://github.com/EspressoSystems/jellyfish/issues/369
    let vid_proposal = Proposal {
        data: vid_disperse_inner.clone(),
        signature: vid_signature,
        _pd: PhantomData,
    };

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

    let view_1 = TestScriptStage {
        inputs: vec![
            ViewChange(ViewNumber::new(1)),
            QuorumProposalRecv(
                proposals[0].clone(),
                quorum_membership.get_leader(ViewNumber::new(1)),
            ),
        ],
        outputs: vec![
            exact(ViewChange(ViewNumber::new(1))),
            exact(QuorumVoteSend(votes[0].clone())),
        ],
    };

    let view_2 = TestScriptStage {
        inputs: vec![
            ViewChange(ViewNumber::new(2)),
            VidDisperseRecv(vid_proposal.clone(), public_key),
            QuorumProposalRecv(
                proposals[1].clone(),
                quorum_membership.get_leader(ViewNumber::new(2)),
            ),
        ],
        outputs: vec![exact(ViewChange(ViewNumber::new(2)))],
    };

    let view_3 = TestScriptStage {
        inputs: vec![
            ViewChange(ViewNumber::new(3)),
            QuorumProposalRecv(
                proposals[2].clone(),
                quorum_membership.get_leader(ViewNumber::new(3)),
            ),
        ],
        outputs: vec![exact(ViewChange(ViewNumber::new(3))), leaf_decided()],
    };

    let view_4 = TestScriptStage {
        inputs: vec![
            ViewChange(ViewNumber::new(4)),
            QuorumProposalRecv(
                proposals[3].clone(),
                quorum_membership.get_leader(ViewNumber::new(4)),
            ),
        ],
        outputs: vec![exact(ViewChange(ViewNumber::new(4))), leaf_decided()],
    };

    let script = vec![view_1, view_2, view_3, view_4];

    let consensus_state = ConsensusTaskState::<
        TestTypes,
        MemoryImpl,
        SystemContextHandle<TestTypes, MemoryImpl>,
    >::create_from(&handle);

    inject_consensus_polls(&consensus_state).await;

    run_test_script(script, consensus_state).await;
}
