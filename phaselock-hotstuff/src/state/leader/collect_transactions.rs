use super::Ctx;
use crate::{ConsensusApi, ConsensusMessage, Result};
use phaselock_types::{
    data::{Leaf, QuorumCertificate, Stage},
    error::{FailedToBroadcastSnafu, PhaseLockError, StorageSnafu},
    message::{Prepare, Vote},
    traits::{
        node_implementation::{NodeImplementation, TypeMap},
        storage::Storage,
        BlockContents, State,
    },
};
use snafu::ResultExt;
use tracing::{debug, error, instrument, warn};

#[derive(Debug, Clone)]
pub struct CollectTransactions<I: NodeImplementation<N>, const N: usize> {
    ctx: Ctx<I, N>,
    high_qc: QuorumCertificate<N>,
    transactions: Vec<<<I as NodeImplementation<N>>::Block as BlockContents<N>>::Transaction>,
}

pub(super) enum CollectTransactionsTransition<I: NodeImplementation<N>, const N: usize> {
    Stay,
    PreCommit { ctx: Ctx<I, N>, vote: Vote<N> },
}

impl<I: NodeImplementation<N>, const N: usize> CollectTransactions<I, N> {
    pub(super) fn new(ctx: Ctx<I, N>, high_qc: QuorumCertificate<N>) -> Self {
        Self {
            ctx,
            high_qc,
            transactions: Vec::new(),
        }
    }

    #[instrument(skip(api))]
    pub(super) async fn step(
        &mut self,
        msg: ConsensusMessage<'_, I, N>,
        api: &mut impl ConsensusApi<I, N>,
    ) -> Result<CollectTransactionsTransition<I, N>> {
        match msg {
            ConsensusMessage::Idle { transactions } => {
                self.add_transactions(std::mem::take(transactions), api)
                    .await
            }
            msg => {
                warn!(?msg, "dropping message");
                Ok(CollectTransactionsTransition::Stay)
            }
        }
    }

    async fn add_transactions(
        &mut self,
        transactions: Vec<<I as TypeMap<N>>::Transaction>,
        api: &mut impl ConsensusApi<I, N>,
    ) -> Result<CollectTransactionsTransition<I, N>> {
        // try to take all transactions and add them to `self.block` and `self.transactions`.
        // This is an implementation detail that HotStuff does not document.
        for transaction in transactions {
            let new_block = self.ctx.block.add_transaction_raw(&transaction);
            match new_block {
                Ok(new_block) => {
                    if self.ctx.state.validate_block(&new_block) {
                        self.ctx.block = new_block;
                        debug!(?transaction, "Added transaction to block");
                        self.transactions.push(transaction);
                    } else {
                        // TODO: This could change the `state` internals as it tries to append this block
                        // It might be better if `validate_block` returned `Result<(), Error>`
                        // let err = self.state.append(&new_block).unwrap_err();
                        warn!(?transaction, "Invalid transaction rejected");
                    }
                }
                Err(e) => warn!(?e, ?transaction, "Invalid transaction rejected"),
            }
        }
        // if we have no transactions, wait for incoming transactions.
        // TODO: This means that the leader could be stuck in this state forever.
        if self.transactions.is_empty() {
            return Ok(CollectTransactionsTransition::Stay);
        }
        // Back to HotStuff

        // The leader uses the createLeaf method to extend the tail of highQC .node with a new proposal [1]. The method
        // creates a new leaf node as a child and embeds a digest of the parent in the child node. The leader then sends the new
        // node in a prepare message to all other replicas[2]. The proposal carries highQC for safety justification.

        let new_leaf = Leaf::new(self.ctx.block.clone(), self.high_qc.leaf_hash);
        let the_hash = new_leaf.hash();
        let new_state = self.ctx.state.append(&new_leaf.item).map_err(|error| {
            error!(?error, "Failed to append block to existing state");
            PhaseLockError::InconsistentBlock {
                stage: Stage::Prepare,
            }
        })?;
        api.storage()
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

        // Broadcast out the leaf
        let network_result = api
            .send_broadcast_message(phaselock_types::message::ConsensusMessage::Prepare(
                Prepare {
                    current_view: self.ctx.current_view,
                    leaf: new_leaf.clone(),
                    high_qc: self.high_qc.clone(),
                    state: new_state.clone(),
                },
            ))
            .await
            .context(FailedToBroadcastSnafu {
                stage: Stage::Prepare,
            });
        if let Err(e) = network_result {
            warn!(?e, "Error broadcasting leaf");
        }
        // Notify our listeners
        api.send_propose(self.ctx.current_view, &self.ctx.block)
            .await;

        // Make a prepare signature and send it to ourselves
        let signature =
            api.private_key()
                .partial_sign(&the_hash, Stage::Prepare, self.ctx.current_view);
        let vote = Vote {
            signature,
            leaf_hash: the_hash,
            id: api.public_key().nonce,
            current_view: self.ctx.current_view,
            stage: Stage::Prepare,
        };
        Ok(CollectTransactionsTransition::PreCommit {
            ctx: self.ctx.clone(),
            vote,
        })
    }
}
