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
    TestNodeImpl,
};
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal},
    vote::QuorumVote,
};
// use hotshot_utils::test_util::shutdown_logging;
use jf_primitives::{signatures::BLSSignatureScheme, vrf::blsvrf::BLSVRFScheme};
use tracing::instrument;

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
        .build::<VrfTestTypes, TestNodeImpl<
            VrfTestTypes,
            ValidatingLeaf<VrfTestTypes>,
            ValidatingProposal<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
            QuorumVote<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
            CentralizedCommChannel<
                VrfTestTypes,
                ValidatingProposal<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
                QuorumVote<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
                VrfImpl<
                    VrfTestTypes,
                    ValidatingLeaf<VrfTestTypes>,
                    BLSSignatureScheme<Param381>,
                    BLSVRFScheme<Param381>,
                    Hasher,
                    Param381,
                >,
            >,
            MemoryStorage<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
            VrfImpl<
                VrfTestTypes,
                ValidatingLeaf<VrfTestTypes>,
                BLSSignatureScheme<Param381>,
                BLSVRFScheme<Param381>,
                Hasher,
                Param381,
            >,
        >>()
        .execute()
        .await
        .unwrap();
    shutdown_logging();
}

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
        .build::<StaticCommitteeTestTypes, TestNodeImpl<
            StaticCommitteeTestTypes,
            ValidatingLeaf<StaticCommitteeTestTypes>,
            ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
            QuorumVote<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
            CentralizedCommChannel<
                StaticCommitteeTestTypes,
                ValidatingProposal<
                    StaticCommitteeTestTypes,
                    ValidatingLeaf<StaticCommitteeTestTypes>,
                >,
                QuorumVote<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
                StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
            >,
            MemoryStorage<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
            StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
        >>()
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
        .build::<StaticCommitteeTestTypes, TestNodeImpl<
            StaticCommitteeTestTypes,
            ValidatingLeaf<StaticCommitteeTestTypes>,
            ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
            QuorumVote<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
            CentralizedCommChannel<
                StaticCommitteeTestTypes,
                ValidatingProposal<
                    StaticCommitteeTestTypes,
                    ValidatingLeaf<StaticCommitteeTestTypes>,
                >,
                QuorumVote<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
                StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
            >,
            MemoryStorage<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
            StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
        >>()
        .execute()
        .await
        .unwrap();
}
