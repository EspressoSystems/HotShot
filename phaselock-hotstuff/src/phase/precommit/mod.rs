//! Precommit implementation

/// Precommit leader implementation
mod leader;
/// Precommit replica implementation
mod replica;

use super::{commit::CommitPhase, err, Progress, UpdateCtx};
use crate::{ConsensusApi, Result};
use leader::PreCommitLeader;
use phaselock_types::{
    data::Stage,
    error::{FailedToBroadcastSnafu, FailedToMessageLeaderSnafu},
    message::{ConsensusMessage, PreCommit, PreCommitVote, Prepare, PrepareVote},
    traits::node_implementation::NodeImplementation,
};
use replica::PreCommitReplica;
use snafu::ResultExt;

/// The pre commit phase.
#[derive(Debug)]
pub(super) enum PreCommitPhase<I: NodeImplementation<N>, const N: usize> {
    /// Leader phase
    Leader(PreCommitLeader<I, N>),
    /// Replica phase
    Replica(PreCommitReplica),
}

impl<I: NodeImplementation<N>, const N: usize> PreCommitPhase<I, N> {
    /// Create a new replica
    pub fn replica() -> Self {
        Self::Replica(PreCommitReplica::new())
    }

    /// Create a new leader
    pub fn leader(prepare: Prepare<I::Block, I::State, N>, vote: Option<PrepareVote<N>>) -> Self {
        Self::Leader(PreCommitLeader::new(prepare, vote))
    }

    /// Update this precommit. This will either call `leader.update` or `replica.update`.
    ///
    /// # Errors
    ///
    /// Will return an error if this phase is in an incorrect state or if the underlying [`ConsensusApi`] returns an error.
    pub(super) async fn update<A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<CommitPhase<N>>> {
        let outcome: Option<Outcome<N>> = match (self, ctx.is_leader) {
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
            let commit = outcome.execute(ctx).await?;
            Ok(Progress::Next(commit))
        } else {
            Ok(Progress::NotReady)
        }
    }
}

/// The outcome of this [`PreCommitPhase`]
struct Outcome<const N: usize> {
    /// The pre commit that we created or voted on
    pre_commit: PreCommit<N>,
    /// The vote that we created
    vote: Option<PreCommitVote<N>>,
}

impl<const N: usize> Outcome<N> {
    /// Execute the given `Outcome`.
    ///
    /// # Errors
    ///
    /// Will propagate any errors that the underlying [`ConsensusApi`] encounters.
    async fn execute<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<CommitPhase<N>> {
        let Outcome { pre_commit, vote } = self;

        let was_leader = ctx.is_leader;
        let next_leader = ctx.api.get_leader(ctx.view_number.0, Stage::Commit).await;
        let is_next_leader = ctx.api.public_key() == &next_leader;

        if was_leader {
            ctx.api
                .send_broadcast_message(ConsensusMessage::PreCommit(pre_commit.clone()))
                .await
                .context(FailedToBroadcastSnafu {
                    stage: Stage::PreCommit,
                })?;
        }
        if is_next_leader {
            Ok(CommitPhase::leader(pre_commit, vote))
        } else {
            if let Some(vote) = vote {
                ctx.api
                    .send_direct_message(next_leader, ConsensusMessage::PreCommitVote(vote))
                    .await
                    .context(FailedToMessageLeaderSnafu {
                        stage: Stage::PreCommit,
                    })?;
            }
            Ok(CommitPhase::replica())
        }
    }
}
