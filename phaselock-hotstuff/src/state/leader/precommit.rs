use super::Ctx;
use crate::{ConsensusApi, ConsensusMessage, Result};
use phaselock_types::{
    data::{LeafHash, QuorumCertificate, Stage},
    error::{FailedToBroadcastSnafu, PhaseLockError},
    message::{PreCommit as PreCommitMessage, Vote},
    traits::{
        node_implementation::{NodeImplementation, TypeMap},
        BlockContents,
    },
};
use snafu::ResultExt;
use tracing::{info, instrument, warn};

#[derive(Debug, Clone)]
pub struct PreCommit<I: NodeImplementation<N>, const N: usize> {
    ctx: Ctx<I, N>,
    expected_leaf_hash: LeafHash<N>,
    votes: Vec<Vote<N>>,
}

pub(super) enum PreCommitTransition<I: NodeImplementation<N>, const N: usize> {
    Stay,
    Commit {
        ctx: Ctx<I, N>,
        qc: QuorumCertificate<N>,
        vote: Vote<N>,
    },
}

impl<I: NodeImplementation<N>, const N: usize> PreCommit<I, N> {
    pub(super) fn new(ctx: Ctx<I, N>, initial_vote: Vote<N>) -> Self {
        Self {
            ctx,
            // `initial_vote` is our vote, so this always contains the leaf_hash we expect
            expected_leaf_hash: initial_vote.leaf_hash,
            votes: vec![initial_vote],
        }
    }

    #[instrument(skip(api))]
    pub(super) async fn step(
        &mut self,
        msg: ConsensusMessage<'_, I, N>,
        api: &mut impl ConsensusApi<I, N>,
    ) -> Result<PreCommitTransition<I, N>> {
        match msg {
            ConsensusMessage::PrepareVote(vote) => self.prepare_vote(vote, api).await,
            msg => {
                warn!(?msg, "ignored");
                Ok(PreCommitTransition::Stay)
            }
        }
    }

    async fn prepare_vote(
        &mut self,
        vote: Vote<N>,
        api: &mut impl ConsensusApi<I, N>,
    ) -> Result<PreCommitTransition<I, N>> {
        // When the leader receives (n − f) prepare votes for the current proposal curProposal[1] , it
        // combines them into a prepareQC [2]. The leader broadcasts prepareQC in pre-commit messages. [3] A replica responds
        // to the leader with pre-commit vote having a signed digest of the proposal.

        // [1]
        if vote.leaf_hash != self.expected_leaf_hash {
            info!(?vote, ?self.expected_leaf_hash, "Vote does not match expected leaf hash, ignoring");
            return Ok(PreCommitTransition::Stay);
        }
        self.votes.push(vote);
        if self.votes.len() < api.threshold().get() as usize {
            return Ok(PreCommitTransition::Stay);
        }

        // [2]
        let key_set = &api.public_key().set;
        let signatures = self
            .votes
            .iter()
            .map(|vote| (vote.id, &vote.signature))
            .collect::<Vec<_>>();
        let signature = key_set.combine_signatures(signatures).map_err(|source| {
            PhaseLockError::FailedToAssembleQC {
                stage: Stage::PreCommit,
                source,
            }
        })?;
        let qc = QuorumCertificate {
            block_hash: self.ctx.block.hash(),
            leaf_hash: self.expected_leaf_hash,
            signature: Some(signature),
            stage: Stage::Prepare,
            view_number: self.ctx.current_view,
            genesis: false,
        };
        // [3]
        api.send_broadcast_message(<I as TypeMap<N>>::ConsensusMessage::PreCommit(
            PreCommitMessage {
                leaf_hash: self.expected_leaf_hash,
                qc: qc.clone(),
                current_view: self.ctx.current_view,
            },
        ))
        .await
        .context(FailedToBroadcastSnafu {
            stage: Stage::PreCommit,
        })?;

        // Make a vote and send it to ourself
        let signature = api.private_key().partial_sign(
            &self.expected_leaf_hash,
            Stage::PreCommit,
            self.ctx.current_view,
        );
        let vote = Vote {
            leaf_hash: self.expected_leaf_hash,
            signature,
            id: api.public_key().nonce,
            current_view: self.ctx.current_view,
            stage: Stage::Prepare,
        };

        Ok(PreCommitTransition::Commit {
            ctx: self.ctx.clone(),
            qc,
            vote,
        })
    }
}
