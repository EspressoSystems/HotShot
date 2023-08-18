use commit::Committable;
use either::Right;
use hotshot::traits::NodeImplementation;
use hotshot::types::SystemContextHandle;
use hotshot::{certificate::QuorumCertificate, traits::TestableNodeImplementation, SystemContext};

use hotshot::rand::SeedableRng;

use hotshot::tasks::add_consensus_task;

use hotshot::traits::Block;
use hotshot::types::SignatureKey;
use hotshot::HotShotInitializer;
use hotshot::HotShotSequencingConsensusApi;
use hotshot_consensus::traits::ConsensusSharedApi;
use hotshot_task::event_stream::ChannelStream;

use hotshot_types::message::Proposal;

use hotshot_task_impls::events::SequencingHotShotEvent;
use hotshot_testing::node_types::SequencingMemoryImpl;
use hotshot_testing::node_types::SequencingTestTypes;

use hotshot_testing::test_builder::TestMetadata;

use hotshot_types::data::QuorumProposal;
use hotshot_types::data::SequencingLeaf;
use hotshot_types::data::ViewNumber;
use hotshot_types::message::Message;

use hotshot_types::traits::consensus_type::sequencing_consensus::SequencingConsensus;

use hotshot_types::traits::election::Membership;

use hotshot_types::traits::election::QuorumExchangeType;
use hotshot_types::traits::election::SignedCertificate;
use hotshot_types::traits::metrics::NoMetrics;

use hotshot_types::traits::node_implementation::CommitteeEx;
use hotshot_types::traits::node_implementation::ExchangesType;

use hotshot_types::traits::node_implementation::QuorumEx;

use hotshot_task_impls::harness::run_harness;

use hotshot_types::traits::{
    election::ConsensusExchange, node_implementation::NodeType, state::ConsensusTime,
};

use std::collections::HashMap;

use std::sync::Arc;

async fn build_consensus_api() -> SystemContextHandle<SequencingTestTypes, SequencingMemoryImpl> {
    let builder = TestMetadata::default_multiple_rounds();

    let launcher = builder.gen_launcher::<SequencingTestTypes, SequencingMemoryImpl>();

    let node_id = 1;
    let network_generator = Arc::new((launcher.resource_generator.network_generator)(node_id));
    let quorum_network = (launcher.resource_generator.quorum_network)(network_generator.clone());
    let committee_network =
        (launcher.resource_generator.committee_network)(network_generator.clone());
    let view_sync_network = (launcher.resource_generator.view_sync_network)(network_generator);
    let storage = (launcher.resource_generator.storage)(node_id);
    let config = launcher.resource_generator.config.clone();
    let initializer = HotShotInitializer::<
        SequencingTestTypes,
        <SequencingMemoryImpl as NodeImplementation<SequencingTestTypes>>::Leaf,
    >::from_genesis(<SequencingMemoryImpl as TestableNodeImplementation<
        SequencingConsensus,
        SequencingTestTypes,
    >>::block_genesis())
    .unwrap();

    let known_nodes = config.known_nodes.clone();
    let private_key = <SequencingMemoryImpl as TestableNodeImplementation<
        SequencingConsensus,
        SequencingTestTypes,
    >>::generate_test_key(node_id);
    let public_key = <SequencingTestTypes as NodeType>::SignatureKey::from_private(&private_key);
    let ek =
        jf_primitives::aead::KeyPair::generate(&mut rand_chacha::ChaChaRng::from_seed([0u8; 32]));
    let quorum_election_config = config.election_config.clone().unwrap_or_else(|| {
        <QuorumEx<SequencingTestTypes, SequencingMemoryImpl> as ConsensusExchange<
            SequencingTestTypes,
            Message<SequencingTestTypes, SequencingMemoryImpl>,
        >>::Membership::default_election_config(config.total_nodes.get() as u64)
    });

    let committee_election_config = config.election_config.clone().unwrap_or_else(|| {
        <CommitteeEx<SequencingTestTypes, SequencingMemoryImpl> as ConsensusExchange<
            SequencingTestTypes,
            Message<SequencingTestTypes, SequencingMemoryImpl>,
        >>::Membership::default_election_config(config.total_nodes.get() as u64)
    });
    let exchanges =
        <SequencingMemoryImpl as NodeImplementation<SequencingTestTypes>>::Exchanges::create(
            known_nodes.clone(),
            (quorum_election_config, committee_election_config),
            (quorum_network, view_sync_network, committee_network),
            public_key.clone(),
            private_key.clone(),
            ek.clone(),
        );
    SystemContext::init(
        public_key,
        private_key,
        node_id,
        config,
        storage,
        exchanges,
        initializer,
        NoMetrics::boxed(),
    )
    .await
    .expect("Could not init hotshot")
}

async fn build_proposal(
    handle: &SystemContextHandle<SequencingTestTypes, SequencingMemoryImpl>,
    _block: <SequencingTestTypes as NodeType>::BlockType,
) -> SequencingHotShotEvent<SequencingTestTypes, SequencingMemoryImpl> {
    let consensus_lock = handle.get_consensus();
    let consensus = consensus_lock.read().await;
    let api: HotShotSequencingConsensusApi<SequencingTestTypes, SequencingMemoryImpl> =
        HotShotSequencingConsensusApi {
            inner: handle.hotshot.inner.clone(),
        };

    let quorum_exchange = api.inner.exchanges.quorum_exchange().clone();

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
    let signature = quorum_exchange
        .sign_validating_or_commitment_proposal::<SequencingMemoryImpl>(&leaf.commit());
    let proposal = QuorumProposal::<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>> {
        block_commitment,
        view_number: ViewNumber::new(1),
        height: 1,
        justify_qc: QuorumCertificate::genesis(),
        proposer_id: leaf.proposer_id,
        dac: None,
    };
    let message = Proposal {
        data: proposal,
        signature,
    };
    SequencingHotShotEvent::QuorumProposalSend(message, api.public_key().clone())
}

#[cfg(test)]
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
async fn test_consensus_task() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_consensus_api().await;

    let mut input = Vec::new();
    let mut output = HashMap::new();

    input.push(SequencingHotShotEvent::ViewChange(ViewNumber::new(1)));
    input.push(SequencingHotShotEvent::ViewChange(ViewNumber::new(2)));
    input.push(SequencingHotShotEvent::Shutdown);

    output.insert(
        build_proposal(&handle, <SequencingTestTypes as NodeType>::BlockType::new()).await,
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
