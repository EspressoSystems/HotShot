use crate::{
    phase::{err, precommit::PreCommitPhase, Phase, Progress, UpdateCtx},
    utils, ConsensusApi, Result, TransactionLink, TransactionState,
};
use phaselock_types::{
    data::{Leaf, LeafHash, QuorumCertificate, Stage},
    error::{FailedToBroadcastSnafu, FailedToMessageLeaderSnafu, PhaseLockError, StorageSnafu},
    message::{ConsensusMessage, Prepare, Vote},
    traits::{node_implementation::NodeImplementation, storage::Storage, BlockContents, State},
};
use snafu::ResultExt;
use std::time::Instant;
use tracing::{debug, error, trace, warn};

#[derive(Debug)]
pub(crate) struct PrepareReplica {}

impl PrepareReplica {
    pub(super) fn new() -> Self {
        Self {}
    }
    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<PreCommitPhase<I, N>>> {
        match ctx.prepare_message() {
            Some(msg) => self.vote(ctx, msg).await.map(Progress::Next),
            None => Ok(Progress::NotReady),
        }
    }

    async fn vote<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
        &mut self,
        ctx: &UpdateCtx<'_, I, A, N>,
        prepare: &Prepare<I::Block, I::State, N>,
    ) -> Result<PreCommitPhase<I, N>> {
        let leaf = prepare.leaf.clone();
        let leaf_hash = leaf.hash();
        let high_qc = prepare.high_qc.clone();
        let suggested_state = prepare.state.clone();

        let state = ctx.get_state_by_leaf(&leaf.parent).await?;
        let self_highest_qc = match ctx.get_newest_qc().await? {
            Some(qc) => qc,
            None => return err("No QC in storage"),
        };
        let is_safe_node = utils::safe_node(ctx.api, &self_highest_qc, &leaf, &high_qc).await;
        if !is_safe_node || !state.validate_block(&leaf.item) {
            error!("is_safe_node: {}", is_safe_node);
            error!(?leaf, "Leaf failed safe_node predicate");
            return Err(PhaseLockError::BadBlock {
                stage: Stage::Prepare,
            });
        }

        let current_view = ctx.view_number.0;

        let vote = || {
            let signature =
                ctx.api
                    .private_key()
                    .partial_sign(&leaf_hash, Stage::Prepare, current_view);
            Vote {
                signature,
                id: ctx.api.public_key().nonce,
                leaf_hash,
                current_view,
                stage: Stage::Prepare,
            }
        };

        let leader_next_round = ctx.api.get_leader(current_view, Stage::PreCommit).await;
        let is_leader_next_round = &leader_next_round == ctx.api.public_key();

        // Only send the vote if we're not a leader next round. Else we'll use it when instantiating PreCommit
        let next_phase = if !is_leader_next_round {
            let vote_message = ConsensusMessage::PrepareVote(vote());
            let network_result = ctx
                .api
                .send_direct_message(leader_next_round, vote_message)
                .await
                .context(FailedToMessageLeaderSnafu {
                    stage: Stage::Prepare,
                });
            if let Err(e) = network_result {
                warn!(?e, "Error submitting prepare vote");
            } else {
                debug!("Prepare message successfully processed");
            }
            PreCommitPhase::replica(None)
        } else if ctx.api.leader_acts_as_replica() {
            PreCommitPhase::leader(None, Some(vote()))
        } else {
            PreCommitPhase::leader(None, None)
        };

        ctx.api.send_propose(current_view, &leaf.item).await;
        // Add resulting state to storage
        let new_state = state.append(&leaf.item).map_err(|error| {
            error!(?error, "Failed to append block to existing state");
            PhaseLockError::InconsistentBlock {
                stage: Stage::Prepare,
            }
        })?;
        // Insert new state into storage
        debug!(?new_state, "New state inserted");
        if suggested_state != new_state {
            // the state that the leader send does not match with what we calculated
            error!(
                ?new_state,
                ?suggested_state,
                "Suggested state does not match actual state."
            );
            return Err(PhaseLockError::InconsistentBlock {
                stage: Stage::Prepare,
            });
        }
        ctx.api
            .storage()
            .update(|mut m| {
                let leaf = leaf.clone();
                async move {
                    m.insert_state(new_state, leaf_hash).await?;
                    m.insert_leaf(leaf).await?;
                    Ok(())
                }
            })
            .await
            .context(StorageSnafu)?;

        Ok(next_phase)
    }
}
