use crate::error::PhaseLockError;
use crate::traits::block_contents::BlockContents;
use crate::PhaseLock;

/// Holder for the state
pub struct BasicStateMachine<'a, B: BlockContents<N> + 'static, const N: usize> {
    phaselock: &'a mut PhaseLock<B, N>,
}

/// Starting state for the machie
struct Start {}

/// Indicates that the node was selected as a leader
struct Leader {}

/// Indicates that the leader is waiting for enough new-views to start the round
struct LeaderWaitingNewViews {}

/// Indicates that the leader is waiting for enough prepare votes to create a prepare qc
struct LeaderWaitingPrepare {}

/// Indicates that the leader is waiting for enough pre-commit votes to create a precommit qc
struct LeaderWaitingPreCommit {}

/// Indicates that the leader is waiting for enough commit votes to create a commit qc
struct LeaderWaitingCommit {}

/// Enum represnting current machine state
enum BasicState {
    Start,
    Leader,
    LeaderWaitingNewViews(LeaderWaitingNewViews),
    LeaderWaitingPrepare(LeaderWaitingPrepare),
    LeaderWaitingPreCommit(LeaderWaitingPreCommit),
    LeaderWaitingCommit(LeaderWaitingCommit),
    Ready,
    Error(PhaseLockError),
}

impl From<LeaderWaitingNewViews> for BasicState {
    fn from(x: LeaderWaitingNewViews) -> Self {}
}
