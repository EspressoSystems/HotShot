mod leader;
mod replica;

use super::{decide::DecidePhase, err, Progress, UpdateCtx};
use crate::{ConsensusApi, Result};
use leader::CommitLeader;
use phaselock_types::{
    message::{Commit, PreCommitVote},
    traits::node_implementation::NodeImplementation,
};
use replica::CommitReplica;

#[derive(Debug)]
pub(super) enum CommitPhase<const N: usize> {
    Leader(CommitLeader<N>),
    Replica(CommitReplica<N>),
}

impl<const N: usize> CommitPhase<N> {
    pub fn replica(commit: Option<Commit<N>>) -> Self {
        Self::Replica(CommitReplica::new(commit))
    }

    pub fn leader(commit: Option<Commit<N>>, vote: Option<PreCommitVote<N>>) -> Self {
        Self::Leader(CommitLeader::new(commit, vote))
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<DecidePhase<N>>> {
        match (self, ctx.is_leader) {
            (Self::Leader(leader), true) => leader.update(ctx).await,
            (Self::Replica(replica), false) => replica.update(ctx).await,
            (this, _) => err(format!(
                "We're in {:?} but is_leader is {}",
                this, ctx.is_leader
            )),
        }
    }
}
