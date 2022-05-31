mod common;
use common::*;

use either::Either::Right;

use phaselock::{
    demos::dentry::{DEntryBlock, State},
    traits::implementations::{AtomicStorage, Libp2pNetwork, MemoryNetwork, MemoryStorage},
};
use tracing::instrument;

cross_all_types!(
    ten_tx_five_nodes,
    GeneralTestDescriptionBuilder {
        total_nodes: 5,
        start_nodes: 5,
        num_succeeds: 10,
        txn_ids: Right(1),
        ..GeneralTestDescriptionBuilder::default()
    },
    keep: true
);

cross_all_types!(
    ten_tx_seven_nodes,
    GeneralTestDescriptionBuilder {
        total_nodes: 7,
        start_nodes: 7,
        num_succeeds: 10,
        txn_ids: Right(1),
        ..GeneralTestDescriptionBuilder::default()
    },
    keep: true
);
