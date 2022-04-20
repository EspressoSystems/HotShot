use crate::{
    phase::{decide::DecidePhase, err, Progress, UpdateCtx},
    ConsensusApi, Result,
};
use phaselock_types::{
    data::{QuorumCertificate, Stage},
    error::{FailedToBroadcastSnafu, PhaseLockError, StorageSnafu},
    message::{Commit, ConsensusMessage, Decide, Vote},
    traits::{node_implementation::NodeImplementation, storage::Storage, State},
};
use snafu::ResultExt;
use tracing::{debug, info, trace, warn};

#[derive(Debug)]
pub(crate) struct CommitLeader<const N: usize> {
    #[allow(dead_code)]
    commit: Option<Commit<N>>,
    #[allow(dead_code)]
    vote: Option<Vote<N>>,
}

impl<const N: usize> CommitLeader<N> {
    pub(super) fn new(commit: Option<Commit<N>>, vote: Option<Vote<N>>) -> Self {
        Self { commit, vote }
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<DecidePhase>> {
        let commit = if let Some(commit) = &self.commit {
            commit
        } else if let Some(commit) = ctx.commit_message() {
            commit
        } else {
            return Ok(Progress::NotReady);
        };

        let valid_votes: Vec<Vote<N>> = ctx
            .pre_commit_vote_messages()
            // make sure to append our own vote if we have one
            .chain(self.vote.iter())
            .filter(|vote| vote.leaf_hash == commit.leaf_hash)
            .cloned()
            .collect();

        if valid_votes.len() as u64 >= ctx.api.threshold().get() {
            let commit = commit.clone();
            let result = self.create_decide(ctx, commit, valid_votes).await?;
            Ok(Progress::Next(result))
        } else {
            Ok(Progress::NotReady)
        }
    }

    async fn create_decide<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
        commit: Commit<N>,
        votes: Vec<Vote<N>>,
    ) -> Result<DecidePhase> {
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
        debug!(?qc, "decide qc generated");
        let old_qc = match ctx.get_newest_qc().await? {
            Some(qc) => qc,
            None => {
                return err("No QC in storage");
            }
        };
        // Find blocks and states that were commited
        let mut walk_leaf = commit.leaf_hash;
        let old_leaf_hash = old_qc.leaf_hash;

        let mut blocks = vec![];
        let mut states = vec![];
        while walk_leaf != old_leaf_hash {
            debug!(?walk_leaf, "Looping");
            let leaf = if let Some(x) = ctx
                .api
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
            let state = if let Some(x) = ctx
                .api
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
            blocks.push(leaf.item);
            states.push(state);
            walk_leaf = leaf.parent;
        }
        for state in &states {
            state.on_commit();
        }
        info!(?blocks, ?states, "Sending decide events");
        // Send decide event
        ctx.api.notify(blocks.clone(), states.clone()).await;
        let events = blocks
            .into_iter()
            .zip(states.into_iter())
            .collect::<Vec<_>>();
        ctx.api.send_decide(ctx.view_number.0, &events).await;

        // Add qc to decision cache
        ctx.api
            .storage()
            .update(|mut m| {
                let qc = qc.clone();
                async move { m.insert_qc(qc).await }
            })
            .await
            .context(StorageSnafu)?;
        trace!(?qc, "New state written");
        // Broadcast the decision
        let decide = Decide {
            leaf_hash: qc.leaf_hash,
            qc,
            current_view: ctx.view_number.0,
        };

        ctx.api
            .send_broadcast_message(ConsensusMessage::Decide(decide))
            .await
            .context(FailedToBroadcastSnafu {
                stage: Stage::Decide,
            })?;

        let is_leader_next_phase = ctx.api.is_leader(ctx.view_number.0, Stage::Decide).await;
        Ok(if is_leader_next_phase {
            DecidePhase::leader(true)
        } else {
            DecidePhase::replica(true)
        })
    }
}
