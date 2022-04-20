#![allow(dead_code, unused_imports, unused_variables)]

mod leader;
mod replica;

use super::{err, precommit::PreCommitPhase, Phase, Progress, UpdateCtx};
use crate::{ConsensusApi, Result, TransactionLink, TransactionState};
use leader::PrepareLeader;
use phaselock_types::{
    data::{Leaf, QuorumCertificate, Stage},
    error::{FailedToBroadcastSnafu, PhaseLockError, StorageSnafu},
    message::{ConsensusMessage, Prepare, PrepareVote, Vote},
    traits::{node_implementation::NodeImplementation, storage::Storage, BlockContents, State},
};
use replica::PrepareReplica;
use snafu::ResultExt;
use std::time::Instant;
use tracing::{debug, error, trace, warn};

#[derive(Debug)]
pub(crate) enum PreparePhase<const N: usize> {
    Leader(PrepareLeader<N>),
    Replica(PrepareReplica),
}

impl<const N: usize> PreparePhase<N> {
    pub(super) fn new(is_leader: bool) -> Self {
        if is_leader {
            Self::Leader(PrepareLeader::new())
        } else {
            Self::Replica(PrepareReplica::new())
        }
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<PreCommitPhase<I, N>>> {
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
            let pre_commit = outcome.execute(ctx).await?;
            Ok(Progress::Next(pre_commit))
        } else {
            Ok(Progress::NotReady)
        }
    }
}

struct Outcome<I: NodeImplementation<N>, const N: usize> {
    transactions: Vec<TransactionState<I, N>>,
    new_leaf: Leaf<I::Block, N>,
    new_state: I::State,
    prepare: Prepare<I::Block, I::State, N>,
    vote: Option<PrepareVote<N>>,
}

impl<I: NodeImplementation<N>, const N: usize> Outcome<I, N> {
    async fn execute<A: ConsensusApi<I, N>>(
        self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<PreCommitPhase<I, N>> {
        let Outcome {
            transactions,
            new_leaf,
            new_state,
            prepare,
            vote,
        } = self;

        let was_leader = ctx.api.is_leader(ctx.view_number.0, Stage::Prepare).await;
        let next_leader = ctx
            .api
            .get_leader(ctx.view_number.0, Stage::PreCommit)
            .await;
        let is_next_leader = ctx.api.public_key() == &next_leader;

        for transaction in transactions {
            *transaction.propose.write().await = Some(TransactionLink {
                timestamp: Instant::now(),
                view_number: ctx.view_number,
            });
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
            ctx.api
                .send_broadcast_message(ConsensusMessage::Prepare(prepare.clone()))
                .await
                .context(FailedToBroadcastSnafu {
                    stage: Stage::Prepare,
                })?;
        }
        // Notify our listeners
        ctx.api.send_propose(ctx.view_number.0, new_leaf.item).await;

        let next_phase = if is_next_leader {
            PreCommitPhase::leader(prepare, vote)
        } else {
            PreCommitPhase::replica()
        };
        Ok(next_phase)
    }
}
