use crate::{
    phase::{commit::CommitPhase, Progress, UpdateCtx},
    ConsensusApi, Result,
};
use phaselock_types::{
    data::{QuorumCertificate, Stage},
    error::{FailedToBroadcastSnafu, FailedToMessageLeaderSnafu, PhaseLockError},
    message::{Commit, ConsensusMessage, Prepare, Vote},
    traits::{node_implementation::NodeImplementation, BlockContents},
};
use snafu::ResultExt;
use tracing::{debug, warn};

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct PreCommitLeader<I: NodeImplementation<N>, const N: usize> {
    prepare: Option<Prepare<I::Block, I::State, N>>,
    vote: Option<Vote<N>>,
}

impl<I: NodeImplementation<N>, const N: usize> PreCommitLeader<I, N> {
    pub(super) fn new(
        prepare: Option<Prepare<I::Block, I::State, N>>,
        vote: Option<Vote<N>>,
    ) -> Self {
        Self { prepare, vote }
    }

    pub(super) async fn update<A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<CommitPhase<N>>> {
        // Get the prepare message
        // TODO: Should we loop through all incoming prepare messages and get the first that has enough votes?
        let prepare = if let Some(prepare) = &self.prepare {
            prepare
        } else if let Some(prepare) = ctx.prepare_message() {
            prepare
        } else {
            return Ok(Progress::NotReady);
        };
        // Collect all votes that target this `leaf_hash`
        let new_leaf_hash = prepare.leaf.hash();
        let valid_votes: Vec<Vote<N>> = ctx
            .prepare_vote_messages()
            // make sure to append our own vote if we have one
            .chain(self.vote.iter())
            .filter(|vote| vote.leaf_hash == new_leaf_hash)
            .cloned()
            .collect();
        if valid_votes.len() as u64 >= ctx.api.threshold().get() {
            let prepare = prepare.clone();
            let result = self.create_commit(ctx, prepare, valid_votes).await?;
            Ok(Progress::Next(result))
        } else {
            Ok(Progress::NotReady)
        }
    }

    async fn create_commit<A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
        prepare: Prepare<I::Block, I::State, N>,
        votes: Vec<Vote<N>>,
    ) -> Result<CommitPhase<N>> {
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
        let commit = Commit {
            leaf_hash,
            qc,
            current_view,
        };
        let network_result = ctx
            .api
            .send_broadcast_message(ConsensusMessage::Commit(commit.clone()))
            .await
            .context(FailedToBroadcastSnafu {
                stage: Stage::Commit,
            });
        if let Err(e) = network_result {
            warn!(?e, "Failed to broadcast commit message");
        }
        debug!("Commit message sent");
        let mut vote = None;
        if ctx.api.leader_acts_as_replica() {
            // Make a pre commit vote and send it to the next leader
            let signature =
                ctx.api
                    .private_key()
                    .partial_sign(&leaf_hash, Stage::Commit, current_view);
            let send_vote = Vote {
                leaf_hash,
                signature,
                id: ctx.api.public_key().nonce,
                current_view,
                stage: Stage::Commit,
            };
            vote = Some(send_vote.clone());
            let leader = ctx.api.get_leader(current_view, Stage::Commit).await;
            ctx.api
                .send_direct_message(leader, ConsensusMessage::PreCommitVote(send_vote))
                .await
                .context(FailedToMessageLeaderSnafu {
                    stage: Stage::PreCommit,
                })?;
        }

        Ok(if ctx.api.is_leader(current_view, Stage::Commit).await {
            CommitPhase::leader(Some(commit), vote)
        } else {
            CommitPhase::replica(Some(commit))
        })
    }
}
