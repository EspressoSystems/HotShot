use hotshot_task::task::TaskErr;
use snafu::Snafu;

/// An overarching consensus test failure
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum ConsensusTestError {
    NotEnoughSuccessfulViews,
    /// Too many nodes failed
    TooManyFailures,
    /// too many consecutive failures
    TooManyConsecutiveFailures,
    /// HACK successul completion
    CompletedTestSuccessfully,
    /// safety violation
    ConsensusSafetyFailed {
        /// description of error
        description: String,
    },
    /// No node exists
    NoSuchNode {
        /// the existing nodes
        node_ids: Vec<u64>,
        /// the node requested
        requested_id: u64,
    },
    /// custom correctness property failed
    CustomError {
        /// description
        err: String,
    },
}

impl TaskErr for ConsensusTestError {}

/// A view specific error
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum ConsensusRoundError {
    /// inconsistent blocks
    InconsistentBlocks,
    /// inconsistent states
    InconsistentStates,
    /// inconsistent leaves
    InconsistentLeaves,
    /// lack of progress
    NoMajorityProgress,
    /// Too Many timed out nodes
    TooManyTimedOutNodes,
}
