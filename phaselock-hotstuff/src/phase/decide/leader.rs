use super::Outcome;
use crate::{
    phase::{err, UpdateCtx},
    utils, ConsensusApi, Result,
};
use phaselock_types::{
    data::{QuorumCertificate, Stage},
    error::PhaseLockError,
    message::{Commit, CommitVote, Decide},
    traits::node_implementation::NodeImplementation,
};
use tracing::debug;

#[derive(Debug)]
#[allow(dead_code)] // TODO(vko): cleanup
pub struct DecideLeader<const N: usize> {
    commit: Commit<N>,
    vote: Option<CommitVote<N>>,
}

impl<const N: usize> DecideLeader<N> {
    pub fn new(commit: Commit<N>, vote: Option<CommitVote<N>>) -> Self {
        Self { commit, vote }
    }

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
        if valid_votes.len() as u64 >= ctx.api.threshold().get() {
            let outcome = self.decide(ctx, self.commit.clone(), valid_votes).await?;
            Ok(Some(outcome))
        } else {
            Ok(None)
        }
    }

    async fn decide<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &self,
        ctx: &UpdateCtx<'_, I, A, N>,
        commit: Commit<N>,
        votes: Vec<CommitVote<N>>,
    ) -> Result<Outcome<I, N>> {
        // Generate QC
        let signature = ctx
            .api
            .public_key()
            .set
            .combine_signatures(votes.iter().map(|vote| (vote.id, &vote.signature)))
            .map_err(|e| PhaseLockError::FailedToAssembleQC {
                stage: Stage::Decide,
                source: e,
            })?;

        let qc = QuorumCertificate {
            stage: Stage::Commit,
            signature: Some(signature),
            genesis: false,
            ..commit.qc
        };

        let decide = Decide {
            leaf_hash: qc.leaf_hash,
            qc,
            current_view: ctx.view_number.0,
        };
        debug!(?decide.qc, "decide qc generated");

        let old_qc = match ctx.get_newest_qc().await? {
            Some(qc) => qc,
            None => {
                return err("No QC in storage");
            }
        };
        // Find blocks and states that were commited
        let walk_leaf = decide.leaf_hash;
        let old_leaf_hash = old_qc.leaf_hash;

        let (blocks, states) = utils::walk_leaves(ctx.api, walk_leaf, old_leaf_hash).await?;

        Ok(Outcome {
            blocks,
            states,
            decide,
        })
    }
}
