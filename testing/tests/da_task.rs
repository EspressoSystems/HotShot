use commit::Committable;
use hotshot::rand::SeedableRng;
use hotshot::traits::election::static_committee::GeneralStaticCommittee;
use hotshot::traits::election::static_committee::StaticElectionConfig;
use hotshot::traits::election::vrf::JfPubKey;
use hotshot::traits::implementations::MemoryStorage;
use hotshot::types::SignatureKey;
use hotshot::types::SystemContextHandle;
use hotshot::HotShotInitializer;
use hotshot::HotShotSequencingConsensusApi;
use hotshot::{certificate::QuorumCertificate, traits::TestableNodeImplementation, SystemContext};
use hotshot_consensus::traits::ConsensusSharedApi;
use hotshot_task_impls::events::SequencingHotShotEvent;
use hotshot_testing::node_types::SequencingMemoryImpl;
use hotshot_testing::node_types::SequencingTestTypes;
use hotshot_testing::node_types::{
    StaticMembership, StaticMemoryDAComm, StaticMemoryQuorumComm, StaticMemoryViewSyncComm,
};
use hotshot_testing::test_builder::TestMetadata;
use hotshot_types::certificate::ViewSyncCertificate;
use hotshot_types::data::DAProposal;
use hotshot_types::data::QuorumProposal;
use hotshot_types::data::SequencingLeaf;
use hotshot_types::data::ViewNumber;
use hotshot_types::message::Message;
use hotshot_types::message::SequencingMessage;
use hotshot_types::traits::election::Membership;
use hotshot_types::traits::metrics::NoMetrics;
use hotshot_types::traits::node_implementation::CommitteeEx;
use hotshot_types::traits::node_implementation::ExchangesType;
use hotshot_types::traits::node_implementation::QuorumEx;
use hotshot_types::traits::node_implementation::SequencingQuorumEx;
use hotshot_types::traits::node_implementation::ViewSyncEx;
use hotshot_types::traits::{
    election::ConsensusExchange, node_implementation::NodeType, state::ConsensusTime,
};
use hotshot_types::{certificate::DACertificate, vote::ViewSyncData};
use jf_primitives::signatures::BLSSignatureScheme;
use std::collections::HashMap;

#[cfg(test)]
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
async fn test_da_task() {
    use hotshot::{
        demos::sdemo::{SDemoBlock, SDemoNormalBlock},
        tasks::add_da_task,
    };
    use hotshot_task_impls::harness::{run_harness, build_api};
    use hotshot_types::{
        message::{CommitteeConsensusMessage, Proposal},
        traits::election::CommitteeExchangeType,
    };

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    // Build the API for node 2.
    let handle = build_api::<
        hotshot_testing::node_types::SequencingTestTypes,
        hotshot_testing::node_types::SequencingMemoryImpl,
    >(2)
    .await;
    let api: HotShotSequencingConsensusApi<SequencingTestTypes, SequencingMemoryImpl> =
        HotShotSequencingConsensusApi {
            inner: handle.hotshot.inner.clone(),
        };
    let committee_exchange = api.inner.exchanges.committee_exchange().clone();
    let pub_key = api.public_key().clone();
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
    input.push(SequencingHotShotEvent::DAProposalRecv(
        message.clone(),
        pub_key.clone(),
    ));
    input.push(SequencingHotShotEvent::Shutdown);

    output.insert(SequencingHotShotEvent::ViewChange(ViewNumber::new(1)), 1);
    output.insert(SequencingHotShotEvent::SendDABlockData(block), 1);
    output.insert(
        SequencingHotShotEvent::DAProposalSend(message.clone(), pub_key.clone()),
        1,
    );
    if let Ok(Some(vote_token)) = committee_exchange.make_vote_token(ViewNumber::new(2)) {
        let da_message =
            committee_exchange.create_da_message(block_commitment, ViewNumber::new(2), vote_token);
        if let CommitteeConsensusMessage::DAVote(vote) = da_message {
            output.insert(SequencingHotShotEvent::DAVoteSend(vote), 1);
        }
    }
    output.insert(
        SequencingHotShotEvent::DAProposalRecv(message, pub_key.clone()),
        1,
    );
    output.insert(SequencingHotShotEvent::ViewChange(ViewNumber::new(2)), 1);
    output.insert(SequencingHotShotEvent::Shutdown, 1);

    let build_fn = |task_runner, event_stream| {
        add_da_task(task_runner, event_stream, committee_exchange, handle)
    };

    run_harness(input, output, build_fn).await;
}
