use super::Ctx;
use crate::{ConsensusApi, ConsensusMessage, Result};
use phaselock_types::{
    data::{LeafHash, QuorumCertificate, Stage},
    error::{FailedToBroadcastSnafu, PhaseLockError, StorageSnafu},
    message::{Decide, Vote},
    traits::{
        node_implementation::{NodeImplementation, TypeMap},
        storage::Storage,
        BlockContents, State,
    },
};
use snafu::ResultExt;
use tracing::{debug, info, instrument, trace, warn};

#[derive(Debug, Clone)]
#[allow(dead_code)] // TODO(vko): Prune unused variables
pub struct Commit<I: NodeImplementation<N>, const N: usize> {
    ctx: Ctx<I, N>,
    qc: QuorumCertificate<N>,
    expected_leaf_hash: LeafHash<N>,
    votes: Vec<Vote<N>>,
}

pub(super) enum CommitTransition {
    Stay,
    End { current_view: u64 },
}

impl<I: NodeImplementation<N>, const N: usize> Commit<I, N> {
    pub(super) fn new(ctx: Ctx<I, N>, qc: QuorumCertificate<N>, vote: Vote<N>) -> Self {
        Self {
            ctx,
            qc,
            // `initial_vote` is our vote, so this always contains the leaf_hash we expect
            expected_leaf_hash: vote.leaf_hash,
            votes: vec![vote],
        }
    }

    #[instrument(skip(api))]
    pub(super) async fn step(
        &mut self,
        msg: ConsensusMessage<'_, I, N>,
        api: &mut impl ConsensusApi<I, N>,
    ) -> Result<CommitTransition> {
        match msg {
            ConsensusMessage::CommitVote(vote) => self.precommit_vote(vote, api).await,
            msg => {
                warn!(?msg, "ignored");
                Ok(CommitTransition::Stay)
            }
        }
    }

    async fn precommit_vote(
        &mut self,
        vote: Vote<N>,
        api: &mut impl ConsensusApi<I, N>,
    ) -> Result<CommitTransition> {
        // The commit phase is similar to pre-commit phase. When the leader receives (n − f) pre-commit
        // votes[1], it combines them into a precommitQC [2] and broadcasts it in commit messages[3];

        // [1]
        if vote.leaf_hash != self.expected_leaf_hash {
            info!(?vote, ?self.expected_leaf_hash, "Vote does not match expected leaf hash, ignoring");
            return Ok(CommitTransition::Stay);
        }
        self.votes.push(vote);
        if self.votes.len() < api.threshold().get() as usize {
            return Ok(CommitTransition::Stay);
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
                stage: Stage::Commit,
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

        debug!(?qc, "decide qc generated");
        let old_leaf = self.ctx.committed_leaf.hash();
        let mut events = vec![];
        let mut walk_leaf = self.expected_leaf_hash;
        while walk_leaf != old_leaf {
            debug!(?walk_leaf, "Looping");
            let block = if let Some(x) = api
                .storage()
                .get_leaf(&walk_leaf)
                .await
                .context(StorageSnafu)?
            {
                x
            } else {
                warn!(?walk_leaf, "Parent did not exist in store");
                break;
            };
            let state = if let Some(x) = api
                .storage()
                .get_state(&walk_leaf)
                .await
                .context(StorageSnafu)?
            {
                x
            } else {
                warn!(?walk_leaf, "Parent did not exist in store");
                break;
            };
            state.on_commit();
            events.push((block.item, state));
            walk_leaf = block.parent;
        }
        info!(?events, "Sending decide events");
        // Send decide event
        api.notify(
            events.iter().map(|(x, _)| x.clone()).collect(),
            events.iter().map(|(_, x)| x.clone()).collect(),
        )
        .await;
        api.send_decide(self.ctx.current_view, &events).await;

        // Add qc to decision cache
        api.storage()
            .update(|mut m| {
                let qc = qc.clone();
                async move { m.insert_qc(qc).await }
            })
            .await
            .context(StorageSnafu)?;
        trace!("New state written");

        // Broadcast the decision
        api.send_broadcast_message(<I as TypeMap<N>>::ConsensusMessage::Decide(Decide {
            leaf_hash: self.expected_leaf_hash,
            qc,
            current_view: self.ctx.current_view,
        }))
        .await
        .context(FailedToBroadcastSnafu {
            stage: Stage::Decide,
        })?;

        Ok(CommitTransition::End {
            current_view: self.ctx.current_view,
        })
    }
}
