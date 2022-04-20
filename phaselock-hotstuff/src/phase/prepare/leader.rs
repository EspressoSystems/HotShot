use super::Outcome;
use crate::{
    phase::{err, precommit::PreCommitPhase, Phase, Progress, UpdateCtx},
    utils, ConsensusApi, Result, TransactionLink, TransactionState, ViewNumber,
};
use phaselock_types::{
    data::{Leaf, QuorumCertificate, Stage, TransactionHash},
    error::{FailedToBroadcastSnafu, FailedToMessageLeaderSnafu, PhaseLockError, StorageSnafu},
    message::{ConsensusMessage, Prepare, PrepareVote, Vote},
    traits::{
        node_implementation::NodeImplementation, storage::Storage, BlockContents, State,
        Transaction,
    },
};
use snafu::ResultExt;
use std::time::Instant;
use tracing::{debug, error, trace, warn};

#[derive(Debug)]
pub(crate) struct PrepareLeader<const N: usize> {
    high_qc: Option<QuorumCertificate<N>>,
    round_start: Option<Instant>,
}

impl<const N: usize> PrepareLeader<N> {
    pub(super) fn new() -> Self {
        Self {
            high_qc: None,
            round_start: None,
        }
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &UpdateCtx<'_, I, A, N>,
    ) -> Result<Option<super::Outcome<I, N>>> {
        if self.high_qc.is_none() {
            let view_messages = ctx.new_view_messages().collect::<Vec<_>>();
            if view_messages.len() as u64 >= ctx.api.threshold().get() {
                // this `.unwrap()` is fine because `api.threshold()` is a NonZeroU64.
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
        for transaction in ctx.transactions {
            if transaction.is_unclaimed().await {
                unclaimed_transactions.push(transaction.clone());
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

    async fn propose_round<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &self,
        ctx: &UpdateCtx<'_, I, A, N>,
        transactions: Vec<TransactionState<I, N>>,
    ) -> Result<Outcome<I, N>> {
        let high_qc = match self.high_qc.clone() {
            Some(high_qc) => high_qc,
            None => return err("in propose_round: no high_qc set"),
        };
        let leaf = ctx.get_leaf_by_block(&high_qc.block_hash).await?;
        let leaf_hash = leaf.hash();
        let state = ctx.get_state_by_leaf(&leaf_hash).await?;
        trace!(?state, ?leaf_hash);
        let mut block = state.next_block();
        let view_number = ctx.view_number;
        let current_view = ctx.view_number.0;

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
                // TODO: `state.append` could change our state.
                // we should probably make `validate_block` return this error.
                let err = state.append(&new_block).unwrap_err();
                warn!(?tx, ?err, "Invalid transaction rejected");
                rejected_transactions.push(transaction);
            }
        }

        // Create new leaf and add it to the store
        let new_leaf = Leaf::new(block.clone(), high_qc.leaf_hash);
        // let the_hash = new_leaf.hash();
        let new_state = state.append(&new_leaf.item).map_err(|error| {
            error!(?error, "Failed to append block to existing state");
            PhaseLockError::InconsistentBlock {
                stage: Stage::Prepare,
            }
        })?;

        let prepare = Prepare {
            current_view,
            leaf: new_leaf.clone(),
            high_qc: high_qc.clone(),
            state: new_state.clone(),
        };
        let leaf_hash = new_leaf.hash();

        // if the leader can vote like a replica, cast this vote now
        let next_leader = ctx.api.get_leader(current_view, Stage::PreCommit).await;
        let is_leader_next_phase = ctx.api.public_key() == &next_leader;
        let vote = if ctx.api.leader_acts_as_replica() {
            let signature =
                ctx.api
                    .private_key()
                    .partial_sign(&leaf_hash, Stage::Prepare, current_view);
            Some(PrepareVote(Vote {
                signature,
                leaf_hash,
                id: ctx.api.public_key().nonce,
                current_view,
            }))
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
        })
    }
}
