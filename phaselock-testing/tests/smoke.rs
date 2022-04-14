#![allow(clippy::type_complexity)]
mod common;
use common::*;

use either::Either::Right;

use tracing::instrument;

#[async_std::test]
#[instrument]
async fn ten_tx_seven_nodes() {
    let description = TestDescriptionBuilder {
        total_nodes: 7,
        start_nodes: 7,
        num_rounds: 10,
        txn_ids: Right(1),
        ..TestDescriptionBuilder::default()
    }
    .build();
    description.execute().await.unwrap();
}

#[async_std::test]
#[instrument]
async fn ten_tx_five_nodes() {
    let description = TestDescriptionBuilder {
        total_nodes: 5,
        start_nodes: 5,
        num_rounds: 10,
        txn_ids: Right(1),
        ..TestDescriptionBuilder::default()
    }
    .build();
    description.execute().await.unwrap();
}
