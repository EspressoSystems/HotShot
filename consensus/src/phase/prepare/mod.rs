//! Prepare implementation

mod leader;
mod replica;

use super::{precommit::PreCommitPhase, Progress, UpdateCtx};
use crate::{utils, ConsensusApi, Result, RoundTimedoutState, TransactionLink, TransactionState};
use hotshot_types::{
    data::{Leaf, QuorumCertificate, Stage},
    error::{FailedToMessageLeaderSnafu, StorageSnafu},
    message::{ConsensusMessage, Prepare, PrepareVote},
    traits::{node_implementation::NodeImplementation, storage::Storage},
};
use leader::PrepareLeader;
use replica::PrepareReplica;
use snafu::ResultExt;
use std::time::Instant;
use tracing::debug;

/// The prepare phase
#[derive(Debug)]
pub(crate) enum PreparePhase<const N: usize> {
    /// Leader phase
    Leader(PrepareLeader<N>),
    /// Replica phase
    Replica(PrepareReplica),
}

impl<const N: usize> PreparePhase<N> {
    /// Create a new [`PreparePhase`]
    pub(super) fn new(is_leader: bool) -> Self {
        if is_leader {
            Self::Leader(PrepareLeader::new())
        } else {
            Self::Replica(PrepareReplica::new())
        }
    }

    /// Update the current phase, returning the next phase if this phase is done.
    ///
    /// # Errors
    ///
    /// Will return an error if this phase is in an incorrect state or if the underlying [`ConsensusApi`] returns an error.
    pub(super) async fn update<'a, I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
        transactions: &'a mut [TransactionState<I, N>],
    ) -> Result<Progress<PreCommitPhase<I, N>>> {
        let outcome: Option<Outcome<'a, I, N>> = match (self, ctx.is_leader) {
            (Self::Leader(leader), true) => leader.update(ctx, transactions).await?,
            (Self::Replica(replica), false) => replica.update(ctx, transactions).await?,
            (this, _) => {
                return utils::err(format!(
                    "We're in {:?} but is_leader is {}",
                    this, ctx.is_leader
                ))
            }
        };
        debug!(?outcome);
        if let Some(outcome) = outcome {
            let pre_commit = outcome.execute(ctx).await?;
            Ok(Progress::Next(pre_commit))
        } else {
            Ok(Progress::NotReady)
        }
    }

    /// We're timing out, get the state of this phase
    pub fn timeout_reason(&self) -> RoundTimedoutState {
        match self {
            Self::Leader(leader) => {
                if leader.high_qc.is_none() {
                    RoundTimedoutState::LeaderWaitingForHighQC
                } else {
                    RoundTimedoutState::LeaderMinRoundTimeNotReached
                }
            }
            Self::Replica(_) => RoundTimedoutState::ReplicaWaitingForPrepare,
        }
    }
}

/// The outcome of the current [`PreparePhase`]
#[derive(Debug)]
struct Outcome<'a, I: NodeImplementation<N>, const N: usize> {
    /// A list of added transactions.
    added_transactions: Vec<&'a mut TransactionState<I, N>>,
    /// A list of rejected transactions
    rejected_transactions: Vec<&'a mut TransactionState<I, N>>,
    /// The new leaf
    new_leaf: Leaf<I::Block, N>,
    /// The new state
    new_state: I::State,
    /// The prepare block that we proposed or voted on
    prepare: Prepare<I::Block, I::State, N>,
    /// The vote that was cast this round. This is only `None` if we were a leader and `leader_acts_as_replica` is `false`.
    vote: Option<PrepareVote<N>>,
    /// The current newest QC. This should be replaced by `prepare_qc` and `locked_qc` in the future.
    newest_qc: QuorumCertificate<N>,
}

impl<'a, I: NodeImplementation<N>, const N: usize> Outcome<'a, I, N> {
    /// execute the given outcome, returning the next phase
    ///
    /// # Errors
    ///
    /// will return an error if the underlying `Storage` or `NetworkImplementation` returns an error.
    async fn execute<A: ConsensusApi<I, N>>(
        self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<PreCommitPhase<I, N>> {
        let Outcome {
            added_transactions,
            rejected_transactions,
            new_leaf,
            new_state,
            prepare,
            vote,
            newest_qc,
        } = self;

        let was_leader = ctx.is_leader;
        let next_leader = ctx.api.get_leader(ctx.view_number, Stage::PreCommit).await;
        let is_next_leader = ctx.api.public_key() == &next_leader;

        for transaction in added_transactions {
            transaction.propose = Some(TransactionLink {
                timestamp: Instant::now(),
                view_number: ctx.view_number,
            });
        }
        for transaction in rejected_transactions {
            transaction.rejected = Some(Instant::now());
        }
        let leaf_hash = new_leaf.hash();
        ctx.api
            .storage()
            .update(|mut m| {
                let new_leaf = new_leaf.clone();
                let leaf_hash = leaf_hash;
                let new_state = new_state.clone();
                async move {
                    m.insert_leaf(new_leaf).await?;
                    m.insert_state(new_state, leaf_hash).await?;
                    Ok(())
                }
            })
            .await
            .context(StorageSnafu)?;
        debug!(?new_leaf, ?leaf_hash, "Leaf created and added to store");
        debug!(?new_state, "New state inserted");

        if was_leader {
            // Broadcast out the leaf
            ctx.send_broadcast_message(ConsensusMessage::Prepare(prepare.clone()))
                .await?;
        }

        // Notify our listeners
        ctx.api.send_propose(ctx.view_number, new_leaf.item).await;

        let next_phase = if is_next_leader {
            PreCommitPhase::leader(newest_qc, prepare, vote)
        } else {
            if let Some(vote) = vote {
                ctx.api
                    .send_direct_message(next_leader, ConsensusMessage::PrepareVote(vote))
                    .await
                    .context(FailedToMessageLeaderSnafu {
                        stage: Stage::Prepare,
                    })?;
            }
            PreCommitPhase::replica(newest_qc)
        };
        Ok(next_phase)
    }
}
