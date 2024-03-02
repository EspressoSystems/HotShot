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
    key_pair_for_id, vid_init, TestView, TestViewGenerator,
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
    let function = move |e: &_| e == &event;

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
    let function = |e: &_| match e {
        LeafDecided(_) => true,
        _ => false,
    };

    Predicate {
        function: Box::new(function),
        info,
    }
}

pub fn quorum_vote_send<TYPES>() -> Predicate<HotShotEvent<TYPES>>
where
    TYPES: NodeType,
{
    let info = "QuorumVoteSend".to_string();
    let function = |e: &_| match e {
        QuorumVoteSend(_) => true,
        _ => false,
    };

    Predicate {
        function: Box::new(function),
        info,
    }
}

type ConsensusTaskTestState =
    ConsensusTaskState<TestTypes, MemoryImpl, SystemContextHandle<TestTypes, MemoryImpl>>;

pub fn consensus_predicate(
    function: Box<dyn for<'a> Fn(&'a ConsensusTaskTestState) -> bool>,
    info: &str,
) -> Predicate<ConsensusTaskTestState> {
    Predicate {
        function: function,
        info: info.to_string(),
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

    let mut proposals = Vec::new();
    let mut votes = Vec::new();
    let mut dacs = Vec::new();
    let mut vids = Vec::new();

    let mut generator = TestViewGenerator::generate(quorum_membership.clone());

    for view in (&mut generator).take(2) {
        proposals.push(view.quorum_proposal.clone());
        votes.push(view.create_vote(&handle));
        dacs.push(view.da_certificate.clone());
        vids.push(view.vid_proposal.clone());
    }

    generator.add_upgrade(upgrade_data);

    for view in generator.take(4) {
        proposals.push(view.quorum_proposal.clone());
        votes.push(view.create_vote(&handle));
        dacs.push(view.da_certificate.clone());
        vids.push(view.vid_proposal.clone());
    }


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
        asserts: vec![],
        // asserts: vec![consensus_predicate(
        //     Box::new(|state: &_| state.decided_upgrade_cert.is_none()),
        //     "expected no decided_upgrade_cert",
        // )],
    };

    let view_2 = TestScriptStage {
        inputs: vec![
            ViewChange(ViewNumber::new(2)),
            VidDisperseRecv(vids[1].0.clone(), vids[1].1.clone()),
            QuorumProposalRecv(
                proposals[1].clone(),
                quorum_membership.get_leader(ViewNumber::new(2)),
            ),
            DACRecv(dacs[1].clone()),
        ],
        outputs: vec![
            exact(ViewChange(ViewNumber::new(2))),
            exact(QuorumVoteSend(votes[1].clone())),
        ],
        // asserts: vec![],
        asserts: vec![consensus_predicate(
            Box::new(|state: &_| state.decided_upgrade_cert.is_none()),
            "expected no decided_upgrade_cert",
        )],
    };

    let view_3 = TestScriptStage {
        inputs: vec![
            ViewChange(ViewNumber::new(3)),
            QuorumProposalRecv(
                proposals[2].clone(),
                quorum_membership.get_leader(ViewNumber::new(3)),
            ),
            DACRecv(dacs[2].clone()),
            VidDisperseRecv(vids[2].0.clone(), vids[2].1.clone()),
        ],
        outputs: vec![
            exact(ViewChange(ViewNumber::new(3))),
            leaf_decided(),
            exact(QuorumVoteSend(votes[2].clone())),
        ],
        // asserts: vec![],
        asserts: vec![consensus_predicate(
            Box::new(|state: &_| state.decided_upgrade_cert.is_some()),
            "expected a decided_upgrade_cert",
        )],
    };

    let view_4 = TestScriptStage {
        inputs: vec![
            ViewChange(ViewNumber::new(4)),
            QuorumProposalRecv(
                proposals[3].clone(),
                quorum_membership.get_leader(ViewNumber::new(4)),
            ),
            DACRecv(dacs[3].clone()),
            VidDisperseRecv(vids[3].0.clone(), vids[3].1.clone()),
        ],
        outputs: vec![
            exact(ViewChange(ViewNumber::new(4))),
            leaf_decided(),
            exact(QuorumVoteSend(votes[3].clone())),
        ],
        asserts: vec![],
        //        asserts: vec![consensus_predicate(
        //            Box::new(|state: &_| state.decided_upgrade_cert.is_none()),
        //            "expected a decided_upgrade_cert",
        //        )],
    };

    let view_5 = TestScriptStage {
        inputs: vec![
            ViewChange(ViewNumber::new(5)),
            QuorumProposalRecv(
                proposals[4].clone(),
                quorum_membership.get_leader(ViewNumber::new(5)),
            ),
        ],
        outputs: vec![exact(ViewChange(ViewNumber::new(5))), leaf_decided()],
        asserts: vec![],
        //        asserts: vec![consensus_predicate(
        //            Box::new(|state: &_| state.decided_upgrade_cert.is_some()),
        //            "expected a decided_upgrade_cert",
        //        )],
    };

    let script = vec![view_1, view_2, view_3, view_4, view_5];

    let consensus_state = ConsensusTaskState::<
        TestTypes,
        MemoryImpl,
        SystemContextHandle<TestTypes, MemoryImpl>,
    >::create_from(&handle);

    inject_consensus_polls(&consensus_state).await;

    //    assert_eq!(consensus_state.upgrade_cert, None);

    run_test_script(script, consensus_state).await;

    //    assert_eq!(consensus_state.upgrade_cert.unwrap().data, upgrade_data);
}
