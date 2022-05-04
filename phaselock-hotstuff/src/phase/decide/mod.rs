//! Decide implementation

/// Leader implementation
mod leader;
/// Replica implementation
mod replica;

use super::{Progress, UpdateCtx};
use crate::{utils, ConsensusApi, Result};
use leader::DecideLeader;
use phaselock_types::{
    data::Stage,
    error::FailedToMessageLeaderSnafu,
    message::{Commit, CommitVote, ConsensusMessage, Decide, NewView},
    traits::node_implementation::NodeImplementation,
};
use replica::DecideReplica;
use snafu::ResultExt;
use tracing::debug;

/// The decide phase
#[derive(Debug)]
pub(super) enum DecidePhase<const N: usize> {
    /// Leader phase
    Leader(DecideLeader<N>),
    /// Replica phase
    Replica(DecideReplica),
}

impl<const N: usize> DecidePhase<N> {
    /// Create a new replica
    pub fn replica() -> Self {
        Self::Replica(DecideReplica::new())
    }

    /// Create a new leader
    pub fn leader(commit: Commit<N>, vote: Option<CommitVote<N>>) -> Self {
        Self::Leader(DecideLeader::new(commit, vote))
    }

    /// Update the decide phase.
    ///
    /// # Errors
    ///
    /// Will return any errors that `leader.update`, `replica.update` and `outcome.execute` can return.
    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<()>> {
        let outcome: Option<Outcome<I, N>> = match (&mut *self, ctx.is_leader) {
            (Self::Leader(leader), true) => leader.update(ctx).await?,
            (Self::Replica(replica), false) => replica.update(ctx).await?,
            (this, _) => {
                return utils::err(format!(
                    "We're in {:?} but is_leader is {}",
                    this, ctx.is_leader
                ))
            }
        };
        debug!(?outcome, ?self);

        if let Some(outcome) = outcome {
            outcome.execute(ctx).await?;
            Ok(Progress::Next(()))
        } else {
            Ok(Progress::NotReady)
        }
    }
}

/// The outcome of this [`DecidePhase`]
#[derive(Debug)]
struct Outcome<I: NodeImplementation<N>, const N: usize> {
    /// The blocks that were decided on
    blocks: Vec<I::Block>,
    /// The states that were decided on
    states: Vec<I::State>,
    /// The decide message that was created or received
    decide: Decide<N>,
}

impl<I: NodeImplementation<N>, const N: usize> Outcome<I, N> {
    /// Execute the outcome of this [`DecidePhase`]
    ///
    /// This will notify any listeners of the blocks and states that were decided on. If this node was a leader this round, it will broadcast the [`Decide`] message.
    ///
    /// If a new round should be started, this will also send [`NewView`] to the leader of the `prepare` stage of the next round.
    ///
    /// # Errors
    ///
    /// Will return an error if:
    /// - The leader could not broadcast the [`Decide`]
    /// - a [`NewView`] could not be send
    async fn execute<A: ConsensusApi<I, N>>(self, ctx: &mut UpdateCtx<'_, I, A, N>) -> Result {
        let Outcome {
            blocks,
            states,
            decide,
        } = self;

        ctx.api.notify(blocks.clone(), states.clone()).await;
        ctx.api.send_decide(ctx.view_number.0, blocks, states).await;
        if ctx.is_leader {
            ctx.send_broadcast_message(ConsensusMessage::Decide(decide.clone()))
                .await?;
        }

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
