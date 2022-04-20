use super::Outcome;
use crate::{
    phase::{err, UpdateCtx},
    utils, ConsensusApi, Result,
};
use phaselock_types::{
    data::{QuorumCertificate, Stage},
    error::PhaseLockError,
    message::{Commit, CommitVote, PreCommit, PreCommitVote, Vote},
    traits::node_implementation::NodeImplementation,
};
use tracing::{debug, info};

#[derive(Debug)]
pub(crate) struct CommitLeader<const N: usize> {
    pre_commit: PreCommit<N>,
    vote: Option<PreCommitVote<N>>,
}

impl<const N: usize> CommitLeader<N> {
    pub(super) fn new(pre_commit: PreCommit<N>, vote: Option<PreCommitVote<N>>) -> Self {
        Self { pre_commit, vote }
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &self,
        ctx: &UpdateCtx<'_, I, A, N>,
    ) -> Result<Option<Outcome<I, N>>> {
        let valid_votes: Vec<PreCommitVote<N>> = ctx
            .pre_commit_vote_messages()
            // make sure to append our own vote if we have one
            .chain(self.vote.iter())
            .filter(|vote| vote.leaf_hash == self.pre_commit.leaf_hash)
            .cloned()
            .collect();

        if valid_votes.len() as u64 >= ctx.api.threshold().get() {
            let outcome: Outcome<I, N> = self
                .create_commit(ctx, &self.pre_commit, valid_votes)
                .await?;
            Ok(Some(outcome))
        } else {
            Ok(None)
        }
    }

    async fn create_commit<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &self,
        ctx: &UpdateCtx<'_, I, A, N>,
        pre_commit: &PreCommit<N>,
        votes: Vec<PreCommitVote<N>>,
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
            ..pre_commit.qc
        };
        debug!(?qc, "decide qc generated");
        let old_qc = match ctx.get_newest_qc().await? {
            Some(qc) => qc,
            None => {
                return err("No QC in storage");
            }
        };
        // Find blocks and states that were commited
        let walk_leaf = pre_commit.leaf_hash;
        let old_leaf_hash = old_qc.leaf_hash;

        let (blocks, states) = utils::walk_leaves(ctx.api, walk_leaf, old_leaf_hash).await?;
        info!(?blocks, ?states, "Sending decide events");

        let commit = Commit {
            leaf_hash: qc.leaf_hash,
            qc,
            current_view: ctx.view_number.0,
        };

        let vote = if ctx.api.leader_acts_as_replica() {
            let signature = ctx.api.private_key().partial_sign(
                &commit.leaf_hash,
                Stage::Commit,
                ctx.view_number.0,
            );
            Some(CommitVote(Vote {
                leaf_hash: commit.leaf_hash,
                signature,
                id: ctx.api.public_key().nonce,
                current_view: ctx.view_number.0,
            }))
        } else {
            None
        };

        Ok(Outcome {
            blocks,
            states,
            commit,
            vote,
        })
    }
}
