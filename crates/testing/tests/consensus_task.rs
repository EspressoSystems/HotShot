use commit::Commitment;
use commit::Committable;
use either::Right;
use hotshot::{
    tasks::add_consensus_task,
    types::{SignatureKey, SystemContextHandle},
    HotShotSequencingConsensusApi,
};
use hotshot_task::event_stream::ChannelStream;
use hotshot_task_impls::events::SequencingHotShotEvent;
use hotshot_testing::{
    node_types::{SequencingMemoryImpl, SequencingTestTypes},
    task_helpers::{build_quorum_proposal, key_pair_for_id},
};
use hotshot_types::{
    data::{QuorumProposal, SequencingLeaf, ViewNumber},
    message::GeneralConsensusMessage,
    traits::{
        election::{ConsensusExchange, QuorumExchangeType, SignedCertificate},
        node_implementation::ExchangesType,
        state::ConsensusTime,
    },
};

use std::collections::HashMap;

async fn build_vote(
    handle: &SystemContextHandle<SequencingTestTypes, SequencingMemoryImpl>,
    proposal: QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    view: ViewNumber,
) -> GeneralConsensusMessage<SequencingTestTypes, SequencingMemoryImpl> {
    let consensus_lock = handle.get_consensus();
    let consensus = consensus_lock.read().await;
    let api: HotShotSequencingConsensusApi<SequencingTestTypes, SequencingMemoryImpl> =
        HotShotSequencingConsensusApi {
            inner: handle.hotshot.inner.clone(),
        };
    let quorum_exchange = api.inner.exchanges.quorum_exchange().clone();
    let vote_token = quorum_exchange.make_vote_token(view).unwrap().unwrap();

    let justify_qc = proposal.justify_qc.clone();
    let view = ViewNumber::new(*proposal.view_number);
    let parent = if justify_qc.is_genesis() {
        let Some(genesis_view) = consensus.state_map.get(&ViewNumber::new(0)) else {
            panic!("Couldn't find genesis view in state map.");
        };
        let Some(leaf) = genesis_view.get_leaf_commitment() else {
            panic!("Genesis view points to a view without a leaf");
        };
        let Some(leaf) = consensus.saved_leaves.get(&leaf) else {
            panic!("Failed to find genesis leaf.");
        };
        leaf.clone()
    } else {
        consensus
            .saved_leaves
            .get(&justify_qc.leaf_commitment())
            .cloned()
            .unwrap()
    };

    let parent_commitment = parent.commit();

    let leaf: SequencingLeaf<_> = SequencingLeaf {
        view_number: view,
        height: proposal.height,
        justify_qc: proposal.justify_qc.clone(),
        parent_commitment,
        deltas: Right(proposal.block_commitment),
        rejected: Vec::new(),
        timestamp: 0,
        proposer_id: quorum_exchange.get_leader(view).to_bytes(),
    };

    quorum_exchange.create_yes_message(
        proposal.justify_qc.commit(),
        leaf.commit(),
        view,
        vote_token,
    )
}

#[cfg(test)]
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
#[ignore]
async fn test_consensus_task() {
    use hotshot_task_impls::harness::run_harness;
    use hotshot_testing::task_helpers::build_system_handle;
    use hotshot_types::certificate::QuorumCertificate;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(1).await.0;
    let (private_key, public_key) = key_pair_for_id(1);

    let mut input = Vec::new();
    let mut output = HashMap::new();

    // Trigger a proposal to send by creating a new QC.  Then recieve that proposal and update view based on the valid QC in the proposal
    let qc = QuorumCertificate::<
        SequencingTestTypes,
        Commitment<SequencingLeaf<SequencingTestTypes>>,
    >::genesis();
    let proposal = build_quorum_proposal(&handle, &private_key, 1).await;

    input.push(SequencingHotShotEvent::QCFormed(either::Left(qc.clone())));
    input.push(SequencingHotShotEvent::QuorumProposalRecv(
        proposal.clone(),
        public_key,
    ));
    input.push(SequencingHotShotEvent::Shutdown);

    output.insert(SequencingHotShotEvent::QCFormed(either::Left(qc)), 1);
    output.insert(
        SequencingHotShotEvent::QuorumProposalSend(proposal.clone(), public_key),
        1,
    );
    output.insert(
        SequencingHotShotEvent::QuorumProposalRecv(proposal.clone(), public_key),
        1,
    );
    output.insert(SequencingHotShotEvent::ViewChange(ViewNumber::new(1)), 1);
    output.insert(SequencingHotShotEvent::Shutdown, 1);

    let build_fn = |task_runner, event_stream| {
        add_consensus_task(task_runner, event_stream, ChannelStream::new(), handle)
    };

    run_harness(input, output, None, build_fn).await;
}

#[cfg(test)]
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
async fn test_consensus_vote() {
    use hotshot_task_impls::harness::run_harness;
    use hotshot_testing::task_helpers::build_system_handle;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(2).await.0;
    let (private_key, public_key) = key_pair_for_id(1);

    let mut input = Vec::new();
    let mut output = HashMap::new();

    let proposal = build_quorum_proposal(&handle, &private_key, 1).await;

    // Send a proposal, vote on said proposal, update view based on proposal QC, receive vote as next leader
    input.push(SequencingHotShotEvent::QuorumProposalRecv(
        proposal.clone(),
        public_key,
    ));
    output.insert(
        SequencingHotShotEvent::QuorumProposalRecv(proposal.clone(), public_key),
        1,
    );
    let proposal = proposal.data;
    if let GeneralConsensusMessage::Vote(vote) =
        build_vote(&handle, proposal, ViewNumber::new(1)).await
    {
        output.insert(SequencingHotShotEvent::QuorumVoteSend(vote.clone()), 1);
        input.push(SequencingHotShotEvent::QuorumVoteRecv(vote.clone()));
        output.insert(SequencingHotShotEvent::QuorumVoteRecv(vote), 1);
    }

    output.insert(SequencingHotShotEvent::ViewChange(ViewNumber::new(1)), 1);

    input.push(SequencingHotShotEvent::Shutdown);
    output.insert(SequencingHotShotEvent::Shutdown, 1);

    let build_fn = |task_runner, event_stream| {
        add_consensus_task(task_runner, event_stream, ChannelStream::new(), handle)
    };

    run_harness(input, output, None, build_fn).await;
}
