use commit::Committable;
use either::Right;
use hotshot::certificate::QuorumCertificate;
use hotshot::tasks::add_consensus_task;
use hotshot::traits::Block;
use hotshot::types::bn254::BN254Pub;
use hotshot::types::SignatureKey;
use hotshot::types::SystemContextHandle;
use hotshot::HotShotSequencingConsensusApi;
use hotshot_consensus::traits::ConsensusSharedApi;
use hotshot_task::event_stream::ChannelStream;
use hotshot_task_impls::events::SequencingHotShotEvent;
use hotshot_task_impls::harness::run_harness;
use hotshot_testing::node_types::SequencingMemoryImpl;
use hotshot_testing::node_types::SequencingTestTypes;
use hotshot_types::data::QuorumProposal;
use hotshot_types::data::SequencingLeaf;
use hotshot_types::data::ViewNumber;
use hotshot_types::message::GeneralConsensusMessage;
use hotshot_types::message::Proposal;
use hotshot_types::traits::election::QuorumExchangeType;
use hotshot_types::traits::election::SignedCertificate;
use hotshot_types::traits::node_implementation::ExchangesType;
use hotshot_types::traits::signature_key::EncodedSignature;
use hotshot_types::traits::{
    election::ConsensusExchange, node_implementation::NodeType, state::ConsensusTime,
};

use std::collections::HashMap;

async fn build_proposal(
    handle: &SystemContextHandle<SequencingTestTypes, SequencingMemoryImpl>,
    private_key: &<BN254Pub as SignatureKey>::PrivateKey,
) -> Proposal<QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>> {
    let (proposal, signature) = build_proposal_and_signature(handle, private_key).await;
    Proposal {
        data: proposal,
        signature,
    }
}

async fn build_proposal_and_signature(
    handle: &SystemContextHandle<SequencingTestTypes, SequencingMemoryImpl>,
    private_key: &<BN254Pub as SignatureKey>::PrivateKey,
) -> (
    QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    EncodedSignature,
) {
    let consensus_lock = handle.get_consensus();
    let consensus = consensus_lock.read().await;
    let api: HotShotSequencingConsensusApi<SequencingTestTypes, SequencingMemoryImpl> =
        HotShotSequencingConsensusApi {
            inner: handle.hotshot.inner.clone(),
        };
    let _quorum_exchange = api.inner.exchanges.quorum_exchange().clone();

    let parent_view_number = &consensus.high_qc.view_number();
    let Some(parent_view) = consensus.state_map.get(parent_view_number) else {
                    panic!("Couldn't find high QC parent in state map.");
                };
    let Some(leaf) = parent_view.get_leaf_commitment() else {
                    panic!(
                        "Parent of high QC points to a view without a proposal"
                    );
                };
    let Some(leaf) = consensus.saved_leaves.get(&leaf) else {
                    panic!("Failed to find high QC parent.");
                };
    let parent_leaf = leaf.clone();

    // every event input is seen on the event stream in the output.

    let block_commitment = <SequencingTestTypes as NodeType>::BlockType::new().commit();
    let leaf = SequencingLeaf {
        view_number: ViewNumber::new(1),
        height: parent_leaf.height + 1,
        justify_qc: consensus.high_qc.clone(),
        parent_commitment: parent_leaf.commit(),
        // Use the block commitment rather than the block, so that the replica can construct
        // the same leaf with the commitment.
        deltas: Right(block_commitment),
        rejected: vec![],
        timestamp: 0,
        proposer_id: api.public_key().to_bytes(),
    };
    let signature = <BN254Pub as SignatureKey>::sign(private_key, leaf.commit().as_ref());
    let proposal = QuorumProposal::<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>> {
        block_commitment,
        view_number: ViewNumber::new(1),
        height: 1,
        justify_qc: QuorumCertificate::genesis(),
        timeout_certificate: None, 
        proposer_id: leaf.proposer_id,
        dac: None,
    };

    (proposal, signature)
}

fn key_pair_for_id(node_id: u64) -> (<BN254Pub as SignatureKey>::PrivateKey, BN254Pub) {
    let private_key = <BN254Pub as SignatureKey>::generated_from_seed_indexed([0u8; 32], node_id).1;
    let public_key = <SequencingTestTypes as NodeType>::SignatureKey::from_private(&private_key);
    (private_key, public_key)
}

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
            panic!(
                "Genesis view points to a view without a leaf"
            );
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
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
async fn test_consensus_task() {
    use hotshot_testing::system_handle::build_system_handle;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(1).await;
    let (private_key, public_key) = key_pair_for_id(1);

    let mut input = Vec::new();
    let mut output = HashMap::new();

    input.push(SequencingHotShotEvent::ViewChange(ViewNumber::new(1)));
    input.push(SequencingHotShotEvent::ViewChange(ViewNumber::new(2)));
    input.push(SequencingHotShotEvent::Shutdown);

    output.insert(
        SequencingHotShotEvent::QuorumProposalSend(
            build_proposal(&handle, &private_key).await,
            public_key,
        ),
        1,
    );
    output.insert(SequencingHotShotEvent::ViewChange(ViewNumber::new(1)), 2);
    output.insert(SequencingHotShotEvent::ViewChange(ViewNumber::new(2)), 2);
    output.insert(SequencingHotShotEvent::Shutdown, 1);

    let build_fn = |task_runner, event_stream| {
        add_consensus_task(task_runner, event_stream, ChannelStream::new(), handle)
    };

    run_harness(input, output, build_fn).await;
}

#[cfg(test)]
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
async fn test_consensus_vote() {
    use hotshot_testing::system_handle::build_system_handle;

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_system_handle(2).await;
    let (private_key, public_key) = key_pair_for_id(1);

    let mut input = Vec::new();
    let mut output = HashMap::new();

    let proposal = build_proposal(&handle, &private_key).await;

    input.push(SequencingHotShotEvent::ViewChange(ViewNumber::new(1)));
    input.push(SequencingHotShotEvent::QuorumProposalRecv(
        proposal.clone(),
        public_key,
    ));

    input.push(SequencingHotShotEvent::Shutdown);

    output.insert(
        SequencingHotShotEvent::QuorumProposalRecv(proposal.clone(), public_key),
        1,
    );
    let proposal = proposal.data;
    if let GeneralConsensusMessage::Vote(vote) =
        build_vote(&handle, proposal, ViewNumber::new(1)).await
    {
        output.insert(SequencingHotShotEvent::QuorumVoteSend(vote), 1);
    }
    output.insert(SequencingHotShotEvent::ViewChange(ViewNumber::new(1)), 2);
    output.insert(SequencingHotShotEvent::ViewChange(ViewNumber::new(2)), 1);
    output.insert(SequencingHotShotEvent::Shutdown, 1);

    let build_fn = |task_runner, event_stream| {
        add_consensus_task(task_runner, event_stream, ChannelStream::new(), handle)
    };

    run_harness(input, output, build_fn).await;
}
