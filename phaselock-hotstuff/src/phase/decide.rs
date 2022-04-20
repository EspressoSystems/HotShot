mod leader;
mod replica;

use super::{err, Progress, UpdateCtx};
use crate::{ConsensusApi, Result};
use leader::DecideLeader;
use phaselock_types::{
    data::Stage,
    error::FailedToMessageLeaderSnafu,
    message::{Commit, CommitVote, ConsensusMessage, Decide, NewView},
    traits::node_implementation::NodeImplementation,
};
use replica::DecideReplica;
use snafu::ResultExt;

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
        let outcome: Option<Outcome<I, N>> = match (self, ctx.is_leader) {
            (Self::Leader(leader), true) => leader.update(ctx).await?,
            (Self::Replica(replica), false) => replica.update(ctx).await?,
            (this, _) => {
                return err(format!(
                    "We're in {:?} but is_leader is {}",
                    this, ctx.is_leader
                ))
            }
        };

        if let Some(outcome) = outcome {
            outcome.execute(ctx).await?;
            Ok(Progress::Next(()))
        } else {
            Ok(Progress::NotReady)
        }
    }
}

struct Outcome<I: NodeImplementation<N>, const N: usize> {
    blocks: Vec<I::Block>,
    states: Vec<I::State>,
    decide: Decide<N>,
}

impl<I: NodeImplementation<N>, const N: usize> Outcome<I, N> {
    async fn execute<A: ConsensusApi<I, N>>(self, ctx: &mut UpdateCtx<'_, I, A, N>) -> Result {
        let Outcome {
            blocks,
            states,
            decide,
        } = self;

        // TODO(vko): We currently do everything though the storage API, validate that we can indeed drop the `locked_qc` etc
        // // Update locked qc
        // let mut locked_qc = pl.inner.locked_qc.write().await;
        // *locked_qc = Some(pc_qc);
        // trace!("Locked qc updated");
        // Send decide event

        ctx.api.notify(blocks.clone(), states.clone()).await;
        ctx.api.send_decide(ctx.view_number.0, blocks, states).await;

        if ctx.api.should_start_round(ctx.view_number.0 + 1).await {
            let next_leader = ctx.api.get_leader(ctx.view_number.0, Stage::Prepare).await;
            ctx.api
                .send_direct_message(
                    next_leader,
                    ConsensusMessage::NewView(NewView {
                        current_view: ctx.view_number.0 + 1,
                        justify: decide.qc,
                    }),
                )
                .await
                .context(FailedToMessageLeaderSnafu {
                    stage: Stage::Decide,
                })?;
        }

        Ok(())
    }
}
