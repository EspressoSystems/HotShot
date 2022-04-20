mod leader;
mod replica;

use super::{decide::DecidePhase, err, Progress, UpdateCtx};
use crate::{ConsensusApi, Result};
use leader::CommitLeader;
use phaselock_types::{
    data::Stage,
    error::{FailedToBroadcastSnafu, FailedToMessageLeaderSnafu, StorageSnafu},
    message::{Commit, CommitVote, ConsensusMessage, PreCommit, PreCommitVote},
    traits::{node_implementation::NodeImplementation, storage::Storage},
};
use replica::CommitReplica;
use snafu::ResultExt;
use tracing::trace;

#[derive(Debug)]
pub(super) enum CommitPhase<const N: usize> {
    Leader(CommitLeader<N>),
    Replica(CommitReplica),
}

impl<const N: usize> CommitPhase<N> {
    pub fn replica() -> Self {
        Self::Replica(CommitReplica::new())
    }

    pub fn leader(pre_commit: PreCommit<N>, vote: Option<PreCommitVote<N>>) -> Self {
        Self::Leader(CommitLeader::new(pre_commit, vote))
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<DecidePhase<N>>> {
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
            let decide = outcome.execute(ctx).await?;
            Ok(Progress::Next(decide))
        } else {
            Ok(Progress::NotReady)
        }
    }
}

struct Outcome<const N: usize> {
    commit: Commit<N>,
    vote: Option<CommitVote<N>>,
}

impl<const N: usize> Outcome<N> {
    async fn execute<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<DecidePhase<N>> {
        let Outcome { commit, vote } = self;

        let was_leader = ctx.api.is_leader(ctx.view_number.0, Stage::Commit).await;
        let next_leader = ctx.api.get_leader(ctx.view_number.0, Stage::Decide).await;
        let is_next_leader = ctx.api.public_key() == &next_leader;

        // Importantly, a replica becomes locked on the precommitQC at this point by setting its locked QC to
        // precommitQC
        ctx.api
            .storage()
            .update(|mut m| {
                let qc = commit.qc.clone();
                async move { m.insert_qc(qc).await }
            })
            .await
            .context(StorageSnafu)?;
        trace!(?commit.qc, "New state written");

        if was_leader {
            ctx.api
                .send_broadcast_message(ConsensusMessage::Commit(commit.clone()))
                .await
                .context(FailedToBroadcastSnafu {
                    stage: Stage::Commit,
                })?;
        }

        if is_next_leader {
            Ok(DecidePhase::leader(commit, vote))
        } else {
            if let Some(vote) = vote {
                ctx.api
                    .send_direct_message(next_leader, ConsensusMessage::CommitVote(vote))
                    .await
                    .context(FailedToMessageLeaderSnafu {
                        stage: Stage::Commit,
                    })?;
            }
            Ok(DecidePhase::replica())
        }
    }
}
