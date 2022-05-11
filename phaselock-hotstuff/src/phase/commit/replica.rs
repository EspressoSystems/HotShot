use super::Outcome;
use crate::{phase::UpdateCtx, utils, ConsensusApi, Result};
use phaselock_types::{
    data::{QuorumCertificate, Stage},
    error::{PhaseLockError, StorageSnafu},
    message::{Commit, CommitVote, Vote},
    traits::{node_implementation::NodeImplementation, storage::Storage},
};
use snafu::ResultExt;
use tracing::{error, trace};

/// The replica
#[derive(Debug)]
pub struct CommitReplica<const N: usize> {
    /// The QC that this round started with
    starting_qc: QuorumCertificate<N>,
}

impl<const N: usize> CommitReplica<N> {
    /// Create a new replica
    pub fn new(starting_qc: QuorumCertificate<N>) -> Self {
        Self { starting_qc }
    }

    /// Update the replica. This will:
    /// - Wait for an incoming [`Commit`]
    /// - Get the leaf from the incoming commit
    /// - Verify the commit QC is valid
    /// - Create a new vote and sign it
    ///
    /// # Errors
    ///
    /// Will return an error if:
    /// - The leaf could not be loaded
    /// - The QC is invalid
    /// - A vote signature could not be made
    #[tracing::instrument]
    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &self,
        ctx: &UpdateCtx<'_, I, A, N>,
    ) -> Result<Option<Outcome<N>>> {
        let commit = if let Some(commit) = ctx.commit_message() {
            commit
        } else {
            return Ok(None);
        };
        let commit = commit.clone();
        let outcome = self.vote(ctx, commit).await?;
        Ok(Some(outcome))
    }

    /// Vote on the given [`Commit`]
    ///
    /// # Errors
    ///
    /// Errors are described in the documentation of [`Self::update`]
    async fn vote<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &self,
        ctx: &UpdateCtx<'_, I, A, N>,
        commit: Commit<N>,
    ) -> Result<Outcome<N>> {
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
                return utils::err(format!("Could not find leaf {:?}", commit.leaf_hash));
            }
        };
        let leaf_hash = leaf.hash();
        // Verify QC
        if !(commit.qc.verify(
            ctx.api.cluster_public_keys(),
            ctx.api.threshold().get(),
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

        let signature = ctx
            .api
            .sign_vote(&leaf_hash, Stage::Commit, ctx.view_number.0);
        let vote = CommitVote(Vote {
            leaf_hash,
            signature,
            id: ctx.api.public_key().nonce,
            current_view: ctx.view_number,
        });
        trace!("Commit vote packed");

        Ok(Outcome {
            commit,
            vote: Some(vote),
            starting_qc: self.starting_qc.clone(),
        })
    }
}
