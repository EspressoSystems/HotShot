use crate::{
    phase::{commit::CommitPhase, err, Progress, UpdateCtx},
    utils, ConsensusApi, Result,
};
use phaselock_types::{
    data::Stage,
    error::{FailedToMessageLeaderSnafu, PhaseLockError, StorageSnafu},
    message::{ConsensusMessage, PreCommitVote, Prepare, Vote},
    traits::{node_implementation::NodeImplementation, storage::Storage, State},
};
use snafu::ResultExt;
use tracing::{debug, error, warn};

#[derive(Debug)]
pub struct PreCommitReplica<I: NodeImplementation<N>, const N: usize> {
    prepare: Option<Prepare<I::Block, I::State, N>>,
}

impl<I: NodeImplementation<N>, const N: usize> PreCommitReplica<I, N> {
    pub fn new(prepare: Option<Prepare<I::Block, I::State, N>>) -> Self {
        Self { prepare }
    }

    pub(super) async fn update<A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<CommitPhase<N>>> {
        let prepare = if let Some(prepare) = &self.prepare {
            prepare
        } else if let Some(prepare) = ctx.prepare_message() {
            prepare
        } else {
            return Ok(Progress::NotReady);
        };

        let prepare = prepare.clone();
        Ok(Progress::Next(self.vote(ctx, prepare).await?))
    }

    async fn vote<A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
        prepare: Prepare<I::Block, I::State, N>,
    ) -> Result<CommitPhase<N>> {
        let leaf = prepare.leaf;
        let state = ctx.get_state_by_leaf(&leaf.parent).await?;
        let self_highest_qc = match ctx.get_newest_qc().await? {
            Some(qc) => qc,
            None => return err("No QC in storage"),
        };

        if !state.validate_block(&leaf.item) {
            error!(?leaf, "Leaf failed safe_node predicate");
            return Err(PhaseLockError::BadBlock {
                stage: Stage::Prepare,
            });
        }

        let is_safe_node =
            utils::safe_node(ctx.api, &self_highest_qc, &leaf, &prepare.high_qc).await;
        if !is_safe_node {
            error!("is_safe_node: {}", is_safe_node);
            error!(?leaf, "Leaf failed safe_node predicate");
            return Err(PhaseLockError::BadBlock {
                stage: Stage::Prepare,
            });
        }

        let new_state = state.append(&leaf.item).map_err(|error| {
            error!(?error, "Failed to append block to existing state");
            PhaseLockError::InconsistentBlock {
                stage: Stage::Prepare,
            }
        })?;
        if prepare.state != new_state {
            // the state that the leader send does not match with what we calculated
            error!(
                ?new_state,
                ?prepare.state,
                "Suggested state does not match actual state."
            );
            return Err(PhaseLockError::InconsistentBlock {
                stage: Stage::Prepare,
            });
        }

        let leaf_hash = leaf.hash();
        let current_view = ctx.view_number.0;
        let signature =
            ctx.api
                .private_key()
                .partial_sign(&leaf_hash, Stage::Prepare, current_view);
        let vote = PreCommitVote(Vote {
            signature,
            id: ctx.api.public_key().nonce,
            leaf_hash,
            current_view,
        });
        let vote_message = ConsensusMessage::PreCommitVote(vote.clone());
        let next_leader = ctx.api.get_leader(current_view, Stage::Commit).await;
        let is_leader_next = &next_leader == ctx.api.public_key();
        if !is_leader_next {
            let network_result = ctx
                .api
                .send_direct_message(next_leader, vote_message)
                .await
                .context(FailedToMessageLeaderSnafu {
                    stage: Stage::Prepare,
                });
            if let Err(e) = network_result {
                warn!(?e, "Error submitting prepare vote");
            } else {
                debug!("Prepare message successfully processed");
            }
        }
        ctx.api.send_propose(current_view, &leaf.item).await;
        // Insert new state into storage
        debug!(?new_state, "New state inserted");
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

        Ok(if ctx.api.is_leader(current_view, Stage::Commit).await {
            CommitPhase::leader(
                None,
                if ctx.api.leader_acts_as_replica() {
                    Some(vote)
                } else {
                    None
                },
            )
        } else {
            CommitPhase::replica(None)
        })
    }
}
