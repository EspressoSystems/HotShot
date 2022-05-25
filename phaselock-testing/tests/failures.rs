#![allow(clippy::type_complexity)]
mod common;
use common::*;
use phaselock::{
    demos::dentry::{DEntryBlock, State},
    traits::implementations::{AtomicStorage, Libp2pNetwork, MemoryNetwork, MemoryStorage},
};

use either::Either::Right;

use tracing::{instrument, warn};

use std::collections::HashSet;

// TODO jr: fix test
// This test simulates a single permanent failed node
cross_all_types_ignored!(
    single_permanent_failure,
    GeneralTestDescription {
        total_nodes: 7,
        start_nodes: 7,
        num_succeeds: 10,
        txn_ids: Right(1),
        next_view_timeout: 1000,
        ids_to_shut_down: vec![vec![6].into_iter().collect::<HashSet<_>>()],
        ..GeneralTestDescription::default()
    }
);

// TODO jr: fix test
// This test simulates two permanent failed nodes
//
// With n=7, this is the maximum failures that the network can tolerate
cross_all_types_ignored!(
    double_permanent_failure,
    GeneralTestDescription {
        total_nodes: 7,
        start_nodes: 7,
        num_succeeds: 10,
        txn_ids: Right(1),
        next_view_timeout: 1000,
        ids_to_shut_down: vec![vec![5, 6].into_iter().collect::<HashSet<_>>()],
        ..GeneralTestDescription::default()
    }
);
