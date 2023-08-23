use commit::Committable;
use hotshot::HotShotSequencingConsensusApi;
use hotshot_consensus::traits::ConsensusSharedApi;
use hotshot_task_impls::events::SequencingHotShotEvent;
use hotshot_testing::node_types::SequencingMemoryImpl;
use hotshot_testing::node_types::SequencingTestTypes;
use hotshot_types::data::DAProposal;
use hotshot_types::data::ViewNumber;
use hotshot_types::traits::node_implementation::ExchangesType;
use hotshot_types::traits::{election::ConsensusExchange, state::ConsensusTime};
use std::collections::HashMap;

#[cfg(test)]
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
async fn test_network_task() {
    use hotshot::{
        demos::sdemo::{SDemoBlock, SDemoNormalBlock},
        tasks::{add_network_event_task, add_network_message_task},
    };
    use hotshot_task_impls::{
        harness::{build_harness, run_harness},
        network::NetworkTaskKind,
    };
    use hotshot_testing::system_handle::build_system_handle;
    use hotshot_types::{
        message::{CommitteeConsensusMessage, Proposal},
        traits::election::CommitteeExchangeType,
    };

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    // Build the API for node 2.
    let handle = build_system_handle(2).await;
    let api: HotShotSequencingConsensusApi<SequencingTestTypes, SequencingMemoryImpl> =
        HotShotSequencingConsensusApi {
            inner: handle.hotshot.inner.clone(),
        };
    let quorum_exchange = api.inner.exchanges.quorum_exchange().clone();
    let committee_exchange = api.inner.exchanges.committee_exchange().clone();
    let view_sync_excahnge = api.inner.exchanges.view_sync_exchange().clone();
    let pub_key = *api.public_key();
    let block = SDemoBlock::Normal(SDemoNormalBlock {
        previous_state: (),
        transactions: Vec::new(),
    });
    let block_commitment = block.commit();
    let signature = committee_exchange.sign_da_proposal(&block_commitment);
    let proposal = DAProposal {
        deltas: block.clone(),
        view_number: ViewNumber::new(2),
    };
    let message = Proposal {
        data: proposal,
        signature,
    };

    // Every event input is seen on the event stream in the output.
    let mut input = Vec::new();
    let mut output = HashMap::new();

    // In view 1, node 2 is the next leader.
    input.push(SequencingHotShotEvent::ViewChange(ViewNumber::new(1)));
    input.push(SequencingHotShotEvent::ViewChange(ViewNumber::new(2)));
    input.push(SequencingHotShotEvent::DAProposalSend(
        message.clone(),
        pub_key.clone(),
    ));
    input.push(SequencingHotShotEvent::Shutdown);

    output.insert(SequencingHotShotEvent::ViewChange(ViewNumber::new(1)), 1);
    // output.insert(SequencingHotShotEvent::SendDABlockData(block), 1);
    output.insert(
        SequencingHotShotEvent::DAProposalSend(message.clone(), pub_key),
        1,
    );
    if let Ok(Some(vote_token)) = committee_exchange.make_vote_token(ViewNumber::new(2)) {
        let da_message =
            committee_exchange.create_da_message(block_commitment, ViewNumber::new(2), vote_token);
        if let CommitteeConsensusMessage::DAVote(vote) = da_message {
            output.insert(SequencingHotShotEvent::DAVoteRecv(vote), 1);
        }
    }
    output.insert(
        SequencingHotShotEvent::DAProposalRecv(message, pub_key),
        1,
    );
    output.insert(SequencingHotShotEvent::ViewChange(ViewNumber::new(2)), 1);
    output.insert(SequencingHotShotEvent::Shutdown, 1);

    let (task_runner, event_stream) = build_harness(output).await;
    // TODO (Keyao) Why the message tasks don't seem to be necessary (and can cause hanging if there's no message sent)?
    let task_runner =
        add_network_message_task(task_runner, event_stream.clone(), quorum_exchange.clone()).await;
    let task_runner = add_network_message_task(
        task_runner,
        event_stream.clone(),
        committee_exchange.clone(),
    )
    .await;
    let task_runner = add_network_message_task(
        task_runner,
        event_stream.clone(),
        view_sync_excahnge.clone(),
    )
    .await;
    let task_runner = add_network_event_task(
        task_runner,
        event_stream.clone(),
        quorum_exchange,
        NetworkTaskKind::Quorum,
    )
    .await;
    let task_runner = add_network_event_task(
        task_runner,
        event_stream.clone(),
        committee_exchange,
        NetworkTaskKind::Committee,
    )
    .await;
    let task_runner = add_network_event_task(
        task_runner,
        event_stream.clone(),
        view_sync_excahnge,
        NetworkTaskKind::ViewSync,
    )
    .await;
    run_harness(input, task_runner, event_stream).await;
}
