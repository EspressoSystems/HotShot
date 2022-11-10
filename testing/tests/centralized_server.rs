mod common;

use ark_bls12_381::Parameters as Param381;
use blake3::Hasher;
use common::*;
use either::Either::Right;
use hotshot::traits::{
    election::{static_committee::StaticCommittee, vrf::VrfImpl},
    implementations::{CentralizedServerNetwork, MemoryStorage},
};
use hotshot_testing::TestNodeImpl;
use hotshot_utils::test_util::shutdown_logging;
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
    let description = GeneralTestDescriptionBuilder {
        round_start_delay: 25,
        num_bootstrap_nodes: 5,
        timeout_ratio: (11, 10),
        total_nodes: 10,
        start_nodes: 10,
        num_succeeds: 20,
        txn_ids: Right(1),
        next_view_timeout: 10000,
        start_delay: 120000,
        ..GeneralTestDescriptionBuilder::default()
    };

    description
        .build::<VrfTestTypes, TestNodeImpl<
            VrfTestTypes,
            CentralizedServerNetwork<VrfTestTypes>,
            MemoryStorage<VrfTestTypes>,
            VrfImpl<
                VrfTestTypes,
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
    let description = GeneralTestDescriptionBuilder {
        round_start_delay: 25,
        num_bootstrap_nodes: 5,
        timeout_ratio: (11, 10),
        total_nodes: 10,
        start_nodes: 10,
        num_succeeds: 20,
        txn_ids: Right(1),
        next_view_timeout: 10000,
        start_delay: 120000,
        ..GeneralTestDescriptionBuilder::default()
    };

    description
        .build::<StaticCommitteeTestTypes, TestNodeImpl<
            StaticCommitteeTestTypes,
            CentralizedServerNetwork<StaticCommitteeTestTypes>,
            MemoryStorage<StaticCommitteeTestTypes>,
            StaticCommittee<StaticCommitteeTestTypes>,
        >>()
        .execute()
        .await
        .unwrap();
    shutdown_logging();
}

// stress test for a centralized server
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn test_stress_centralized_server_network() {
    let description = GeneralTestDescriptionBuilder {
        round_start_delay: 25,
        num_bootstrap_nodes: 15,
        timeout_ratio: (1, 1),
        total_nodes: 100,
        start_nodes: 100,
        num_succeeds: 5,
        txn_ids: Right(1),
        next_view_timeout: 2000,
        start_delay: 20000,
        ..GeneralTestDescriptionBuilder::default()
    };

    description
        .build::<StaticCommitteeTestTypes, TestNodeImpl<
            StaticCommitteeTestTypes,
            CentralizedServerNetwork<StaticCommitteeTestTypes>,
            MemoryStorage<StaticCommitteeTestTypes>,
            StaticCommittee<StaticCommitteeTestTypes>,
        >>()
        .execute()
        .await
        .unwrap();
}
