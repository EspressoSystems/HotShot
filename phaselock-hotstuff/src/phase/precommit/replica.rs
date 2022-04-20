use super::Outcome;
use crate::{
    phase::{err, UpdateCtx},
    utils, ConsensusApi, Result,
};
use phaselock_types::{
    data::Stage,
    error::PhaseLockError,
    message::{PreCommit, PreCommitVote, Vote},
    traits::{node_implementation::NodeImplementation, State},
};
use tracing::error;

#[derive(Debug)]
pub struct PreCommitReplica {}

impl PreCommitReplica {
    pub fn new() -> Self {
        Self {}
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
        &mut self,
        ctx: &UpdateCtx<'_, I, A, N>,
    ) -> Result<Option<Outcome<N>>> {
        if let Some(pre_commit) = ctx.pre_commit_message() {
            let outcome = self.vote(ctx, pre_commit.clone()).await?;
            Ok(Some(outcome))
        } else {
            Ok(None)
        }
    }

    async fn vote<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
        &mut self,
        ctx: &UpdateCtx<'_, I, A, N>,
        pre_commit: PreCommit<N>,
    ) -> Result<Outcome<N>> {
        // TODO: We can probably get these from `PreparePhase`
        let leaf = ctx.get_leaf(&pre_commit.leaf_hash).await?;
        let state = ctx.get_state_by_leaf(&pre_commit.leaf_hash).await?;
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

        let is_safe_node = utils::safe_node(ctx.api, &self_highest_qc, &leaf, &pre_commit.qc).await;
        if !is_safe_node {
            error!("is_safe_node: {}", is_safe_node);
            error!(?leaf, "Leaf failed safe_node predicate");
            return Err(PhaseLockError::BadBlock {
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
        Ok(Outcome {
            vote: Some(vote),
            pre_commit,
        })
    }
}
