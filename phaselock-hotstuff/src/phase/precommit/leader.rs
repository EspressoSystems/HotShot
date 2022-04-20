use super::Outcome;
use crate::{phase::UpdateCtx, ConsensusApi, Result};
use phaselock_types::{
    data::{QuorumCertificate, Stage},
    error::PhaseLockError,
    message::{PreCommit, PreCommitVote, Prepare, PrepareVote, Vote},
    traits::{node_implementation::NodeImplementation, BlockContents},
};
use tracing::debug;

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct PreCommitLeader<I: NodeImplementation<N>, const N: usize> {
    prepare: Prepare<I::Block, I::State, N>,
    vote: Option<PrepareVote<N>>,
}

impl<I: NodeImplementation<N>, const N: usize> PreCommitLeader<I, N> {
    pub(super) fn new(
        prepare: Prepare<I::Block, I::State, N>,
        vote: Option<PrepareVote<N>>,
    ) -> Self {
        Self { prepare, vote }
    }

    pub(super) async fn update<A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &UpdateCtx<'_, I, A, N>,
    ) -> Result<Option<Outcome<N>>> {
        // Collect all votes that target this `leaf_hash`
        let new_leaf_hash = self.prepare.leaf.hash();
        let valid_votes: Vec<PrepareVote<N>> = ctx
            .prepare_vote_messages()
            // make sure to append our own vote if we have one
            .chain(self.vote.iter())
            .filter(|vote| vote.leaf_hash == new_leaf_hash)
            .cloned()
            .collect();
        if valid_votes.len() as u64 >= ctx.api.threshold().get() {
            let prepare = self.prepare.clone();
            let outcome = self.create_commit(ctx, prepare, valid_votes).await?;
            Ok(Some(outcome))
        } else {
            Ok(None)
        }
    }

    async fn create_commit<A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &UpdateCtx<'_, I, A, N>,
        prepare: Prepare<I::Block, I::State, N>,
        votes: Vec<PrepareVote<N>>,
    ) -> Result<Outcome<N>> {
        let signature = ctx
            .api
            .public_key()
            .set
            .combine_signatures(votes.iter().map(|v| (v.id, &v.signature)))
            .map_err(|source| PhaseLockError::FailedToAssembleQC {
                stage: Stage::PreCommit,
                source,
            })?;

        // TODO: Should we `safe_node` the incoming `Prepare`?
        let block_hash = prepare.leaf.item.hash();
        let leaf_hash = prepare.leaf.hash();
        let current_view = ctx.view_number.0;
        let qc = QuorumCertificate {
            block_hash,
            leaf_hash,
            view_number: current_view,
            stage: Stage::PreCommit,
            signature: Some(signature),
            genesis: false,
        };
        debug!(?qc, "commit qc generated");
        let pre_commit = PreCommit {
            leaf_hash,
            qc,
            current_view,
        };

        let vote = if ctx.api.leader_acts_as_replica() {
            // Make a pre commit vote and send it to the next leader
            let signature =
                ctx.api
                    .private_key()
                    .partial_sign(&leaf_hash, Stage::Commit, current_view);
            Some(PreCommitVote(Vote {
                leaf_hash,
                signature,
                id: ctx.api.public_key().nonce,
                current_view,
            }))
        } else {
            None
        };

        Ok(Outcome { pre_commit, vote })
    }
}
