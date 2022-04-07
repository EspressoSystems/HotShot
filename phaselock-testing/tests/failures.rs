#![allow(clippy::type_complexity)]
mod common;
use common::*;

use either::Either::Right;

use tracing::{instrument, warn};

use std::collections::HashSet;

// This test simulates a single permanent failed node
#[async_std::test]
#[instrument]
async fn single_permanent_failure() {
    let description = TestDescription {
        total_nodes: 7,
        txn_ids: Right((10, 1)),
        next_view_timeout: 100000,
        ids_to_shut_down: vec![6].into_iter().collect::<HashSet<_>>(),
        ..TestDescription::default()
    };
    description.execute().await.unwrap();
}

// This test simulates two permanent failed nodes
//
// With n=7, this is the maximum failures that the network can tolerate
#[async_std::test]
#[instrument]
async fn double_permanent_failure() {
    let description = TestDescription {
        total_nodes: 7,
        txn_ids: Right((10, 1)),
        next_view_timeout: 1000,
        ids_to_shut_down: vec![5, 6].into_iter().collect::<HashSet<_>>(),
        ..TestDescription::default()
    };
    description.execute().await.unwrap();
}
