//! Types and Traits for the `HotShot` consensus module
#![warn(
    clippy::all,
    clippy::pedantic,
    rust_2018_idioms,
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::panic
)]
#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]

use std::{
    num::{NonZeroUsize},
    time::Duration,
};

pub mod constants;
pub mod data;
pub mod error;
pub mod event;
pub mod message;
pub mod traits;

/// the type of consensus to run. Either:
/// wait for a signal to start a view,
/// or constantly run
/// you almost always want continuous
/// incremental is just for testing
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum ExecutionType {
    /// constantly increment view as soon as view finishes
    Continuous,
    /// wait for a signal
    Incremental,
}

/// Holds configuration for a `HotShot`
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
pub struct HotShotConfig<K, ELECTION> {
    /// Whether to run one view or continuous views
    pub execution_type: ExecutionType,
    /// Total number of nodes in the network
    pub total_nodes: NonZeroUsize,
    /// Minimum transactions per block
    pub min_transactions: usize,
    /// Maximum transactions per block
    pub max_transactions: NonZeroUsize,
    /// List of known node's public keys, including own, sorted by nonce ()
    pub known_nodes: Vec<K>,
    /// Base duration for next-view timeout, in milliseconds
    pub next_view_timeout: u64,
    /// The exponential backoff ration for the next-view timeout
    pub timeout_ratio: (u64, u64),
    /// The delay a leader inserts before starting pre-commit, in milliseconds
    pub round_start_delay: u64,
    /// Delay after init before starting consensus, in milliseconds
    pub start_delay: u64,
    /// Number of network bootstrap nodes
    pub num_bootstrap: usize,
    /// The minimum amount of time a leader has to wait to start a round
    pub propose_min_round_time: Duration,
    /// The maximum amount of time a leader can wait to start a round
    pub propose_max_round_time: Duration,
    /// the election configuration
    pub election_config: Option<ELECTION>,
}
