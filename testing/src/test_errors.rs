use snafu::Snafu;

#[derive(Debug, Snafu)]
/// Error that is returned from [`TestRunner`] with methods related to transactions
pub enum TransactionError {
    /// There are no valid nodes online
    NoNodes,
    /// There are no valid balances available
    NoValidBalance,
    /// FIXME remove this entirely
    /// The requested node does not exist
    InvalidNode,
}

/// Overarchign errors encountered
/// when trying to reach consensus
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum ConsensusFailedError {
    /// Safety condition failed
    SafetyFailed {
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

    /// View times out with any node as the leader.
    TimedOutWithoutAnyLeader,

    NoTransactionsSubmitted,

    /// replicas timed out
    ReplicasTimedOut,

    /// States after a round of consensus is inconsistent.
    InconsistentAfterTxn,

    /// Unable to submit valid transaction
    TransactionError {
        /// source of error
        source: TransactionError,
    },
    /// Too many consecutive failures
    TooManyConsecutiveFailures,
    /// too many view failures overall
    TooManyViewFailures,
    /// inconsistent leaves
    InconsistentLeaves,
    InconsistentStates,
    InconsistentBlocks,
}

/// An overarching consensus test failure
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum ConsensusTestError {
    /// Too many nodes failed
    TooManyFailures,
    CompletedTestSuccessfully,
    ConsensusSafetyFailed {
        /// description of error
        description: String,
    },
}
