use hotshot::types::SignatureKey;
use hotshot_example_types::{block_types::TestTransaction, node_types::TestTypes};
use hotshot_task_impls::{events::HotShotEvent, vid::VIDTaskState};
use hotshot_testing::task_helpers::{build_system_handle, vid_scheme_from_view_number};
use hotshot_types::traits::node_implementation::{ConsensusTime, NodeType};
use hotshot_types::{
    data::{DAProposal, VidDisperse, ViewNumber},
    traits::consensus_api::ConsensusApi,
};
use jf_primitives::vid::VidScheme;
use std::collections::HashMap;
use std::marker::PhantomData;

#[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_vid_task() {
    use hotshot_task_impls::harness::run_harness;
    use hotshot_types::message::Proposal;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    // Build the API for node 2.
    let handle = build_system_handle(2).await.0;
    let pub_key = *handle.public_key();

    // quorum membership for VID share distribution
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();

    let vid = vid_scheme_from_view_number::<TestTypes>(&quorum_membership, ViewNumber::new(0));
    let transactions = vec![TestTransaction(vec![0])];
    let encoded_transactions = TestTransaction::encode(transactions.clone()).unwrap();
    let vid_disperse = vid.disperse(&encoded_transactions).unwrap();
    let payload_commitment = vid_disperse.commit;

    let signature = <TestTypes as NodeType>::SignatureKey::sign(
        handle.private_key(),
        payload_commitment.as_ref(),
    )
    .expect("Failed to sign block payload!");
    let proposal: DAProposal<TestTypes> = DAProposal {
        encoded_transactions: encoded_transactions.clone(),
        metadata: (),
        view_number: ViewNumber::new(2),
    };
    let message = Proposal {
        data: proposal,
        signature,
        _pd: PhantomData,
    };

    let vid_disperse = VidDisperse::from_membership(
        message.data.view_number,
        vid_disperse,
        &quorum_membership.clone().into(),
    );

    let vid_proposal = Proposal {
        data: vid_disperse.clone(),
        signature: message.signature.clone(),
        _pd: PhantomData,
    };

    let mut input = Vec::new();
    let mut output = HashMap::new();

    // In view 1, node 2 is the next leader.
    input.push(HotShotEvent::ViewChange(ViewNumber::new(1)));
    input.push(HotShotEvent::ViewChange(ViewNumber::new(2)));
    input.push(HotShotEvent::TransactionsSequenced(
        encoded_transactions.clone(),
        (),
        ViewNumber::new(2),
    ));
    input.push(HotShotEvent::BlockReady(
        vid_disperse.clone(),
        ViewNumber::new(2),
    ));
    input.push(HotShotEvent::VidDisperseSend(vid_proposal.clone(), pub_key));
    input.push(HotShotEvent::VidDisperseRecv(vid_proposal.clone(), pub_key));
    input.push(HotShotEvent::Shutdown);

    output.insert(
        HotShotEvent::BlockReady(vid_disperse, ViewNumber::new(2)),
        1,
    );

    output.insert(
        HotShotEvent::SendPayloadCommitmentAndMetadata(payload_commitment, (), ViewNumber::new(2)),
        1,
    );
    output.insert(
        HotShotEvent::VidDisperseSend(vid_proposal.clone(), pub_key),
        1,
    );

    let vid_state = VIDTaskState {
        api: handle.clone(),
        consensus: handle.hotshot.get_consensus(),
        cur_view: ViewNumber::new(0),
        vote_collector: None,
        network: handle.hotshot.networks.quorum_network.clone(),
        membership: handle.hotshot.memberships.vid_membership.clone().into(),
        public_key: *handle.public_key(),
        private_key: handle.private_key().clone(),
        id: handle.hotshot.id,
    };
    run_harness(input, output, vid_state, false).await;
}
