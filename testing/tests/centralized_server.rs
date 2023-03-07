use ark_bls12_381::Parameters as Param381;
use async_compatibility_layer::logging::shutdown_logging;
use blake3::Hasher;
use hotshot::traits::{
    election::{static_committee::StaticCommittee, vrf::VrfImpl},
    implementations::{CentralizedCommChannel, MemoryStorage},
};
use hotshot_testing::{
    test_description::GeneralTestDescriptionBuilder,
    test_types::{StaticCommitteeTestTypes, VrfTestTypes},
};
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal},
    traits::network::TestableNetworkingImplementation,
    vote::QuorumVote,
};
// use hotshot_utils::test_util::shutdown_logging;
use hotshot_types::message::Message;
use hotshot_types::traits::election::QuorumExchange;
use hotshot_types::traits::node_implementation::NodeImplementation;
use hotshot_types::traits::node_implementation::TestableNodeImplementation;
use jf_primitives::{signatures::BLSSignatureScheme, vrf::blsvrf::BLSVRFScheme};
use tracing::instrument;

#[derive(Clone, Debug)]
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
    type QuorumExchange = QuorumExchange<
        VrfTestTypes,
        ValidatingLeaf<VrfTestTypes>,
        VrfMembership,
        VrfCommunication,
        Message<VrfTestTypes, Self>,
    >;
    type ComitteeExchange = Self::QuorumExchange;
}

impl TestableNodeImplementation<VrfTestTypes> for VrfCentralizedImp {}

/// Centralized server network test
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn centralized_server_network_vrf() {
    let description = GeneralTestDescriptionBuilder::default_multiple_rounds();

    description
        .build::<VrfTestTypes, VrfCentralizedImp>()
        .execute()
        .await
        .unwrap();
    shutdown_logging();
}

#[derive(Clone, Debug)]
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
    type QuorumExchange = QuorumExchange<
        StaticCommitteeTestTypes,
        ValidatingLeaf<StaticCommitteeTestTypes>,
        StaticMembership,
        StaticCommunication,
        Message<StaticCommitteeTestTypes, Self>,
    >;
    type ComitteeExchange = Self::QuorumExchange;
}

impl TestableNodeImplementation<StaticCommitteeTestTypes> for StaticCentralizedImp {}

/// Centralized server network test
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn centralized_server_network() {
    let description = GeneralTestDescriptionBuilder::default_multiple_rounds();

    description
        .build::<StaticCommitteeTestTypes, StaticCentralizedImp>()
        .execute()
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
    let description = GeneralTestDescriptionBuilder::default_stress();

    description
        .build::<StaticCommitteeTestTypes, StaticCentralizedImp>()
        .execute()
        .await
        .unwrap();
}
