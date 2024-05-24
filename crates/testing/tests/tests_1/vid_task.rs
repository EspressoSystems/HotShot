use std::marker::PhantomData;

use hotshot_example_types::{
    block_types::{TestMetadata},
    node_types::TestTypes,
    state_types::TestInstanceState,
};
use hotshot_task_impls::{events::HotShotEvent, vid::VidTaskState};
use hotshot_testing::{
    predicates::event::exact,
    script::{run_test_script, TestScriptStage},
    task_helpers::key_pair_for_id,
    task_helpers::{build_system_handle, vid_scheme_from_view_number},
    view_generator::TestViewGenerator,
};
use hotshot_types::{
    data::{null_block, VidDisperse, ViewNumber},
    message::Proposal,
    traits::{
        election::Membership,
        node_implementation::{ConsensusTime, NodeType},
        signature_key::SignatureKey,
    },
    utils::BuilderCommitment,
};
use jf_vid::{precomputable::Precomputable, VidScheme};
use sha2::Digest;

#[cfg(test)]
#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_vid_task() {
    use hotshot::tasks::task_state::CreateTaskState;
    use hotshot_example_types::node_types::MemoryImpl;
    

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let node_id = 2;
    let handle = build_system_handle(node_id).await.0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let da_membership = handle.hotshot.memberships.da_membership.clone();
    let encoded_transactions = Vec::new();
    let (private_key, public_key) = key_pair_for_id(node_id);

    let mut generator = TestViewGenerator::generate(quorum_membership.clone(), da_membership);
    let mut das = Vec::new();
    let mut vids = Vec::new();
    for view in (&mut generator).take(2) {
        das.push(view.da_proposal.clone());
        vids.push(view.vid_proposal.clone());
    }

    let proposal = das[1].clone();

    let mut vid = vid_scheme_from_view_number::<TestTypes>(&quorum_membership, ViewNumber::new(2));
    let (_, vid_precompute) = vid.commit_only_precompute(&encoded_transactions).unwrap();

    let vid_disperse = vid.disperse(&encoded_transactions).unwrap();
    let payload_commitment = vid_disperse.commit;
    let vid_disperse =
        VidDisperse::from_membership(proposal.data.view_number, vid_disperse, &quorum_membership);
    let signature = <TestTypes as NodeType>::SignatureKey::sign(
        &private_key,
        vid_disperse.payload_commitment.as_ref(),
    )
    .unwrap();
    let vid_proposal = Proposal {
        data: vid_disperse.clone(),
        signature,
        _pd: PhantomData,
    };

    let builder_commitment = BuilderCommitment::from_raw_digest(sha2::Sha256::new().finalize());

    let stage_1 = TestScriptStage {
        inputs: vec![
            HotShotEvent::ViewChange(ViewNumber::new(1)),
            HotShotEvent::ViewChange(ViewNumber::new(2)),
            HotShotEvent::BlockRecv(
                encoded_transactions.into(),
                TestMetadata,
                ViewNumber::new(2),
                null_block::builder_fee(quorum_membership.total_nodes(), &TestInstanceState {})
                    .unwrap(),
                vid_precompute,
            ),
        ],
        outputs: vec![
            exact(HotShotEvent::SendPayloadCommitmentAndMetadata(
                payload_commitment,
                builder_commitment,
                TestMetadata,
                ViewNumber::new(2),
                null_block::builder_fee(quorum_membership.total_nodes(), &TestInstanceState {})
                    .unwrap(),
            )),
            exact(HotShotEvent::BlockReady(
                vid_disperse.clone(),
                ViewNumber::new(2),
            )),
            exact(HotShotEvent::VidDisperseSend(
                vid_proposal.clone(),
                public_key,
            )),
        ],
        asserts: vec![],
    };

    let vid_task_state = VidTaskState::<TestTypes, MemoryImpl>::create_from(&handle).await;
    run_test_script(vec![stage_1], vid_task_state).await;
}
