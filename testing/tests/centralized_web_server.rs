mod common;

use ark_bls12_381::Parameters as Param381;
use blake3::Hasher;
use common::*;
use either::Either::Right;
use hotshot::traits::{
    election::{static_committee::StaticCommittee, vrf::VrfImpl},
    implementations::{CentralizedWebServerNetwork, MemoryStorage},
};
use hotshot_testing::TestNodeImpl;
use hotshot_utils::test_util::shutdown_logging;
use jf_primitives::{signatures::BLSSignatureScheme, vrf::blsvrf::BLSVRFScheme};
use tracing::instrument;

// TODO can implement like the other centralized server impl,
// which is copied below

/// Centralized web server network test
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
        total_nodes: 1,
        start_nodes: 1,
        num_succeeds: 5,
        txn_ids: Right(1),
        next_view_timeout: 10000,
        start_delay: 120000,
        ..GeneralTestDescriptionBuilder::default()
    };

    description
        .build::<StaticCommitteeTestTypes, TestNodeImpl<
            StaticCommitteeTestTypes,
            CentralizedWebServerNetwork<StaticCommitteeTestTypes>,
            MemoryStorage<StaticCommitteeTestTypes>,
            StaticCommittee<StaticCommitteeTestTypes>,
        >>()
        .execute()
        .await
        .unwrap();
    shutdown_logging();
}
