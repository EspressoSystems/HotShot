#![allow(clippy::type_complexity)]
mod common;
use common::*;

use either::Either::Right;

use tracing::instrument;

#[async_std::test]
#[instrument]
async fn ten_tx_seven_nodes() {
    let description = TestDescription {
        total_nodes: 7,
        start_nodes: 7,
        txn_ids: Right((10, 1)),
        ..TestDescription::default()
    };
    description
        .default_populate_rounds()
        .execute()
        .await
        .unwrap();
}

#[async_std::test]
#[instrument]
async fn ten_tx_five_nodes() {
    let description = TestDescription {
        total_nodes: 5,
        start_nodes: 5,
        txn_ids: Right((10, 1)),
        ..TestDescription::default()
    };
    description
        .default_populate_rounds()
        .execute()
        .await
        .unwrap();
}
