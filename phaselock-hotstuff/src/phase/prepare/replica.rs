//! Prepare replica implementation

use super::Outcome;
use crate::{phase::UpdateCtx, utils, ConsensusApi, Result, TransactionState};
use phaselock_types::{
    data::{LeafHash, Stage},
    error::PhaseLockError,
    message::{Prepare, PrepareVote, Vote},
    traits::{node_implementation::NodeImplementation, State},
};
use tracing::error;

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
        if let Some(validation_result) = self.validate_prepare(prepare, ctx).await? {
            let outcome = self.vote(ctx, validation_result).await?;
            Ok(Some(outcome))
        } else {
            Ok(None)
        }
    }

    /// Validate an incoming prepare. If this validation succeeds, `Some(ValidationResult)` is returned. This can then be used for [`Replica::vote`].
    ///
    /// If `Ok(None)` is returned, then the incoming `prepare` has a transaction we don't have, and we cannot vote on this prepare round.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - the underlying `Storage` returns an error.
    /// - The `state` could not be reconstructed with the given information
    async fn validate_prepare<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
        &self,
        prepare: &Prepare<I::Block, I::State, N>,
        ctx: &UpdateCtx<'_, I, A, N>,
    ) -> Result<Option<ValidationResult<I, N>>> {
        let leaf_hash = prepare.leaf.hash();

        let state = ctx.get_state_by_leaf(&prepare.leaf.parent).await?;
        let self_highest_qc = match ctx.get_newest_qc().await? {
            Some(qc) => qc,
            None => return utils::err("No QC in storage"),
        };
        let is_safe_node = utils::validate_against_locked_qc(
            ctx.api,
            &self_highest_qc,
            &prepare.leaf,
            &prepare.high_qc,
        )
        .await;
        if !is_safe_node || !state.validate_block(&prepare.leaf.item) {
            error!("is_safe_node: {}", is_safe_node);
            error!(?prepare.leaf, "Leaf failed safe_node predicate");
            return Err(PhaseLockError::BadBlock {
                stage: Stage::Prepare,
            });
        }

        // Add resulting state to storage
        let new_state = state.append(&prepare.leaf.item).map_err(|error| {
            error!(?error, "Failed to append block to existing state");
            PhaseLockError::InconsistentBlock {
                stage: Stage::Prepare,
            }
        })?;

        if prepare.state != new_state {
            // the state that the leader send does not match with what we calculated
            error!(
                ?new_state,
                ?prepare.state,
                "Suggested state does not match actual state."
            );
            return Err(PhaseLockError::InconsistentBlock {
                stage: Stage::Prepare,
            });
        }

        let added_transactions = Vec::new();
        // TODO: rework how transactions are stored and processed
        // https://github.com/EspressoSystems/phaselock/issues/136
        //
        // for id in prepare.state.new_transaction_ids(&state) {
        //     if let Some(transaction_state) =
        //         ctx.transactions.iter().find(|t| t.transaction.id() == id)
        //     {
        //         added_transactions.push(transaction_state.clone());
        //     } else {
        //         info!(
        //             ?id,
        //             "Could not find transaction to mark, will sit out until we receive it"
        //         );
        //         return Ok(None);
        //     }
        // }

        Ok(Some(ValidationResult {
            added_transactions,
            prepare: prepare.clone(),
            new_state,
            leaf_hash,
        }))
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
        validation_result: ValidationResult<I, N>,
    ) -> Result<Outcome<I, N>> {
        let ValidationResult {
            added_transactions,
            prepare,
            leaf_hash,
            new_state,
        } = validation_result;

        let signature = ctx
            .api
            .sign_vote(&leaf_hash, Stage::Prepare, ctx.view_number);
        let vote = PrepareVote(Vote {
            signature,
            leaf_hash,
            current_view: ctx.view_number,
        });

        let new_leaf = prepare.leaf.clone();
        let newest_qc = prepare.high_qc.clone();

        Ok(Outcome {
            new_state,
            new_leaf,
            prepare,
            added_transactions,
            rejected_transactions: Vec::new(),
            vote: Some(vote),
            newest_qc,
        })
    }
}

/// A result from [`Replica::validate`] that contains the nessecary information to vote.
struct ValidationResult<I: NodeImplementation<N>, const N: usize> {
    /// The transactions that are being added in this vote.
    added_transactions: Vec<TransactionState<I, N>>,
    /// The original prepare message.
    prepare: Prepare<I::Block, I::State, N>,
    /// The leaf hash that was calculated for `prepare.leaf`
    leaf_hash: LeafHash<N>,
    /// The new state, reconstruted from our own state with the given data in `prepare`
    new_state: I::State,
}
