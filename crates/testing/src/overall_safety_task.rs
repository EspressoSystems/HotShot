// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::collections::{HashMap, HashSet};

use hotshot_types::traits::node_implementation::NodeType;
use thiserror::Error;
use tracing::error;

/// convenience type alias for state and block
pub type StateAndBlock<S, B> = (Vec<S>, Vec<B>);

/// the status of a view
#[derive(Debug, Clone)]
pub enum ViewStatus<TYPES: NodeType> {
    /// success
    Ok,
    /// failure
    Failed,
    /// safety violation
    Err(OverallSafetyTaskErr<TYPES>),
    /// in progress
    InProgress,
}

/// possible errors
#[derive(Error, Debug, Clone)]
pub enum OverallSafetyTaskErr<TYPES: NodeType> {
    #[error("Mismatched leaf")]
    MismatchedLeaf,

    #[error("Inconsistent blocks")]
    InconsistentBlocks,

    #[error("Inconsistent number of transactions: {map:?}")]
    InconsistentTxnsNum { map: HashMap<u64, usize> },

    #[error("Not enough decides: got: {got}, expected: {expected}")]
    NotEnoughDecides { got: usize, expected: usize },

    #[error("Too many view failures: {0:?}")]
    TooManyFailures(HashSet<TYPES::View>),

    #[error("Inconsistent failed views: expected: {expected_failed_views:?}, actual: {actual_failed_views:?}")]
    InconsistentFailedViews {
        expected_failed_views: Vec<TYPES::View>,
        actual_failed_views: HashSet<TYPES::View>,
    },
    #[error(
        "Not enough round results: results_count: {results_count}, views_count: {views_count}"
    )]
    NotEnoughRoundResults {
        results_count: usize,
        views_count: usize,
    },

    #[error("View timed out")]
    ViewTimeout,
}

/// cross node safety properties
#[derive(Clone, Debug)]
pub struct OverallSafetyPropertiesDescription {
    /// required number of successful views
    pub num_successful_views: usize,
    /// whether or not to check the leaf
    pub check_leaf: bool,
    /// whether or not to check the block
    pub check_block: bool,
    /// whether or not to check that we have threshold amounts of transactions each block
    /// if 0: don't check
    /// if n > 0, check that at least n transactions are decided upon if such information
    /// is available
    pub transaction_threshold: u64,
    /// num of total rounds allowed to fail
    pub num_failed_views: usize,
    /// pass in the views that we expect to fail
    pub expected_view_failures: Vec<u64>,
}

impl Default for OverallSafetyPropertiesDescription {
    fn default() -> Self {
        Self {
            num_successful_views: 50,
            check_leaf: false,
            check_block: true,
            num_failed_views: 0,
            transaction_threshold: 0,
            expected_view_failures: vec![],
        }
    }
}
