use std::collections::BTreeMap;

use super::Outcome;
use crate::{phase::UpdateCtx, ConsensusApi, Result};
use hotshot_types::{
    data::{QuorumCertificate, Stage},
    error::HotShotError,
    message::{PreCommit, PreCommitVote, Prepare, PrepareVote, Vote},
    traits::{node_implementation::NodeImplementation, signature_key::SignatureKey, BlockContents},
};
use tracing::{debug, warn};

/// a precommit leader
#[derive(Debug)]
pub(crate) struct PreCommitLeader<I: NodeImplementation<N>, const N: usize> {
    /// The prepare block that was proposed or voted on last stage.
    prepare: Prepare<I::Block, I::State, N>,
    /// The vote that we might have casted ourselves last stage.
    vote: Option<PrepareVote<N>>,
    /// The QC that this round started with
    starting_qc: QuorumCertificate<N>,
}

impl<I: NodeImplementation<N>, const N: usize> PreCommitLeader<I, N> {
    /// Create a new leader
    pub(super) fn new(
        starting_qc: QuorumCertificate<N>,
        prepare: Prepare<I::Block, I::State, N>,
        vote: Option<PrepareVote<N>>,
    ) -> Self {
        Self {
            prepare,
            vote,
            starting_qc,
        }
    }

    /// Update this leader. This will:
    /// - Get all [`PrepareVote`] messages directed at the given [`Prepare`]
    /// - Once enough votes have been received:
    ///   - Combine the signatures
    ///   - Crate a new [`PreCommit`]
    ///   - Optionally create a vote
    ///
    /// # Errors
    ///
    /// This will return an error if:
    /// - the signatures could not be combined
    /// - A vote could not be signed
    #[tracing::instrument]
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
        if valid_votes.len() >= ctx.api.threshold().get() {
            let prepare = self.prepare.clone();
            let outcome = self.create_commit(ctx, prepare, valid_votes).await?;
            Ok(Some(outcome))
        } else {
            Ok(None)
        }
    }

    /// Create a commit from the given [`Prepare`] and [`PrepareVote`]s
    ///
    /// # Errors
    ///
    /// Errors are described in the documentation of `update`
    async fn create_commit<A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &UpdateCtx<'_, I, A, N>,
        prepare: Prepare<I::Block, I::State, N>,
        votes: Vec<PrepareVote<N>>,
    ) -> Result<Outcome<N>> {
        let mut signatures = BTreeMap::new();
        let block_hash = prepare.leaf.item.hash();
        let leaf_hash = prepare.leaf.hash();
        let current_view = ctx.view_number;

        let locked_qc = ctx.get_newest_qc().await?.unwrap();
        if !crate::utils::validate_against_locked_qc(
            ctx.api,
            &locked_qc,
            &prepare.leaf,
            &prepare.high_qc,
        )
        .await
        {
            warn!(
                ?prepare,
                ?locked_qc,
                "Incoming prepare is not valid against locked QC"
            );
            return Err(HotShotError::InvalidState {
                context: "Incoming prepare is not valid against locked QC".to_string(),
            });
        }

        let verify_hash = ctx
            .api
            .create_verify_hash(&leaf_hash, Stage::Prepare, current_view);
        for vote in votes {
            let (encoded_pub_key, signature) = &vote.0.signature;
            let pub_key = match <I::SignatureKey as SignatureKey>::from_bytes(encoded_pub_key) {
                Some(pub_key) => pub_key,
                None => {
                    warn!(?vote, "Vote has an invalid public key, ignoring");
                    continue;
                }
            };
            if !pub_key.validate(signature, verify_hash.as_ref()) {
                warn!(
                    ?vote,
                    ?prepare,
                    "Vote is not valid for this prepare proposal, ignoring"
                );
                continue;
            }

            let (encoded_pub_key, signature) = vote.0.signature;
            signatures.insert(encoded_pub_key, signature);
        }

        let qc = QuorumCertificate {
            block_hash,
            leaf_hash,
            view_number: current_view,
            stage: Stage::PreCommit,
            signatures,
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
            let signature = ctx
                .api
                .sign_vote(&leaf_hash, Stage::PreCommit, current_view);
            Some(PreCommitVote(Vote {
                signature,
                leaf_hash,
                current_view,
            }))
        } else {
            None
        };

        Ok(Outcome {
            pre_commit,
            vote,
            starting_qc: self.starting_qc.clone(),
        })
    }
}
