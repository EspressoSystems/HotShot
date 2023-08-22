use crate::node_types::SequencingMemoryImpl;
use crate::node_types::SequencingTestTypes;
use crate::test_builder::TestMetadata;
use hotshot::traits::{NodeImplementation, TestableNodeImplementation};
use hotshot::types::bn254::BN254Pub;
use hotshot::types::SignatureKey;
use hotshot::types::SystemContextHandle;
use hotshot::{HotShotInitializer, SystemContext};
use hotshot_types::message::Message;
use hotshot_types::traits::election::Membership;
use hotshot_types::traits::metrics::NoMetrics;
use hotshot_types::traits::node_implementation::CommitteeEx;
use hotshot_types::traits::node_implementation::ExchangesType;
use hotshot_types::traits::node_implementation::QuorumEx;
use hotshot_types::traits::{election::ConsensusExchange, node_implementation::NodeType};

pub async fn build_system_handle(
    node_id: u64,
) -> SystemContextHandle<SequencingTestTypes, SequencingMemoryImpl> {
    let builder = TestMetadata::default_multiple_rounds();

    let launcher = builder.gen_launcher::<SequencingTestTypes, SequencingMemoryImpl>();

    let networks = (launcher.resource_generator.channel_generator)(node_id);
    let storage = (launcher.resource_generator.storage)(node_id);
    let config = launcher.resource_generator.config.clone();

    let initializer = HotShotInitializer::<
        SequencingTestTypes,
        <SequencingMemoryImpl as NodeImplementation<SequencingTestTypes>>::Leaf,
    >::from_genesis(<SequencingMemoryImpl as TestableNodeImplementation<
        SequencingTestTypes,
    >>::block_genesis())
    .unwrap();

    let known_nodes = config.known_nodes.clone();
    let known_nodes_with_stake = config.known_nodes_with_stake.clone();
    let private_key = <BN254Pub as SignatureKey>::generated_from_seed_indexed([0u8; 32], node_id).1;
    let public_key = <SequencingTestTypes as NodeType>::SignatureKey::from_private(&private_key);
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
            known_nodes_with_stake.clone(),
            known_nodes.clone(),
            (quorum_election_config, committee_election_config),
            networks,
            public_key,
            public_key.get_stake_table_entry(1u64),
            private_key.clone(),
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
