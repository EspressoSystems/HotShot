use ark_bls12_381::Parameters as Param381;
use async_compatibility_layer::logging::shutdown_logging;
use blake3::Hasher;
use hotshot::traits::{
    election::{static_committee::StaticCommittee, vrf::VrfImpl},
    implementations::{CentralizedCommChannel, MemoryStorage},
};
use hotshot_testing::{
    test_builder::TestBuilder,
    test_types::{StaticCommitteeTestTypes, VrfTestTypes},
};
use hotshot_types::message::{Message, ValidatingMessage};
use hotshot_types::traits::{
    election::QuorumExchange,
    node_implementation::{ChannelMaps, NodeImplementation, ValidatingExchanges},
};
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal, ViewNumber},
    vote::QuorumVote,
};
use jf_primitives::{signatures::BLSSignatureScheme, vrf::blsvrf::BLSVRFScheme};
use serde::{Deserialize, Serialize};
use tracing::instrument;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct VrfCentralizedImp {}

type VrfMembership = VrfImpl<
    VrfTestTypes,
    ValidatingLeaf<VrfTestTypes>,
    BLSSignatureScheme<Param381>,
    BLSVRFScheme<Param381>,
    Hasher,
    Param381,
>;

type VrfCommunication = CentralizedCommChannel<
    VrfTestTypes,
    VrfCentralizedImp,
    ValidatingProposal<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
    QuorumVote<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
    VrfMembership,
>;

impl NodeImplementation<VrfTestTypes> for VrfCentralizedImp {
    type Storage = MemoryStorage<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>;
    type Leaf = ValidatingLeaf<VrfTestTypes>;
    type Exchanges = ValidatingExchanges<
        VrfTestTypes,
        Message<VrfTestTypes, Self>,
        QuorumExchange<
            VrfTestTypes,
            ValidatingLeaf<VrfTestTypes>,
            ValidatingProposal<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
            VrfMembership,
            VrfCommunication,
            Message<VrfTestTypes, Self>,
        >,
    >;
    type ConsensusMessage = ValidatingMessage<VrfTestTypes, Self>;

    fn new_channel_maps(
        start_view: ViewNumber,
    ) -> (
        ChannelMaps<VrfTestTypes, Self>,
        Option<ChannelMaps<VrfTestTypes, Self>>,
    ) {
        (ChannelMaps::new(start_view), None)
    }
}

/// Centralized server network test
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn centralized_server_network_vrf() {
    let builder = TestBuilder::default_multiple_rounds();

    builder
        .build::<VrfTestTypes, VrfCentralizedImp>()
        .launch()
        .run_test()
        .await
        .unwrap();
    shutdown_logging();
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StaticCentralizedImp {}

type StaticMembership =
    StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>;

type StaticCommunication = CentralizedCommChannel<
    StaticCommitteeTestTypes,
    StaticCentralizedImp,
    ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    QuorumVote<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
>;

impl NodeImplementation<StaticCommitteeTestTypes> for StaticCentralizedImp {
    type Storage =
        MemoryStorage<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>;
    type Leaf = ValidatingLeaf<StaticCommitteeTestTypes>;
    type Exchanges = ValidatingExchanges<
        StaticCommitteeTestTypes,
        Message<StaticCommitteeTestTypes, Self>,
        QuorumExchange<
            StaticCommitteeTestTypes,
            ValidatingLeaf<StaticCommitteeTestTypes>,
            ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
            StaticMembership,
            StaticCommunication,
            Message<StaticCommitteeTestTypes, Self>,
        >,
    >;
    type ConsensusMessage = ValidatingMessage<StaticCommitteeTestTypes, Self>;

    fn new_channel_maps(
        start_view: ViewNumber,
    ) -> (
        ChannelMaps<StaticCommitteeTestTypes, Self>,
        Option<ChannelMaps<StaticCommitteeTestTypes, Self>>,
    ) {
        (ChannelMaps::new(start_view), None)
    }
}

/// Centralized server network test
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn centralized_server_network() {
    let description = TestBuilder::default_multiple_rounds();

    description
        .build::<StaticCommitteeTestTypes, StaticCentralizedImp>()
        .launch()
        .run_test()
        .await
        .unwrap();
    shutdown_logging();
}

// This test is ignored because it doesn't pass consistently.
// stress test for a centralized server
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn test_stress_centralized_server_network() {
    let description = TestBuilder::default_stress();

    description
        .build::<StaticCommitteeTestTypes, StaticCentralizedImp>()
        .launch()
        .run_test()
        .await
        .unwrap();
}
