//! Types for the View Sync operations

#[derive(PartialEq, PartialOrd, Clone, Debug, Eq, Hash)]
/// Phases of view sync
pub enum ViewSyncPhase {
    /// No phase; before the protocol has begun
    None,
    /// PreCommit phase
    PreCommit,
    /// Commit phase
    Commit,
    /// Finalize phase
    Finalize,
}
