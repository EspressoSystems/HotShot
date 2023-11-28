use commit::Committable;
use hotshot::{types::SignatureKey, HotShotConsensusApi};
use hotshot_task_impls::events::HotShotEvent;
use hotshot_testing::node_types::{MemoryImpl, TestTypes};
use hotshot_types::{
    block_impl::VIDTransaction,
    data::{DAProposal, ViewNumber},
    simple_vote::{DAData, DAVote},
    traits::{
        consensus_api::ConsensusSharedApi, node_implementation::NodeType, state::ConsensusTime,
    },
};
use std::{collections::HashMap, marker::PhantomData};

#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_da_task() {
    use hotshot::tasks::add_da_task;
    use hotshot_task_impls::harness::run_harness;
    use hotshot_testing::task_helpers::build_system_handle;
    use hotshot_types::{block_impl::VIDBlockPayload, message::Proposal};

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    // Build the API for node 2.
    let handle = build_system_handle(2).await.0;
    let api: HotShotConsensusApi<TestTypes, MemoryImpl> = HotShotConsensusApi {
        inner: handle.hotshot.inner.clone(),
    };
    let pub_key = *api.public_key();
    let transactions = vec![VIDTransaction(vec![0])];
    let encoded_txns = VIDTransaction::encode(transactions.clone()).unwrap();
    let payload_commitment = VIDBlockPayload::vid_commitment(&encoded_txns);
    let block = VIDBlockPayload {
        transactions,
        payload_commitment,
    };

    let signature =
        <TestTypes as NodeType>::SignatureKey::sign(api.private_key(), block.commit().as_ref());
    let proposal = DAProposal {
        block_payload: block.clone(),
        view_number: ViewNumber::new(2),
    };
    let message = Proposal {
        data: proposal,
        signature,
        _pd: PhantomData,
    };

    // TODO for now reuse the same block payload commitment and signature as DA committee
    // https://github.com/EspressoSystems/jellyfish/issues/369

    // Every event input is seen on the event stream in the output.
    let mut input = Vec::new();
    let mut output = HashMap::new();

    // In view 1, node 2 is the next leader.
    input.push(HotShotEvent::ViewChange(ViewNumber::new(1)));
    input.push(HotShotEvent::ViewChange(ViewNumber::new(2)));
    input.push(HotShotEvent::BlockReady(
        block.clone(),
        (),
        ViewNumber::new(2),
    ));
    input.push(HotShotEvent::DAProposalRecv(message.clone(), pub_key));

    input.push(HotShotEvent::Shutdown);

    output.insert(HotShotEvent::ViewChange(ViewNumber::new(1)), 1);
    output.insert(
        HotShotEvent::BlockReady(block.clone(), (), ViewNumber::new(2)),
        1,
    );
    output.insert(
        HotShotEvent::SendPayloadCommitmentAndMetadata(block.commit(), ()),
        1,
    );
    output.insert(HotShotEvent::DAProposalSend(message.clone(), pub_key), 1);
    let da_vote = DAVote::create_signed_vote(
        DAData {
            payload_commit: block.commit(),
        },
        ViewNumber::new(2),
        api.public_key(),
        api.private_key(),
    );
    output.insert(HotShotEvent::DAVoteSend(da_vote), 1);

    output.insert(HotShotEvent::DAProposalRecv(message, pub_key), 1);

    output.insert(HotShotEvent::ViewChange(ViewNumber::new(2)), 1);
    output.insert(HotShotEvent::Shutdown, 1);

    let build_fn = |task_runner, event_stream| add_da_task(task_runner, event_stream, handle);

    run_harness(input, output, None, build_fn).await;
}
