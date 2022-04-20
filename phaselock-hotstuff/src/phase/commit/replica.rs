use crate::{
    phase::{decide::DecidePhase, err, Progress, UpdateCtx},
    ConsensusApi, Result,
};
use phaselock_types::{
    data::Stage,
    error::{FailedToMessageLeaderSnafu, PhaseLockError, StorageSnafu},
    message::{Commit, CommitVote, ConsensusMessage, Vote},
    traits::{node_implementation::NodeImplementation, storage::Storage},
};
use snafu::ResultExt;
use tracing::{debug, error, trace, warn};

#[derive(Debug)]
pub struct CommitReplica<const N: usize> {
    commit: Option<Commit<N>>,
}

impl<const N: usize> CommitReplica<N> {
    pub fn new(commit: Option<Commit<N>>) -> Self {
        Self { commit }
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<DecidePhase<N>>> {
        let commit = if let Some(commit) = &self.commit {
            commit
        } else if let Some(commit) = ctx.commit_message() {
            commit
        } else {
            return Ok(Progress::NotReady);
        };
        let commit = commit.clone();
        let decide = self.vote(ctx, commit).await?;
        Ok(Progress::Next(decide))
    }

    async fn vote<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
        commit: Commit<N>,
    ) -> Result<DecidePhase<N>> {
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
            stage: Stage::Commit,
        });
        trace!("Commit vote packed");
        let leader = ctx.api.get_leader(ctx.view_number.0, Stage::Decide).await;

        if &leader == ctx.api.public_key() {
            Ok(DecidePhase::leader(Some(vote), false))
        } else {
            let network_result = ctx
                .api
                .send_direct_message(leader, ConsensusMessage::CommitVote(vote))
                .await
                .context(FailedToMessageLeaderSnafu {
                    stage: Stage::Commit,
                });
            if let Err(e) = network_result {
                warn!(?e, "Error sending commit vote");
            } else {
                debug!("Commit vote sent to leader");
            }
            Ok(DecidePhase::replica(false))
        }
    }
}
