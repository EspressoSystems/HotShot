mod common;

use common::*;

use either::Either::Right;

use hotshot::{
    demos::dentry::DEntryState,
    traits::implementations::{Libp2pNetwork, MemoryStorage},
    types::Message,
};
use hotshot_types::traits::signature_key::ed25519::Ed25519Pub;
use hotshot_utils::async_std_or_tokio::async_test;
use tracing::instrument;

/// libp2p network test
#[async_test]
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
        .build::<Libp2pNetwork<
            Message<
                DEntryState,
                Ed25519Pub,
            >,
            Ed25519Pub,
        >, MemoryStorage<DEntryState>, DEntryState>()
        .execute()
        .await
        .unwrap();
}

// stress test for libp2p
#[async_test]
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
        .build::<Libp2pNetwork<
            Message<
                DEntryState,
                Ed25519Pub,
            >,
            Ed25519Pub,
        >, MemoryStorage<DEntryState>, DEntryState>()
        .execute()
        .await
        .unwrap();
}
