//! Decide implementation

/// Leader implementation
mod leader;
/// Replica implementation
mod replica;

use super::{Progress, UpdateCtx};
use crate::{utils, ConsensusApi, Result, RoundTimedoutState};
use hotshot_types::{
    data::{QuorumCertificate, Stage},
    error::FailedToMessageLeaderSnafu,
    message::{Commit, CommitVote, ConsensusMessage, Decide, NewView},
    traits::{node_implementation::NodeImplementation, storage::Storage, BlockContents},
};
use leader::DecideLeader;
use replica::DecideReplica;
use snafu::ResultExt;
use tracing::{debug, warn};

/// The decide phase
#[derive(Debug)]
pub(super) enum DecidePhase<const N: usize> {
    /// Leader phase
    Leader(DecideLeader<N>),
    /// Replica phase
    Replica(DecideReplica<N>),
}

impl<const N: usize> DecidePhase<N> {
    /// Create a new replica
    pub fn replica(starting_qc: QuorumCertificate<N>) -> Self {
        Self::Replica(DecideReplica::new(starting_qc))
    }

    /// Create a new leader
    pub fn leader(
        starting_qc: QuorumCertificate<N>,
        commit: Commit<N>,
        vote: Option<CommitVote<N>>,
    ) -> Self {
        Self::Leader(DecideLeader::new(starting_qc, commit, vote))
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

    /// We're timing out, get the state of this phase
    pub fn timeout_reason(&self) -> RoundTimedoutState {
        match self {
            Self::Leader(_) => RoundTimedoutState::LeaderWaitingForCommitVotes,
            Self::Replica(_) => RoundTimedoutState::ReplicaWaitingForDecide,
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

        // Get the quorum certificates
        let mut qcs = vec![];
        let storage = ctx.api.storage();
        for block in &blocks {
            match storage.get_qc(&block.hash()).await {
                Ok(Some(x)) => qcs.push(x.to_vec_cert()),
                Ok(None) => warn!(
                    ?block,
                    "Could not find QC in store for block when sending decide event to listener"
                ),
                Err(e) => warn!(
                    ?e,
                    ?block,
                    "Error finding QC in store for block when sending decide event to listener"
                ),
            }
        }
        ctx.api.notify(blocks.clone(), states.clone()).await;
        ctx.api
            .send_decide(ctx.view_number, blocks, states, qcs)
            .await;
        if ctx.is_leader {
            ctx.send_broadcast_message(ConsensusMessage::Decide(decide.clone()))
                .await?;
        }

        if ctx.api.should_start_round(ctx.view_number + 1).await {
            let next_leader = ctx.api.get_leader(ctx.view_number, Stage::Prepare).await;
            ctx.api
                .send_direct_message(
                    next_leader,
                    ConsensusMessage::NewView(NewView {
                        current_view: ctx.view_number + 1,
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
