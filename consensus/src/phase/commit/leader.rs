use super::Outcome;
use crate::{phase::UpdateCtx, ConsensusApi, Result};
use hotshot_types::{
    data::{QuorumCertificate, Stage},
    message::{Commit, CommitVote, PreCommit, PreCommitVote, Vote},
    traits::node_implementation::NodeImplementation,
};

/// The leader
#[derive(Debug)]
pub(crate) struct CommitLeader<const N: usize> {
    /// The precommit that was created or voted on last stage
    pre_commit: PreCommit<N>,
    /// Optionally the vote that we created last stage
    vote: Option<PreCommitVote<N>>,
    /// The QC that this round started with
    starting_qc: QuorumCertificate<N>,
}

impl<const N: usize> CommitLeader<N> {
    /// Create a new leader
    pub(super) fn new(
        starting_qc: QuorumCertificate<N>,
        pre_commit: PreCommit<N>,
        vote: Option<PreCommitVote<N>>,
    ) -> Self {
        Self {
            pre_commit,
            vote,
            starting_qc,
        }
    }

    /// Update this leader. This will:
    /// - Get a list of [`PreCommitVote`] targetting this [`PreCommit`]
    /// - If the threshold is reached:
    ///   - Combine the signatures
    ///   - Create a new QC and [`Commit`]
    ///   - Optionally vote on this commit
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The signatures could not be combined
    /// - The vote could not be signed
    #[tracing::instrument]
    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &self,
        ctx: &UpdateCtx<'_, I, A, N>,
    ) -> Result<Option<Outcome<N>>> {
        let valid_votes: Vec<PreCommitVote<N>> = ctx
            .pre_commit_vote_messages()
            // make sure to append our own vote if we have one
            .chain(self.vote.iter())
            .filter(|vote| vote.leaf_hash == self.pre_commit.leaf_hash)
            .cloned()
            .collect();

        if valid_votes.len() >= ctx.api.threshold().get() {
            let outcome: Outcome<N> = self
                .create_commit(ctx, &self.pre_commit, valid_votes)
                .await?;
            Ok(Some(outcome))
        } else {
            Ok(None)
        }
    }

    /// Create a new commit based on the given [`PreCommit`] and [`PreCommitVote`]s
    ///
    /// # Errors
    ///
    /// Errors are described in the documentation of `update`
    async fn create_commit<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &self,
        ctx: &UpdateCtx<'_, I, A, N>,
        pre_commit: &PreCommit<N>,
        votes: Vec<PreCommitVote<N>>,
    ) -> Result<Outcome<N>> {
        // Generate QC
        let leaf_hash = pre_commit.leaf_hash;
        let current_view = ctx.view_number;

        let verify_hash = ctx
            .api
            .create_verify_hash(&leaf_hash, Stage::PreCommit, current_view);

        let signatures = votes
            .into_iter()
            .map(|vote| (vote.0.signature.0.clone(), vote.0))
            .collect();
        let valid_signatures = ctx.api.get_valid_signatures(
            signatures,
            verify_hash,
            pre_commit.state_hash,
            ctx.view_number,
        )?;

        let qc = QuorumCertificate {
            stage: Stage::Commit,
            signatures: valid_signatures,
            genesis: false,
            ..pre_commit.qc
        };

        let commit = Commit {
            leaf_hash: qc.leaf_hash,
            qc,
            current_view: ctx.view_number,
            state_hash: pre_commit.state_hash,
        };

        let vote = if ctx.api.leader_acts_as_replica() {
            let signature = ctx
                .api
                .sign_vote(&commit.leaf_hash, Stage::Commit, ctx.view_number);
            if let Some(token) = ctx
                .api
                .generate_vote_token(ctx.view_number, pre_commit.state_hash)
            {
                Some(CommitVote(Vote {
                    leaf_hash: commit.leaf_hash,
                    token,
                    signature,
                    current_view: ctx.view_number,
                    chain_id: ctx.api.chain_id(),
                }))
            } else {
                None
            }
        } else {
            None
        };

        Ok(Outcome {
            commit,
            vote,
            starting_qc: self.starting_qc.clone(),
        })
    }
}
