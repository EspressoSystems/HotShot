//! Commit phase

/// Leader implementation
mod leader;
/// Replica implementation
mod replica;

use super::{decide::DecidePhase, Progress, UpdateCtx};
use crate::{utils, ConsensusApi, Result, RoundTimedoutState};
use hotshot_types::{
    data::{QuorumCertificate, Stage},
    error::{FailedToMessageLeaderSnafu, StorageSnafu},
    message::{Commit, CommitVote, ConsensusMessage, PreCommit, PreCommitVote},
    traits::{node_implementation::NodeImplementation, storage::Storage},
};
use leader::CommitLeader;
use replica::CommitReplica;
use snafu::ResultExt;
use tracing::{debug, trace};

/// The commit phase
#[derive(Debug)]
pub(super) enum CommitPhase<const N: usize> {
    /// The leader
    Leader(CommitLeader<N>),
    /// The replica
    Replica(CommitReplica<N>),
}

impl<const N: usize> CommitPhase<N> {
    /// Create a new replica
    pub fn replica(starting_qc: QuorumCertificate<N>) -> Self {
        Self::Replica(CommitReplica::new(starting_qc))
    }

    /// Create a new leader
    pub fn leader(
        starting_qc: QuorumCertificate<N>,
        pre_commit: PreCommit<N>,
        vote: Option<PreCommitVote<N>>,
    ) -> Self {
        Self::Leader(CommitLeader::new(starting_qc, pre_commit, vote))
    }

    /// Update the current phase, returning the next phase if this phase is done.
    ///
    /// # Errors
    ///
    /// Will return an error if this phase is in an incorrect state or if the underlying [`ConsensusApi`] returns an error.
    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<DecidePhase<N>>> {
        let outcome: Option<Outcome<N>> = match (self, ctx.is_leader) {
            (Self::Leader(leader), true) => leader.update(ctx).await?,
            (Self::Replica(replica), false) => replica.update(ctx).await?,
            (this, _) => {
                return utils::err(format!(
                    "We're in {:?} but is_leader is {}",
                    this, ctx.is_leader
                ))
            }
        };
        debug!(?outcome);

        if let Some(outcome) = outcome {
            let decide = outcome.execute(ctx).await?;
            Ok(Progress::Next(decide))
        } else {
            Ok(Progress::NotReady)
        }
    }

    /// We're timing out, get the state of this phase
    pub fn timeout_reason(&self) -> RoundTimedoutState {
        match self {
            Self::Leader(_) => RoundTimedoutState::LeaderWaitingForPreCommitVotes,
            Self::Replica(_) => RoundTimedoutState::ReplicaWaitingForCommit,
        }
    }
}

/// The outcome of this [`CommitPhase`]
#[derive(Debug)]
struct Outcome<const N: usize> {
    /// The commit that was created on received
    commit: Commit<N>,
    /// Optionally the vote we're going to send
    vote: Option<CommitVote<N>>,
    /// The newest QC this round started with. This should be replaced by `prepare_qc` and `locked_qc` in the future.
    starting_qc: QuorumCertificate<N>,
}

impl<const N: usize> Outcome<N> {
    /// Execute the outcome. This will:
    /// - Store the QC
    /// - Send the commit if we are the leader
    /// - Send the vote if we have one
    ///
    /// # Errors
    ///
    /// Will return an error if:
    /// - The QC could not be stored
    /// - The commit could not be broadcasted
    /// - The vote could not be send
    async fn execute<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<DecidePhase<N>> {
        let Outcome {
            commit,
            vote,
            starting_qc,
        } = self;

        let was_leader = ctx.is_leader;
        let next_leader = ctx.api.get_leader(ctx.view_number).await;
        let is_next_leader = ctx.api.public_key() == &next_leader;

        // Importantly, a replica becomes locked on the precommitQC at this point by setting its locked QC to
        // precommitQC
        debug!(?commit.qc, "Inserting QC");
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
            ctx.send_broadcast_message(ConsensusMessage::Commit(commit.clone()))
                .await?;
        }

        if is_next_leader {
            Ok(DecidePhase::leader(starting_qc.clone(), commit, vote))
        } else {
            if let Some(vote) = vote {
                ctx.api
                    .send_direct_message(next_leader, ConsensusMessage::CommitVote(vote))
                    .await
                    .context(FailedToMessageLeaderSnafu {
                        stage: Stage::Commit,
                    })?;
            }
            Ok(DecidePhase::replica(starting_qc.clone()))
        }
    }
}
