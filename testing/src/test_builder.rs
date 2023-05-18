use std::num::NonZeroUsize;
use std::{sync::Arc, time::Duration};

use hotshot::traits::TestableNodeImplementation;
use hotshot::{traits::NetworkReliability, HotShotError};

use hotshot_types::traits::node_implementation::NodeType;

use snafu::Snafu;

use crate::round::Round;
use crate::round_builder::{RoundSafetyCheckBuilder, RoundSetupBuilder};

use crate::test_launcher::TestLauncher;

/// metadata describing a test
#[derive(Clone, Debug)]
pub struct TestMetadata {
    /// Total number of nodes in the test
    pub total_nodes: usize,
    /// nodes available at start
    pub start_nodes: usize,
    /// number of bootstrap nodes
    pub num_bootstrap_nodes: usize,
    /// number of successful/passing rounds required for test to pass
    /// equivalent to num_sucessful_views
    pub num_succeeds: usize,
    /// max number of failing rounds before test is failed
    pub failure_threshold: usize,
    /// number of txn per round
    /// TODO in the future we should make this sample from a distribution
    /// much like how network reliability is implemented
    pub num_txns_per_round: usize,
    /// Description of the network reliability
    /// `None` == perfect network
    pub network_reliability: Option<Arc<dyn NetworkReliability>>,
    /// Maximum transactions allowed in a block
    pub max_transactions: NonZeroUsize,
    /// Minimum transactions required for a block
    pub min_transactions: usize,
    /// timing data
    pub timing_data: TimingData,
    /// Size of the DA committee for the test.  0 == no DA.
    pub da_committee_size: usize,
}

impl Default for TestMetadata {
    /// by default, just a single round
    fn default() -> Self {
        Self {
            total_nodes: 5,
            start_nodes: 5,
            num_succeeds: 10,
            failure_threshold: 10,
            network_reliability: None,
            num_bootstrap_nodes: 5,
            max_transactions: NonZeroUsize::new(999999).unwrap(),
            min_transactions: 0,
            timing_data: TimingData::default(),
            num_txns_per_round: 20,
            da_committee_size: 0,
        }
    }
}

impl TestMetadata {
    /// generate a reasonable round description
    pub fn gen_sane_round<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(
        metadata: &TestMetadata,
    ) -> Round<TYPES, I> {
        Round {
            setup_round: RoundSetupBuilder {
                num_txns_per_round: metadata.num_txns_per_round,
                ..RoundSetupBuilder::default()
            }
            .build(),
            safety_check: RoundSafetyCheckBuilder {
                num_failed_rounds_total: metadata.failure_threshold,
                ..RoundSafetyCheckBuilder::default()
            }
            .build(),
            hooks: vec![],
        }
    }
}

impl Default for TimingData {
    fn default() -> Self {
        Self {
            next_view_timeout: 10000,
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            propose_min_round_time: Duration::new(0, 0),
            propose_max_round_time: Duration::new(5, 0),
        }
    }
}

impl TestBuilder {
    /// Default constructor for multiple rounds.
    pub fn default_multiple_rounds() -> Self {
        TestBuilder {
            metadata: TestMetadata {
                total_nodes: 10,
                start_nodes: 10,
                num_succeeds: 20,
                timing_data: TimingData {
                    start_delay: 120000,
                    round_start_delay: 25,
                    ..TimingData::default()
                },
                ..TestMetadata::default()
            },
            ..TestBuilder::default()
        }
    }

    pub fn default_multiple_rounds_da() -> Self {
        TestBuilder {
            metadata: TestMetadata {
                total_nodes: 10,
                start_nodes: 10,
                num_succeeds: 20,
                timing_data: TimingData {
                    start_delay: 120000,
                    round_start_delay: 25,
                    ..TimingData::default()
                },
                da_committee_size: 5,
                ..TestMetadata::default()
            },
            ..TestBuilder::default()
        }
    }

    /// Default constructor for stress testing.
    pub fn default_stress() -> Self {
        TestBuilder {
            metadata: TestMetadata {
                num_bootstrap_nodes: 15,
                total_nodes: 100,
                start_nodes: 100,
                num_succeeds: 5,
                timing_data: TimingData {
                    next_view_timeout: 2000,
                    timeout_ratio: (1, 1),
                    start_delay: 20000,
                    round_start_delay: 25,
                    ..TimingData::default()
                },
                ..TestMetadata::default()
            },
            ..TestBuilder::default()
        }
    }
}

#[derive(Debug, Snafu)]
enum RoundError<TYPES: NodeType> {
    HotShot { source: HotShotError<TYPES> },
}

/// data describing how a round should be timed.
#[derive(Clone, Debug, Copy)]
pub struct TimingData {
    /// Base duration for next-view timeout, in milliseconds
    pub next_view_timeout: u64,
    /// The exponential backoff ration for the next-view timeout
    pub timeout_ratio: (u64, u64),
    /// The delay a leader inserts before starting pre-commit, in milliseconds
    pub round_start_delay: u64,
    /// Delay after init before starting consensus, in milliseconds
    pub start_delay: u64,
    /// The minimum amount of time a leader has to wait to start a round
    pub propose_min_round_time: Duration,
    /// The maximum amount of time a leader can wait to start a round
    pub propose_max_round_time: Duration,
}

/// Builder for a test
#[derive(Default)]
pub struct TestBuilder {
    /// metadata with which to generate round
    pub metadata: TestMetadata,
    /// optional override to build safety check
    pub check: Option<RoundSafetyCheckBuilder>,
    /// optional override to build setup
    pub setup: Option<RoundSetupBuilder>,
}

impl TestBuilder {
    /// build a test description from a detailed testing spec
    pub fn build<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(
        self,
    ) -> TestLauncher<TYPES, I> {
        let sane_round = TestMetadata::gen_sane_round(&self.metadata);

        let setup_round = self
            .setup
            .map(|x| x.build())
            .unwrap_or_else(|| sane_round.setup_round);
        let safety_check = self
            .check
            .map(|x| x.build())
            .unwrap_or_else(|| sane_round.safety_check);

        let round = Round {
            hooks: vec![],
            setup_round,
            safety_check,
        };
        TestLauncher::new(self.metadata, round)
    }
}

/// given `num_nodes`, calculate min number of honest nodes
/// for consensus to function properly
pub fn get_threshold(num_nodes: u64) -> u64 {
    ((num_nodes * 2) / 3) + 1
}

/// given `num_nodes`, calculate max number of byzantine nodes
pub fn get_tolerance(num_nodes: u64) -> u64 {
    num_nodes - get_threshold(num_nodes)
}
