mod common;

use common::*;

use either::Either::Right;

use phaselock::{
    demos::dentry::{DEntryBlock, State as DemoState},
    traits::{
        implementations::{Libp2pNetwork, MemoryStorage},
        BlockContents,
    },
    types::Message,
    H_256,
};
use tracing::instrument;

/// libp2p network test
#[async_std::test]
#[instrument]
#[ignore]
async fn libp2p_network() {
    let description = GeneralTestDescriptionBuilder {
        next_view_timeout: 600,
        round_start_delay: 25,
        timeout_ratio: (1, 1),
        start_delay: 25,
        total_nodes: 15,
        start_nodes: 15,
        num_succeeds: 100,
        txn_ids: Right(1),
        ..GeneralTestDescriptionBuilder::default()
    };

    description
        .build::<Libp2pNetwork<
            Message<
                DEntryBlock,
                <DEntryBlock as BlockContents<H_256>>::Transaction,
                DemoState,
                H_256,
            >,
        >, MemoryStorage<DEntryBlock, DemoState, H_256>, DEntryBlock, DemoState>()
        .execute()
        .await
        .unwrap();
}
