mod common;

use common::*;

use either::Either::Right;

use hotshot::{
    traits::{
        election::static_committee::StaticCommittee,
        implementations::{MemoryStorage, Libp2pCommChannel},
    },
};
use hotshot_testing::TestNodeImpl;
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal},
};
use tracing::instrument;

/// libp2p network test
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn libp2p_network() {
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
            ValidatingLeaf<StaticCommitteeTestTypes>,
            ValidatingProposal<
                StaticCommitteeTestTypes,
                StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
            >,
            Libp2pCommChannel<
                StaticCommitteeTestTypes,
                ValidatingLeaf<StaticCommitteeTestTypes>,
                ValidatingProposal<
                    StaticCommitteeTestTypes,
                    StaticCommittee<
                        StaticCommitteeTestTypes,
                        ValidatingLeaf<StaticCommitteeTestTypes>,
                    >,
                >,
            >,
            MemoryStorage<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
            StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
        >>()
        .execute()
        .await
        .unwrap();
}

// stress test for libp2p
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn test_stress_libp2p_network() {
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
            ValidatingLeaf<StaticCommitteeTestTypes>,
            ValidatingProposal<
                StaticCommitteeTestTypes,
                StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
            >,
            Libp2pCommChannel<
                StaticCommitteeTestTypes,
                ValidatingLeaf<StaticCommitteeTestTypes>,
                ValidatingProposal<
                    StaticCommitteeTestTypes,
                    StaticCommittee<
                        StaticCommitteeTestTypes,
                        ValidatingLeaf<StaticCommitteeTestTypes>,
                    >,
                >,
            >,
            MemoryStorage<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
            StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
        >>()
        .execute()
        .await
        .unwrap();
}
