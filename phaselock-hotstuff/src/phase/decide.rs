mod leader;
mod replica;

use super::{err, Progress, UpdateCtx};
use crate::{ConsensusApi, Result};
use leader::DecideLeader;
use phaselock_types::{
    message::{Commit, CommitVote},
    traits::node_implementation::NodeImplementation,
};
use replica::DecideReplica;

#[derive(Debug)]
pub(super) enum DecidePhase<const N: usize> {
    Leader(DecideLeader<N>),
    Replica(DecideReplica),
}

impl<const N: usize> DecidePhase<N> {
    pub fn replica() -> Self {
        Self::Replica(DecideReplica::new())
    }

    pub fn leader(commit: Commit<N>, vote: Option<CommitVote<N>>) -> Self {
        Self::Leader(DecideLeader::new(commit, vote))
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<()>> {
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
