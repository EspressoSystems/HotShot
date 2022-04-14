#![allow(clippy::type_complexity)]
mod common;
use common::*;

use either::Either::Right;

use tracing::{instrument, warn};

use std::collections::HashSet;

// TODO jr: fix test
// This test simulates a single permanent failed node
#[async_std::test]
#[instrument]
#[ignore]
async fn single_permanent_failure() {
    let description = TestDescriptionBuilder {
        total_nodes: 7,
        start_nodes: 7,
        num_rounds: 10,
        txn_ids: Right(1),
        next_view_timeout: 1000,
        ids_to_shut_down: vec![vec![6].into_iter().collect::<HashSet<_>>()],
        ..TestDescriptionBuilder::default()
    }
    .build();
    description.execute().await.unwrap();
}

// TODO jr: fix test
// This test simulates two permanent failed nodes
//
// With n=7, this is the maximum failures that the network can tolerate
#[async_std::test]
#[instrument]
#[ignore]
async fn double_permanent_failure() {
    let description = TestDescriptionBuilder {
        total_nodes: 7,
        start_nodes: 7,
        num_rounds: 10,
        txn_ids: Right(1),
        next_view_timeout: 1000,
        ids_to_shut_down: vec![vec![5, 6].into_iter().collect::<HashSet<_>>()],
        ..TestDescriptionBuilder::default()
    }
    .build();
    description.execute().await.unwrap();
}
