mod leader;
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

#[derive(Debug)]
pub(super) enum PreCommitPhase<I: NodeImplementation<N>, const N: usize> {
    Leader(PreCommitLeader<I, N>),
    Replica(PreCommitReplica),
}

impl<I: NodeImplementation<N>, const N: usize> PreCommitPhase<I, N> {
    pub fn replica() -> Self {
        Self::Replica(PreCommitReplica::new())
    }

    pub fn leader(prepare: Prepare<I::Block, I::State, N>, vote: Option<PrepareVote<N>>) -> Self {
        Self::Leader(PreCommitLeader::new(prepare, vote))
    }

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

struct Outcome<const N: usize> {
    pre_commit: PreCommit<N>,
    vote: Option<PreCommitVote<N>>,
}
impl<const N: usize> Outcome<N> {
    async fn execute<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<CommitPhase<N>> {
        let Outcome { pre_commit, vote } = self;

        let was_leader = ctx.api.is_leader(ctx.view_number.0, Stage::PreCommit).await;
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
