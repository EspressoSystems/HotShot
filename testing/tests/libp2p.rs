mod common;

use common::*;

use either::Either::Right;

use hotshot::{
    demos::dentry::{DEntryBlock, State as DemoState},
    traits::{
        implementations::{Libp2pNetwork, MemoryStorage},
        BlockContents,
    },
    types::Message,
    H_256,
};
use hotshot_types::traits::signature_key::ed25519::Ed25519Pub;
use tracing::instrument;

/// libp2p network test
#[async_std::test]
#[instrument]
async fn libp2p_network() {
    let description = GeneralTestDescriptionBuilder {
        round_start_delay: 25,
        num_bootstrap_nodes: 7,
        timeout_ratio: (11, 10),
        total_nodes: 20,
        start_nodes: 20,
        num_succeeds: 20,
        txn_ids: Right(1),
        next_view_timeout: 10000,
        start_delay: 120000,
        ..GeneralTestDescriptionBuilder::default()
    };

    description
        .build::<Libp2pNetwork<
            Message<
                DEntryBlock,
                <DEntryBlock as BlockContents<H_256>>::Transaction,
                DemoState,
                Ed25519Pub,
                H_256,
            >,
            Ed25519Pub,
        >, MemoryStorage<DEntryBlock, DemoState, H_256>, DEntryBlock, DemoState>()
        .execute()
        .await
        .unwrap();
}

// stress test for libp2p
#[async_std::test]
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
                DEntryBlock,
                <DEntryBlock as BlockContents<H_256>>::Transaction,
                DemoState,
                Ed25519Pub,
                H_256,
            >,
            Ed25519Pub,
        >, MemoryStorage<DEntryBlock, DemoState, H_256>, DEntryBlock, DemoState>()
        .execute()
        .await
        .unwrap();
}
