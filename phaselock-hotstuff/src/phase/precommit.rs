mod leader;
mod replica;

use leader::PreCommitLeader;
use replica::PreCommitReplica;

#[allow(dead_code)]
pub(super) enum PreCommitPhase {
    Leader(PreCommitLeader),
    Replica(PreCommitReplica),
}

impl PreCommitPhase {
    pub fn replica() -> Self {
        Self::Replica(PreCommitReplica::new())
    }
}
