//! Prepare leader implementation

use super::Outcome;
use crate::{phase::UpdateCtx, utils, ConsensusApi, Result, TransactionState};
use hotshot_types::{
    data::{Leaf, QuorumCertificate, Stage, ViewNumber},
    error::HotShotError,
    message::{NewView, Prepare, PrepareVote, Vote},
    traits::{node_implementation::NodeImplementation, BlockContents, State},
};
use std::time::Instant;
use tracing::{debug, error, instrument, trace, warn};

/// A prepare leader
#[derive(Debug)]
pub(crate) struct PrepareLeader<const N: usize> {
    /// The `high_qc` that was calculated
    pub(super) high_qc: Option<QuorumCertificate<N>>,
    /// The timestamp at which this round started. This will be after enough `NewView` messages have been received
    pub(super) round_start: Option<Instant>,
}

impl<const N: usize> PrepareLeader<N> {
    /// Create a new leader
    pub(super) fn new() -> Self {
        Self {
            high_qc: None,
            round_start: None,
        }
    }

    /// Update this leader. This will:
    /// - Wait for [`NewView`] messages until the threshold is reached
    /// - Calculate a `high_qc`
    /// - Wait for at least `propose_min_round_time`
    /// - Wait for at least 1 transaction to show up, or until `propose_max_round_time` has been elapsed
    /// - Propose a round based on the given transactions, returning [`Outcome`]
    ///
    /// # Errors
    ///
    /// Will return an error if:
    /// - The underlying [`ConsensusApi`] encountered an error
    /// - The proposed block could not have been added to the state
    #[instrument]
    pub(super) async fn update<'a, I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
        transactions: &'a mut [TransactionState<I, N>],
    ) -> Result<Option<Outcome<'a, I, N>>> {
        if self.high_qc.is_none() {
            let view_messages: Vec<&NewView<N>> = ctx.new_view_messages().collect();
            if view_messages.len() >= ctx.api.threshold().get() {
                // this `.unwrap()` is fine because `api.threshold()` is a NonZeroUsize.
                // `max_by_key` only returns `None` if there are no entries, but we check above that there is at least 1 entry.
                let high_qc = view_messages
                    .into_iter()
                    .max_by_key(|qc| qc.current_view)
                    .unwrap()
                    .justify
                    .clone();
                self.high_qc = Some(high_qc);
                self.round_start = Some(Instant::now());
            }
        }

        let elapsed = if let Some(round_start) = &self.round_start {
            round_start.elapsed()
        } else {
            return Ok(None);
        };

        if elapsed < ctx.api.propose_min_round_time() {
            // the minimum round time has not elapsed
            return Ok(None);
        }

        // Get unclaimed transactions
        let mut unclaimed_transactions = Vec::new();
        for transaction in transactions {
            if transaction.is_unclaimed().await {
                unclaimed_transactions.push(transaction);
            }
        }

        if unclaimed_transactions.is_empty() && elapsed < ctx.api.propose_max_round_time() {
            // we have no transactions and the max round time has not elapsed yet
            return Ok(None);
        }

        // we either have transactions, or the round has timed out
        // so we need to propose a round now
        let outcome = self.propose_round(ctx, unclaimed_transactions).await?;
        Ok(Some(outcome))
    }

    /// Propose a round with the given `transactions`.
    ///
    /// # Errors
    ///
    /// Errors are described in the documentation of `update`
    async fn propose_round<'a, I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &self,
        ctx: &UpdateCtx<'_, I, A, N>,
        transactions: Vec<&'a mut TransactionState<I, N>>,
    ) -> Result<Outcome<'a, I, N>> {
        let mut high_qc = match self.high_qc.clone() {
            Some(high_qc) => high_qc,
            None => return utils::err("in propose_round: no high_qc set"),
        };
        let leaf = ctx.get_leaf_by_block(&high_qc.block_hash).await?;
        let leaf_hash = leaf.hash();
        let state = ctx.get_state_by_leaf(&leaf_hash).await?;
        trace!(?state, ?leaf_hash);
        let mut block = state.next_block();
        let current_view = ctx.view_number;

        // This is to accomodate for the off by one error where the replicas have signed the QC with the previous stage
        let qc_stage = match high_qc.stage {
            Stage::PreCommit => Stage::Prepare,
            Stage::Commit => Stage::PreCommit,
            Stage::Decide => Stage::Commit,
            Stage::Prepare => Stage::Decide,
            Stage::None => Stage::None,
        };

        let verify_hash = ctx
            .api
            .create_verify_hash(&leaf_hash, qc_stage, high_qc.view_number);

        // try to append unclaimed transactions to the block
        let mut added_transactions = Vec::new();
        let mut rejected_transactions = Vec::new();
        for transaction in transactions {
            let tx = &transaction.transaction;
            // Make sure the transaction is valid given the current state,
            // otherwise, discard it
            let new_block = block.add_transaction_raw(tx);
            let new_block = match new_block {
                Ok(new_block) => new_block,
                Err(e) => {
                    warn!(?e, ?tx, "Invalid transaction rejected");
                    rejected_transactions.push(transaction);
                    continue;
                }
            };
            if state.validate_block(&new_block) {
                block = new_block;
                debug!(?tx, "Added transaction to block");
                added_transactions.push(transaction);
            } else {
                let err = state.append(&new_block).unwrap_err();
                warn!(?tx, ?err, "Invalid transaction rejected");
                rejected_transactions.push(transaction);
            }
        }

        // Create new leaf and add it to the store
        let new_leaf = Leaf::new(block, high_qc.leaf_hash);
        // let the_hash = new_leaf.hash();
        let new_state = state.append(&new_leaf.item).map_err(|error| {
            error!(?error, "Failed to append block to existing state");
            HotShotError::InconsistentBlock {
                stage: Stage::Prepare,
            }
        })?;

        if high_qc.view_number != ViewNumber::genesis() {
            let valid_signatures = ctx.api.get_valid_signatures(
                high_qc.signatures.clone(),
                verify_hash,
                high_qc.state_hash,
                high_qc.view_number,
            )?;
            high_qc.signatures = valid_signatures;
        }

        let prepare = Prepare {
            current_view,
            leaf: new_leaf.clone(),
            high_qc: high_qc.clone(),
            state: new_state.clone(),
            chain_id: ctx.api.chain_id(),
        };
        let leaf_hash = new_leaf.hash();

        // if the leader can vote like a replica, cast this vote now
        let vote = if ctx.api.leader_acts_as_replica() {
            let signature = ctx.api.sign_vote(&leaf_hash, Stage::Prepare, current_view);
            ctx.api
                .generate_vote_token(current_view, new_state.hash())
                .map(|token| {
                    PrepareVote(Vote {
                        signature,
                        token,
                        leaf_hash,
                        current_view,
                        chain_id: ctx.api.chain_id(),
                    })
                })
        } else {
            None
        };
        Ok(Outcome {
            added_transactions,
            rejected_transactions,
            new_leaf,
            new_state,
            prepare,
            vote,
            newest_qc: high_qc,
        })
    }
}
