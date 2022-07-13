//! Precommit implementation

/// Precommit leader implementation
mod leader;
/// Precommit replica implementation
mod replica;

use super::{commit::CommitPhase, Progress, UpdateCtx};
use crate::{utils, ConsensusApi, Result, RoundTimedoutState};
use hotshot_types::{
    data::{QuorumCertificate, Stage},
    error::FailedToMessageLeaderSnafu,
    message::{ConsensusMessage, PreCommit, PreCommitVote, Prepare, PrepareVote},
    traits::node_implementation::NodeImplementation,
};
use leader::PreCommitLeader;
use replica::PreCommitReplica;
use snafu::ResultExt;
use tracing::debug;

/// The pre commit phase.
#[derive(Debug)]
pub(super) enum PreCommitPhase<I: NodeImplementation<N>, const N: usize> {
    /// Leader phase
    Leader(PreCommitLeader<I, N>),
    /// Replica phase
    Replica(PreCommitReplica<N>),
}

impl<I: NodeImplementation<N>, const N: usize> PreCommitPhase<I, N> {
    /// Create a new replica
    pub fn replica(starting_qc: QuorumCertificate<N>) -> Self {
        Self::Replica(PreCommitReplica::new(starting_qc))
    }

    /// Create a new leader
    pub fn leader(
        starting_qc: QuorumCertificate<N>,
        prepare: Prepare<I::Block, I::State, N>,
        vote: Option<PrepareVote<N>>,
    ) -> Self {
        Self::Leader(PreCommitLeader::new(starting_qc, prepare, vote))
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
                return utils::err(format!(
                    "We're in {:?} but is_leader is {}",
                    this, ctx.is_leader
                ))
            }
        };
        debug!(?outcome);
        if let Some(outcome) = outcome {
            let commit = outcome.execute(ctx).await?;
            Ok(Progress::Next(commit))
        } else {
            Ok(Progress::NotReady)
        }
    }

    /// We're timing out, get the state of this phase
    pub fn timeout_reason(&self) -> RoundTimedoutState {
        match self {
            Self::Leader(_) => RoundTimedoutState::LeaderWaitingForPrepareVotes,
            Self::Replica(_) => RoundTimedoutState::ReplicaWaitingForPreCommit,
        }
    }
}

/// The outcome of this [`PreCommitPhase`]
#[derive(Debug)]
struct Outcome<const N: usize> {
    /// The pre commit that we created or voted on
    pre_commit: PreCommit<N>,
    /// The vote that we created
    vote: Option<PreCommitVote<N>>,
    /// The newest QC this round started with. This should be replaced by `prepare_qc` and `locked_qc` in the future.
    starting_qc: QuorumCertificate<N>,
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
        let Outcome {
            pre_commit,
            vote,
            starting_qc,
        } = self;

        let was_leader = ctx.is_leader;
        let next_leader = ctx.api.get_leader(ctx.view_number, Stage::Commit).await;
        let is_next_leader = ctx.api.public_key() == &next_leader;

        if was_leader {
            ctx.send_broadcast_message(ConsensusMessage::PreCommit(pre_commit.clone()))
                .await?;
        }
        if is_next_leader {
            Ok(CommitPhase::leader(starting_qc, pre_commit, vote))
        } else {
            if let Some(vote) = vote {
                ctx.api
                    .send_direct_message(next_leader, ConsensusMessage::PreCommitVote(vote))
                    .await
                    .context(FailedToMessageLeaderSnafu {
                        stage: Stage::PreCommit,
                    })?;
            }
            Ok(CommitPhase::replica(starting_qc))
        }
    }
}
