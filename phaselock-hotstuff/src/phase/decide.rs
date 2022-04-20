mod leader;
mod replica;

use super::{err, Progress, UpdateCtx};
use crate::{ConsensusApi, Result};
use leader::DecideLeader;
use phaselock_types::traits::node_implementation::NodeImplementation;
use replica::DecideReplica;

#[derive(Debug)]
pub(super) enum DecidePhase {
    Leader(DecideLeader),
    Replica(DecideReplica),
}

impl DecidePhase {
    pub fn replica(already_validated: bool) -> Self {
        Self::Replica(DecideReplica::new(already_validated))
    }

    pub fn leader(already_validated: bool) -> Self {
        Self::Leader(DecideLeader::new(already_validated))
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
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
