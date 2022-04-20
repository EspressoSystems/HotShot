use crate::{
    phase::{err, precommit::PreCommitPhase, Phase, Progress, UpdateCtx},
    ConsensusApi, Result, TransactionLink, TransactionState, ViewNumber,
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
    created_on: Instant,
}

impl<const N: usize> PrepareLeader<N> {
    pub(super) fn new() -> Self {
        Self {
            high_qc: None,
            created_on: Instant::now(),
        }
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<PreCommitPhase<I, N>>> {
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
            }
        }

        if self.created_on.elapsed() < ctx.api.propose_min_round_time() {
            return Ok(Progress::NotReady);
        }

        // if we have no transactions and we're not forced to start a round (by `propose_max_round_time`)
        // return now
        if ctx.get_unclaimed_transactions_mut().count() == 0
            && self.created_on.elapsed() < ctx.api.propose_max_round_time()
        {
            return Ok(Progress::NotReady);
        }

        // we need to propose a round now
        let prepare = self.propose_round(ctx).await?;
        Ok(Progress::Next(prepare))
    }

    async fn propose_round<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<PreCommitPhase<I, N>> {
        let high_qc = match self.high_qc.take() {
            Some(high_qc) => high_qc,
            None => return err("in propose_round: no high_qc set"),
        };
        let leaf = ctx.get_leaf_by_block(&high_qc.block_hash).await?;
        let leaf_hash = leaf.hash();
        let state = ctx.get_state_by_leaf(&leaf_hash).await?;
        trace!(?state, ?leaf_hash);
        let mut block = state.next_block();
        let view_number = ctx.view_number;

        // try to append unclaimed transactions to the block
        let _transaction_ids = append_transactions(
            ctx.get_unclaimed_transactions_mut(),
            &mut block,
            &state,
            view_number,
        );

        // Create new leaf and add it to the store
        let new_leaf = Leaf::new(block.clone(), high_qc.leaf_hash);
        let the_hash = new_leaf.hash();
        let new_state = state.append(&new_leaf.item).map_err(|error| {
            error!(?error, "Failed to append block to existing state");
            PhaseLockError::InconsistentBlock {
                stage: Stage::Prepare,
            }
        })?;
        ctx.api
            .storage()
            .update(|mut m| {
                let new_leaf = new_leaf.clone();
                let new_state = new_state.clone();
                async move {
                    m.insert_leaf(new_leaf).await?;
                    m.insert_state(new_state, the_hash).await?;
                    Ok(())
                }
            })
            .await
            .context(StorageSnafu)?;

        debug!(?new_leaf, ?the_hash, "Leaf created and added to store");
        debug!(?new_state, "New state inserted");

        let current_view = ctx.view_number.0;
        let prepare = Prepare {
            current_view,
            leaf: new_leaf.clone(),
            high_qc: high_qc.clone(),
            state: new_state.clone(),
        };
        // Broadcast out the leaf
        let network_result = ctx
            .api
            .send_broadcast_message(ConsensusMessage::Prepare(prepare.clone()))
            .await
            .context(FailedToBroadcastSnafu {
                stage: Stage::Prepare,
            });
        if let Err(e) = network_result {
            warn!(?e, "Error broadcasting leaf");
        }
        // Notify our listeners
        ctx.api.send_propose(current_view, &block).await;

        // if the leader can vote like a replica, cast this vote now
        let mut vote = None;
        let next_leader = ctx.api.get_leader(current_view, Stage::PreCommit).await;
        let is_leader_next_phase = ctx.api.public_key() == &next_leader;
        if ctx.api.leader_acts_as_replica() {
            let signature =
                ctx.api
                    .private_key()
                    .partial_sign(&the_hash, Stage::Prepare, current_view);
            let new_vote = PrepareVote(Vote {
                signature,
                leaf_hash: the_hash,
                id: ctx.api.public_key().nonce,
                current_view,
                stage: Stage::Prepare,
            });
            vote = Some(new_vote.clone());
            ctx.api
                .send_direct_message(next_leader.clone(), ConsensusMessage::PrepareVote(new_vote))
                .await
                .context(FailedToMessageLeaderSnafu {
                    stage: Stage::Prepare,
                })?;
        }

        Ok(if is_leader_next_phase {
            PreCommitPhase::leader(Some(prepare), vote)
        } else {
            PreCommitPhase::replica(Some(prepare))
        })
    }
}

fn append_transactions<'a, I: NodeImplementation<N>, const N: usize>(
    transactions: impl Iterator<Item = &'a mut TransactionState<I, N>> + 'a,
    block: &mut I::Block,
    state: &I::State,
    view_number: ViewNumber,
) -> Vec<TransactionHash<N>> {
    let mut added_transactions = Vec::new();
    for transaction in transactions {
        let tx = &transaction.transaction;
        // Make sure the transaction is valid given the current state,
        // otherwise, discard it
        let new_block = block.add_transaction_raw(tx);
        match new_block {
            Ok(new_block) => {
                if state.validate_block(&new_block) {
                    *block = new_block;
                    debug!(?tx, "Added transaction to block");
                    added_transactions.push(transaction);
                } else {
                    // TODO: `state.append` could change our state.
                    // we should probably make `validate_block` return this error.
                    let err = state.append(&new_block).unwrap_err();
                    warn!(?tx, ?err, "Invalid transaction rejected");
                }
            }
            Err(e) => warn!(?e, ?tx, "Invalid transaction rejected"),
        }
    }
    let transaction_ids: Vec<TransactionHash<N>> = added_transactions
        .iter()
        .map(|t| t.transaction.hash())
        .collect();
    for transaction in added_transactions {
        // Mark these transactions as added
        transaction.propose = Some(TransactionLink {
            timestamp: Instant::now(),
            view_number,
        });
    }
    transaction_ids
}
