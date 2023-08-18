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

// TODO (Keyao) This is the same as `build_consensus_api`. We should move it to a separate file.
async fn build_da_api<
    TYPES: NodeType<
        ElectionConfigType = StaticElectionConfig,
        SignatureKey = JfPubKey<BLSSignatureScheme>,
        Time = ViewNumber,
    >,
    I: TestableNodeImplementation<
        TYPES,
        Leaf = SequencingLeaf<TYPES>,
        ConsensusMessage = SequencingMessage<TYPES, I>,
        Storage = MemoryStorage<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    >,
>() -> SystemContextHandle<TYPES, I>
where
    I::Exchanges: ExchangesType<
        TYPES,
        I::Leaf,
        Message<TYPES, I>,
        ElectionConfigs = (StaticElectionConfig, StaticElectionConfig),
    >,
    SequencingQuorumEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
        Certificate = QuorumCertificate<TYPES, SequencingLeaf<TYPES>>,
        Commitment = SequencingLeaf<TYPES>,
        Membership = StaticMembership,
        Networking = StaticMemoryQuorumComm,
    >,
    CommitteeEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = DAProposal<TYPES>,
        Certificate = DACertificate<TYPES>,
        Commitment = TYPES::BlockType,
        Membership = StaticMembership,
        Networking = StaticMemoryDAComm,
    >,
    ViewSyncEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = ViewSyncCertificate<TYPES>,
        Certificate = ViewSyncCertificate<TYPES>,
        Commitment = ViewSyncData<TYPES>,
        Membership = StaticMembership,
        Networking = StaticMemoryViewSyncComm,
    >,
    // Why do we need this?
    GeneralStaticCommittee<
        SequencingTestTypes,
        SequencingLeaf<SequencingTestTypes>,
        JfPubKey<BLSSignatureScheme>,
    >: Membership<TYPES>,
{
    let builder = TestMetadata::default_multiple_rounds();

    let launcher = builder.gen_launcher::<SequencingTestTypes, SequencingMemoryImpl>();

    // In view 1, the node with id 2 is the next leader.
    let node_id = 2;
    let networks = (launcher.resource_generator.channel_generator)(node_id);
    let storage = (launcher.resource_generator.storage)(node_id);
    let config = launcher.resource_generator.config.clone();
    let initializer =
        HotShotInitializer::<TYPES, I::Leaf>::from_genesis(I::block_genesis()).unwrap();

    let known_nodes = config.known_nodes.clone();
    let private_key = I::generate_test_key(node_id);
    let public_key = TYPES::SignatureKey::from_private(&private_key);
    let ek =
        jf_primitives::aead::KeyPair::generate(&mut rand_chacha::ChaChaRng::from_seed([0u8; 32]));
    let quorum_election_config = config.election_config.clone().unwrap_or_else(|| {
        <QuorumEx<TYPES,I> as ConsensusExchange<
                TYPES,
                Message<TYPES, I>,
            >>::Membership::default_election_config(config.total_nodes.get() as u64)
    });

    let committee_election_config = config.election_config.clone().unwrap_or_else(|| {
        <CommitteeEx<TYPES,I> as ConsensusExchange<
                TYPES,
                Message<TYPES, I>,
            >>::Membership::default_election_config(config.total_nodes.get() as u64)
    });
    let exchanges = I::Exchanges::create(
        known_nodes.clone(),
        (quorum_election_config, committee_election_config),
        networks,
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
    use hotshot_task_impls::harness::run_harness;
    use hotshot_types::{message::Proposal, traits::election::CommitteeExchangeType};

    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let handle = build_da_api::<
        hotshot_testing::node_types::SequencingTestTypes,
        hotshot_testing::node_types::SequencingMemoryImpl,
    >()
    .await;

    let api: HotShotSequencingConsensusApi<SequencingTestTypes, SequencingMemoryImpl> =
        HotShotSequencingConsensusApi {
            inner: handle.hotshot.inner.clone(),
        };
    let committee_exchange = api.inner.exchanges.committee_exchange().clone();

    // Every event input is seen on the event stream in the output.
    let mut input = Vec::new();
    let mut output = HashMap::new();

    input.push(SequencingHotShotEvent::ViewChange(ViewNumber::new(1)));
    input.push(SequencingHotShotEvent::ViewChange(ViewNumber::new(2)));
    input.push(SequencingHotShotEvent::Shutdown);

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
    output.insert(SequencingHotShotEvent::SendDABlockData(block), 1);
    output.insert(
        SequencingHotShotEvent::DAProposalSend(message, api.public_key().clone()),
        1,
    );
    output.insert(SequencingHotShotEvent::ViewChange(ViewNumber::new(1)), 1);
    output.insert(SequencingHotShotEvent::ViewChange(ViewNumber::new(2)), 1);
    output.insert(SequencingHotShotEvent::Shutdown, 1);

    let build_fn = |task_runner, event_stream| {
        add_da_task(task_runner, event_stream, committee_exchange, handle)
    };

    run_harness(input, output, build_fn).await;
}
