//! Prepare replica implementation

use super::Outcome;
use crate::{phase::UpdateCtx, utils, ConsensusApi, Result, TransactionState};
use phaselock_types::{
    data::Stage,
    error::PhaseLockError,
    message::{Prepare, PrepareVote, Vote},
    traits::{node_implementation::NodeImplementation, State, Transaction as _},
};
use tracing::{error, info};

/// A prepare replica
#[derive(Debug)]
pub(crate) struct PrepareReplica {}

impl PrepareReplica {
    /// Create a new replica
    pub(super) fn new() -> Self {
        Self {}
    }

    /// Update the given replica, returning [`Outcome`] if this phase is ready.
    ///
    /// # Errors
    ///
    /// Will return any errors `vote` returns.
    #[tracing::instrument]
    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
        &mut self,
        ctx: &UpdateCtx<'_, I, A, N>,
    ) -> Result<Option<Outcome<I, N>>> {
        let prepare = if let Some(prepare) = ctx.prepare_message() {
            prepare
        } else {
            return Ok(None);
        };
        let mut added_transactions = Vec::new();
        for id in prepare.state.transaction_ids() {
            if let Some(transaction_state) =
                ctx.transactions.iter().find(|t| t.transaction.id() == id)
            {
                added_transactions.push(transaction_state.clone());
            } else {
                info!(
                    ?id,
                    "Could not find transaction to mark, will sit out until we receive it"
                );
                return Ok(None);
            }
        }

        let outcome = self.vote(ctx, prepare.clone(), added_transactions).await?;
        Ok(Some(outcome))
    }

    /// Cast a vote on the given [`Prepare`]
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - There is no `QuorumCertificate` in storage
    /// - The incoming QC is not a safe node or valid block
    /// - The given block could not be appended to the current state
    /// - The underlying [`ConsensusApi`] returned an error
    async fn vote<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
        &mut self,
        ctx: &UpdateCtx<'_, I, A, N>,
        prepare: Prepare<I::Block, I::State, N>,
        added_transactions: Vec<TransactionState<I, N>>,
    ) -> Result<Outcome<I, N>> {
        let leaf = prepare.leaf.clone();
        let leaf_hash = leaf.hash();
        let high_qc = prepare.high_qc.clone();
        let suggested_state = prepare.state.clone();

        let state = ctx.get_state_by_leaf(&leaf.parent).await?;
        let self_highest_qc = match ctx.get_newest_qc().await? {
            Some(qc) => qc,
            None => return utils::err("No QC in storage"),
        };
        let is_safe_node =
            utils::validate_against_locked_qc(ctx.api, &self_highest_qc, &leaf, &high_qc).await;
        if !is_safe_node || !state.validate_block(&leaf.item) {
            error!("is_safe_node: {}", is_safe_node);
            error!(?leaf, "Leaf failed safe_node predicate");
            return Err(PhaseLockError::BadBlock {
                stage: Stage::Prepare,
            });
        }

        let current_view = ctx.view_number.0;
        // Add resulting state to storage
        let new_state = state.append(&leaf.item).map_err(|error| {
            error!(?error, "Failed to append block to existing state");
            PhaseLockError::InconsistentBlock {
                stage: Stage::Prepare,
            }
        })?;

        if suggested_state != new_state {
            // the state that the leader send does not match with what we calculated
            error!(
                ?new_state,
                ?suggested_state,
                "Suggested state does not match actual state."
            );
            return Err(PhaseLockError::InconsistentBlock {
                stage: Stage::Prepare,
            });
        }

        let signature =
            ctx.api
                .private_key()
                .partial_sign(&leaf_hash, Stage::Prepare, current_view);
        let vote = PrepareVote(Vote {
            signature,
            id: ctx.api.public_key().nonce,
            leaf_hash,
            current_view,
        });

        Ok(Outcome {
            new_state,
            new_leaf: leaf,
            prepare,
            added_transactions,
            rejected_transactions: Vec::new(),
            vote: Some(vote),
            newest_qc: high_qc,
        })
    }
}
