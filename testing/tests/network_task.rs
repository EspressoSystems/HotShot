use async_compatibility_layer::art::async_spawn;
use commit::Committable;
use hotshot::HotShotSequencingConsensusApi;
use hotshot_consensus::traits::ConsensusSharedApi;
use hotshot_task_impls::events::SequencingHotShotEvent;
use hotshot_testing::node_types::SequencingMemoryImpl;
use hotshot_testing::node_types::SequencingTestTypes;
use hotshot_testing::task_helpers::build_quorum_proposal;
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
    use hotshot_task_impls::{harness::run_harness, network::NetworkTaskKind};
    use hotshot_testing::task_helpers::build_system_handle;
    use hotshot_types::{
        message::{CommitteeConsensusMessage, Proposal},
        traits::election::CommitteeExchangeType,
    };

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    // Build the API for node 2.
    let (handle, event_stream) = build_system_handle(2).await;
    let api: HotShotSequencingConsensusApi<SequencingTestTypes, SequencingMemoryImpl> =
        HotShotSequencingConsensusApi {
            inner: handle.hotshot.inner.clone(),
        };
    let quorum_exchange = api.inner.exchanges.quorum_exchange().clone();
    let committee_exchange = api.inner.exchanges.committee_exchange().clone();
    let view_sync_excahnge = api.inner.exchanges.view_sync_exchange().clone();
    let pub_key = *api.public_key();
    let priv_key = api.private_key();
    let block = SDemoBlock::Normal(SDemoNormalBlock {
        previous_state: (),
        transactions: Vec::new(),
    });
    let block_commitment = block.commit();
    let signature = committee_exchange.sign_da_proposal(&block_commitment);
    let da_proposal = Proposal {
        data: DAProposal {
            deltas: block.clone(),
            view_number: ViewNumber::new(2),
        },
        signature,
    };
    let quorum_proposal = build_quorum_proposal(&handle, &priv_key, 2).await;

    // Every event input is seen on the event stream in the output.
    let mut input = Vec::new();
    let mut output = HashMap::new();

    input.push(SequencingHotShotEvent::ViewChange(ViewNumber::new(1)));
    input.push(SequencingHotShotEvent::ViewChange(ViewNumber::new(2)));
    input.push(SequencingHotShotEvent::DAProposalSend(
        da_proposal.clone(),
        pub_key.clone(),
    ));
    input.push(SequencingHotShotEvent::QuorumProposalSend(
        quorum_proposal.clone(),
        pub_key,
    ));
    input.push(SequencingHotShotEvent::Shutdown);

    output.insert(SequencingHotShotEvent::ViewChange(ViewNumber::new(1)), 2);
    output.insert(SequencingHotShotEvent::ViewChange(ViewNumber::new(2)), 2);
    output.insert(SequencingHotShotEvent::SendDABlockData(block), 1);
    output.insert(
        SequencingHotShotEvent::DAProposalSend(da_proposal.clone(), pub_key),
        2,
    );
    output.insert(
        SequencingHotShotEvent::DAProposalRecv(da_proposal, pub_key),
        1,
    );
    output.insert(
        SequencingHotShotEvent::QuorumProposalSend(quorum_proposal.clone(), pub_key),
        2,
    );
    output.insert(
        SequencingHotShotEvent::QuorumProposalRecv(quorum_proposal, pub_key),
        1,
    );
    output.insert(SequencingHotShotEvent::Shutdown, 1);

    let build_fn = |task_runner, _| async { task_runner };
    run_harness(input, output, Some(event_stream), build_fn).await;
}
