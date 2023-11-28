use hotshot::{tasks::add_vid_task, types::SignatureKey, HotShotConsensusApi};
use hotshot_task_impls::events::HotShotEvent;
use hotshot_testing::{
    node_types::{MemoryImpl, TestTypes},
    task_helpers::vid_init,
};
use hotshot_types::{
    block_impl::VIDTransaction,
    data::{DAProposal, VidDisperse, VidSchemeTrait, ViewNumber},
    traits::{consensus_api::ConsensusSharedApi, state::ConsensusTime},
};
use hotshot_types::{simple_vote::VIDVote, traits::node_implementation::NodeType};
use std::collections::HashMap;
use std::marker::PhantomData;

#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_vid_task() {
    use hotshot_task_impls::harness::run_harness;
    use hotshot_testing::task_helpers::build_system_handle;
    use hotshot_types::message::Proposal;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    // Build the API for node 2.
    let handle = build_system_handle(2).await.0;
    let api: HotShotConsensusApi<TestTypes, MemoryImpl> = HotShotConsensusApi {
        inner: handle.hotshot.inner.clone(),
    };
    let pub_key = *api.public_key();

    let vid = vid_init();
    let transactions = vec![VIDTransaction(vec![0])];
    let encoded_transactions = VIDTransaction::encode(transactions.clone()).unwrap();
    let vid_disperse = vid.disperse(&encoded_transactions).unwrap();
    let payload_commitment = vid_disperse.commit;

    let signature =
        <TestTypes as NodeType>::SignatureKey::sign(api.private_key(), payload_commitment.as_ref());
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
    let vid_proposal = Proposal {
        data: VidDisperse {
            view_number: message.data.view_number,
            payload_commitment,
            shares: vid_disperse.shares,
            common: vid_disperse.common,
        },
        signature: message.signature.clone(),
        _pd: PhantomData,
    };

    // Every event input is seen on the event stream in the output.
    let mut input = Vec::new();
    let mut output = HashMap::new();

    // In view 1, node 2 is the next leader.
    input.push(HotShotEvent::ViewChange(ViewNumber::new(1)));
    input.push(HotShotEvent::ViewChange(ViewNumber::new(2)));
    input.push(HotShotEvent::BlockReady(
        encoded_transactions.clone(),
        (),
        ViewNumber::new(2),
    ));

    input.push(HotShotEvent::VidDisperseRecv(vid_proposal.clone(), pub_key));
    input.push(HotShotEvent::Shutdown);

    output.insert(HotShotEvent::ViewChange(ViewNumber::new(1)), 1);
    output.insert(
        HotShotEvent::BlockReady(encoded_transactions, (), ViewNumber::new(2)),
        1,
    );

    let vid_vote = VIDVote::create_signed_vote(
        hotshot_types::simple_vote::VIDData {
            payload_commit: payload_commitment,
        },
        ViewNumber::new(2),
        api.public_key(),
        api.private_key(),
    );
    output.insert(HotShotEvent::VidVoteSend(vid_vote), 1);

    output.insert(HotShotEvent::VidDisperseRecv(vid_proposal, pub_key), 1);
    output.insert(HotShotEvent::ViewChange(ViewNumber::new(2)), 1);
    output.insert(HotShotEvent::Shutdown, 1);

    let build_fn = |task_runner, event_stream| add_vid_task(task_runner, event_stream, handle);

    run_harness(input, output, None, build_fn).await;
}
