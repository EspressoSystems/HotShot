mod leader;
mod replica;

use super::{commit::CommitPhase, err, Progress, UpdateCtx};
use crate::{ConsensusApi, Result};
use leader::PreCommitLeader;
use phaselock_types::{
    message::{Prepare, Vote},
    traits::node_implementation::NodeImplementation,
};
use replica::PreCommitReplica;

#[derive(Debug)]
pub(super) enum PreCommitPhase<I: NodeImplementation<N>, const N: usize> {
    Leader(PreCommitLeader<I, N>),
    Replica(PreCommitReplica<I, N>),
}

impl<I: NodeImplementation<N>, const N: usize> PreCommitPhase<I, N> {
    pub fn replica(prepare: Option<Prepare<I::Block, I::State, N>>) -> Self {
        Self::Replica(PreCommitReplica::new(prepare))
    }

    pub fn leader(prepare: Option<Prepare<I::Block, I::State, N>>, vote: Option<Vote<N>>) -> Self {
        Self::Leader(PreCommitLeader::new(prepare, vote))
    }

    pub(super) async fn update<A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<CommitPhase<N>>> {
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
