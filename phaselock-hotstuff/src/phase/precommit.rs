mod leader;
mod replica;

use super::{err, Progress, UpdateCtx};
use crate::{ConsensusApi, Result};
use leader::PreCommitLeader;
use phaselock_types::{message::Vote, traits::node_implementation::NodeImplementation};
use replica::PreCommitReplica;

#[derive(Debug)]
pub(super) enum PreCommitPhase<const N: usize> {
    Leader(PreCommitLeader<N>),
    Replica(PreCommitReplica),
}

impl<const N: usize> PreCommitPhase<N> {
    pub fn replica() -> Self {
        Self::Replica(PreCommitReplica::new())
    }

    pub fn leader(vote: Option<Vote<N>>) -> Self {
        Self::Leader(PreCommitLeader::new(vote))
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
