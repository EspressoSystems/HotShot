use super::Outcome;
use crate::{
    phase::{err, UpdateCtx},
    utils, ConsensusApi, Result,
};
use phaselock_types::{
    data::Stage,
    error::{PhaseLockError, StorageSnafu},
    message::{Commit, CommitVote, Vote},
    traits::{node_implementation::NodeImplementation, storage::Storage},
};
use snafu::ResultExt;
use tracing::{error, info, trace};

#[derive(Debug)]
pub struct CommitReplica {}

impl CommitReplica {
    pub fn new() -> Self {
        Self {}
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
        &mut self,
        ctx: &UpdateCtx<'_, I, A, N>,
    ) -> Result<Option<Outcome<I, N>>> {
        let commit = if let Some(commit) = ctx.commit_message() {
            commit
        } else {
            return Ok(None);
        };
        let commit = commit.clone();
        let outcome = self.vote(ctx, commit).await?;
        Ok(Some(outcome))
    }

    async fn vote<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
        &mut self,
        ctx: &UpdateCtx<'_, I, A, N>,
        commit: Commit<N>,
    ) -> Result<Outcome<I, N>> {
        // this leaf hash should've been inserted in `PreCommitPhase`
        let leaf = match ctx
            .api
            .storage()
            .get_leaf(&commit.leaf_hash)
            .await
            .context(StorageSnafu)?
        {
            Some(leaf) => leaf,
            None => {
                // TODO(vko) try the next commit in `ctx` if any?
                return err(format!("Could not find leaf {:?}", commit.leaf_hash));
            }
        };
        let leaf_hash = leaf.hash();
        // Verify QC
        if !(commit.qc.verify(
            &ctx.api.public_key().set,
            ctx.view_number.0,
            Stage::PreCommit,
        ) && commit.leaf_hash == leaf_hash)
        {
            error!(?commit.qc, "Bad or forged precommit qc");
            return Err(PhaseLockError::BadOrForgedQC {
                stage: Stage::Commit,
                bad_qc: commit.qc.to_vec_cert(),
            });
        }
        let old_qc = match ctx.get_newest_qc().await? {
            Some(qc) => qc,
            None => {
                return err("No QC in storage");
            }
        };
        // Find blocks and states that were commited
        let walk_leaf = commit.leaf_hash;
        let old_leaf_hash = old_qc.leaf_hash;

        let (blocks, states) = utils::walk_leaves(ctx.api, walk_leaf, old_leaf_hash).await?;
        info!(?blocks, ?states, "Sending decide events");
        // TODO(vko): We currently do everything though the storage API, validate that we can indeed drop the `locked_qc` etc
        // // Update locked qc
        // let mut locked_qc = pl.inner.locked_qc.write().await;
        // *locked_qc = Some(pc_qc);
        // trace!("Locked qc updated");
        let signature =
            ctx.api
                .private_key()
                .partial_sign(&leaf_hash, Stage::Commit, ctx.view_number.0);
        let vote = CommitVote(Vote {
            leaf_hash,
            signature,
            id: ctx.api.public_key().nonce,
            current_view: ctx.view_number.0,
        });
        trace!("Commit vote packed");

        Ok(Outcome {
            blocks,
            commit,
            states,
            vote: Some(vote),
        })
    }
}
