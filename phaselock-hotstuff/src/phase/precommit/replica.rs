use super::Outcome;
use crate::{phase::UpdateCtx, utils, ConsensusApi, Result};
use phaselock_types::{
    data::{QuorumCertificate, Stage},
    error::PhaseLockError,
    message::{PreCommit, PreCommitVote, Vote},
    traits::node_implementation::NodeImplementation,
};
use tracing::error;

/// A precommit replica
#[derive(Debug)]
pub struct PreCommitReplica<const N: usize> {
    /// The QC that this round started with
    starting_qc: QuorumCertificate<N>,
}

impl<const N: usize> PreCommitReplica<N> {
    /// Create a new replica
    pub fn new(starting_qc: QuorumCertificate<N>) -> Self {
        Self { starting_qc }
    }

    /// Update this replica, returning an `Outcome` when it's done.
    ///
    /// This will:
    /// - Wait for an incoming [`PreCommit`] message.
    /// - Validate this message
    /// - Cast a vote on this message.
    ///
    /// # Errors
    ///
    /// This will return an error if:
    /// - There is no QC in storage.
    /// - The proposed QC is invalid.
    /// - The underlying [`ConsensusApi`] returned an error.
    #[tracing::instrument]
    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
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

    /// Validate the given [`PreCommit`] and cast a vote.
    ///
    /// # Errors
    ///
    /// The possible errors are documented in the `update` method.
    async fn vote<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &UpdateCtx<'_, I, A, N>,
        pre_commit: PreCommit<N>,
    ) -> Result<Outcome<N>> {
        // TODO: We can probably get these from `PreparePhase`
        let leaf = ctx.get_leaf(&pre_commit.leaf_hash).await?;

        let self_highest_qc = match ctx.get_newest_qc().await? {
            Some(qc) => qc,
            None => return utils::err("No QC in storage"),
        };

        let is_safe_node =
            utils::validate_against_locked_qc(ctx.api, &self_highest_qc, &leaf, &pre_commit.qc)
                .await;
        if !is_safe_node {
            error!("is_safe_node: {}", is_safe_node);
            error!(?leaf, "Leaf failed safe_node predicate");
            return Err(PhaseLockError::BadBlock {
                stage: Stage::Prepare,
            });
        }

        let leaf_hash = leaf.hash();
        let current_view = ctx.view_number;
        let signature = ctx
            .api
            .sign_vote(&leaf_hash, Stage::PreCommit, current_view);
        let vote = PreCommitVote(Vote {
            signature,
            leaf_hash,
            current_view,
        });
        Ok(Outcome {
            vote: Some(vote),
            pre_commit,
            starting_qc: self.starting_qc.clone(),
        })
    }
}
