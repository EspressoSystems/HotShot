use super::Outcome;
use crate::{phase::UpdateCtx, utils, ConsensusApi, Result};
use phaselock_types::{
    data::{QuorumCertificate, Stage},
    message::{Commit, CommitVote, Decide},
    traits::node_implementation::NodeImplementation,
};
use tracing::debug;

/// The leader
#[derive(Debug)]
pub struct DecideLeader<const N: usize> {
    /// The commit that was created or voted on last stage
    commit: Commit<N>,
    /// Optionally the vote that we cast last stage
    vote: Option<CommitVote<N>>,
    /// The QC that this round started with
    starting_qc: QuorumCertificate<N>,
}

impl<const N: usize> DecideLeader<N> {
    /// Create a new leader
    pub fn new(
        starting_qc: QuorumCertificate<N>,
        commit: Commit<N>,
        vote: Option<CommitVote<N>>,
    ) -> Self {
        Self {
            commit,
            vote,
            starting_qc,
        }
    }

    /// Update the leader. This will:
    /// - Get the votes that are targetting the current [`Commit`]
    /// - If enough votes have been received:
    ///   - Combine the signatures
    ///   - Create a new QC
    ///   - Get the blocks and states that were committed
    ///
    /// # Errors
    ///
    /// Will return an error if:
    /// - A signature could not be created
    /// - There was no QC in storage
    /// - `utils::walk_leaves` returns an error
    #[tracing::instrument]
    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &UpdateCtx<'_, I, A, N>,
    ) -> Result<Option<Outcome<I, N>>> {
        let valid_votes: Vec<CommitVote<N>> = ctx
            .commit_vote_messages()
            // make sure to append our own vote if we have one
            .chain(self.vote.iter())
            .filter(|vote| vote.leaf_hash == self.commit.leaf_hash)
            .cloned()
            .collect();
        if valid_votes.len() >= ctx.api.threshold().get() {
            let outcome = self.decide(ctx, self.commit.clone(), valid_votes).await?;
            Ok(Some(outcome))
        } else {
            Ok(None)
        }
    }

    /// Decide on the given [`Commit`] and list of [`CommitVote`]
    ///
    /// # Errors
    ///
    /// Errors are described in the documentation of `update`
    async fn decide<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &self,
        ctx: &UpdateCtx<'_, I, A, N>,
        commit: Commit<N>,
        votes: Vec<CommitVote<N>>,
    ) -> Result<Outcome<I, N>> {
        // Generate QC
        let signatures = votes.iter().map(|x| x.signature.clone()).collect();

        let qc = QuorumCertificate {
            stage: Stage::Commit,
            signatures,
            genesis: false,
            ..commit.qc
        };

        let decide = Decide {
            leaf_hash: qc.leaf_hash,
            qc,
            current_view: ctx.view_number,
        };
        debug!(?decide.qc, "decide qc generated");

        // Find blocks and states that were commited
        // TODO: Walk from `storage().locked_qc()` instead
        let walk_leaf = decide.leaf_hash;
        let old_leaf_hash = self.starting_qc.leaf_hash;

        let (blocks, states) = utils::walk_leaves(ctx.api, walk_leaf, old_leaf_hash).await?;

        Ok(Outcome {
            blocks,
            states,
            decide,
        })
    }
}
