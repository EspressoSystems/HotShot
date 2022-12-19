//! Contains the [`ValidatingLeader`] struct used for the leader step in the hotstuff consensus algorithm.

use crate::{utils::ViewInner, CommitmentMap, Consensus, ConsensusApi};
use async_compatibility_layer::{
    art::{async_sleep, async_timeout},
    async_primitives::subscribable_rwlock::{ReadView, SubscribableRwLock},
};
use async_lock::RwLock;
use commit::Committable;
use hotshot_types::{
    certificate::QuorumCertificate,
    data::{ValidatingLeaf, ValidatingProposal},
    message::{ConsensusMessage, Proposal},
    traits::{
        election::Election,
        node_implementation::NodeType,
        signature_key::SignatureKey,
        state::ValidatingConsensus,
        state::{TestableBlock, TestableState},
        Block, State,
    },
};
use std::{collections::HashSet, marker::PhantomData, sync::Arc, time::Instant};
use tracing::{error, info, instrument, warn};

/// This view's validating leader
#[derive(Debug, Clone)]
pub struct ValidatingLeader<
    A: ConsensusApi<TYPES, ValidatingLeaf<TYPES>, ValidatingProposal<TYPES, ELECTION>>,
    TYPES: NodeType,
    ELECTION: Election<TYPES, LeafType = ValidatingLeaf<TYPES>>,
> where
    TYPES::StateType: TestableState,
    TYPES::BlockType: TestableBlock,
{
    /// id of node
    pub id: u64,
    /// Reference to consensus. Validating leader will require a read lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES, ValidatingLeaf<TYPES>>>>,
    /// The `high_qc` per spec
    pub high_qc: QuorumCertificate<TYPES, ValidatingLeaf<TYPES>>,
    /// The view number we're running on
    pub cur_view: TYPES::Time,
    /// Lock over the transactions list
    pub transactions: Arc<SubscribableRwLock<CommitmentMap<TYPES::Transaction>>>,
    /// Limited access to the consensus protocol
    pub api: A,

    #[allow(missing_docs)]
    #[allow(clippy::missing_docs_in_private_items)]
    pub _pd: PhantomData<ELECTION>,
}

impl<
        A: ConsensusApi<TYPES, ValidatingLeaf<TYPES>, ValidatingProposal<TYPES, ELECTION>>,
        TYPES: NodeType<ConsensusType = ValidatingConsensus>,
        ELECTION: Election<TYPES, LeafType = ValidatingLeaf<TYPES>>,
    > ValidatingLeader<A, TYPES, ELECTION>
where
    TYPES::StateType: TestableState,
    TYPES::BlockType: TestableBlock,
{
    /// Run one view of the leader task
    #[instrument(skip(self), fields(id = self.id, view = *self.cur_view), name = "Validating ValidatingLeader Task", level = "error")]
    pub async fn run_view(self) -> QuorumCertificate<TYPES, ValidatingLeaf<TYPES>> {
        let pk = self.api.public_key();
        error!("Validating leader task started!");

        let task_start_time = Instant::now();
        let parent_view_number = &self.high_qc.view_number;
        let consensus = self.consensus.read().await;
        let mut reached_decided = false;

        let parent_leaf = if let Some(parent_view) = consensus.state_map.get(parent_view_number) {
            match &parent_view.view_inner {
                ViewInner::Leaf { leaf } => {
                    if let Some(leaf) = consensus.saved_leaves.get(leaf) {
                        if leaf.view_number == consensus.last_decided_view {
                            reached_decided = true;
                        }
                        leaf
                    } else {
                        warn!("Failed to find high QC parent.");
                        return self.high_qc;
                    }
                }
                // can happen if future api is whacked
                ViewInner::Failed => {
                    warn!("Parent of high QC points to a failed QC");
                    return self.high_qc;
                }
            }
        } else {
            warn!("Couldn't find high QC parent in state map.");
            return self.high_qc;
        };

        let original_parent_hash = parent_leaf.commit();
        let starting_state = &parent_leaf.state;

        let mut previous_used_txns_vec = parent_leaf.deltas.contained_transactions();

        let mut next_parent_hash = original_parent_hash;

        if !reached_decided {
            while let Some(next_parent_leaf) = consensus.saved_leaves.get(&next_parent_hash) {
                if next_parent_leaf.view_number <= consensus.last_decided_view {
                    break;
                }
                let next_parent_txns = next_parent_leaf.deltas.contained_transactions();
                for next_parent_txn in next_parent_txns {
                    previous_used_txns_vec.insert(next_parent_txn);
                }
                next_parent_hash = next_parent_leaf.parent_commitment;
            }
            // TODO do some sort of sanity check on the view number that it matches decided
        }

        let previous_used_txns = previous_used_txns_vec.into_iter().collect::<HashSet<_>>();

        let passed_time = task_start_time - Instant::now();
        async_sleep(self.api.propose_min_round_time() - passed_time).await;

        let receiver = self.transactions.subscribe().await;
        let mut block = starting_state.next_block();

        // Wait until we have min_transactions for the block or we hit propose_max_round_time
        while task_start_time.elapsed() < self.api.propose_max_round_time() {
            let txns = self.transactions.cloned().await;
            let unclaimed_txns: Vec<_> = txns
                .iter()
                .filter(|(txn_hash, _txn)| !previous_used_txns.contains(txn_hash))
                .collect();

            let time_past = task_start_time.elapsed();
            if unclaimed_txns.len() < self.api.min_transactions()
                && (time_past < self.api.propose_max_round_time())
            {
                let duration = self.api.propose_max_round_time() - time_past;
                let result = async_timeout(duration, receiver.recv()).await;
                match result {
                    Err(_) => {
                        // Fall through below to updating new block
                        info!("propose_max_round_time passed, sending transactions we have so far");
                    }
                    Ok(Err(e)) => {
                        // Something unprecedented is wrong, and `transactions` has been dropped
                        error!("Channel receiver error for SubscribableRwLock {:?}", e);
                        return self.high_qc;
                    }
                    Ok(Ok(_)) => continue,
                }
            }

            // Add unclaimed transactions to the new block
            for (_txn_hash, txn) in &unclaimed_txns {
                let new_block_check = block.add_transaction_raw(txn);
                if let Ok(new_block) = new_block_check {
                    if starting_state.validate_block(&new_block, &self.cur_view) {
                        block = new_block;
                        continue;
                    }
                }
            }
            break;
        }

        consensus
            .metrics
            .proposal_wait_duration
            .add_point(task_start_time.elapsed().as_secs_f64());

        let proposal_build_start = Instant::now();

        if let Ok(new_state) = starting_state.append(&block, &self.cur_view) {
            let leaf = ValidatingLeaf {
                view_number: self.cur_view,
                height: parent_leaf.height + 1,
                justify_qc: self.high_qc.clone(),
                parent_commitment: original_parent_hash,
                deltas: block,
                state: new_state,
                rejected: Vec::new(),
                timestamp: time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
                proposer_id: pk.to_bytes(),
            };
            let signature = self.api.sign_proposal(&leaf.commit(), self.cur_view);
            let leaf: ValidatingProposal<TYPES, ELECTION> = leaf.into();
            let message = ConsensusMessage::<
                TYPES,
                ValidatingLeaf<TYPES>,
                ValidatingProposal<TYPES, ELECTION>,
            >::Proposal(Proposal { leaf, signature });
            consensus
                .metrics
                .proposal_build_duration
                .add_point(proposal_build_start.elapsed().as_secs_f64());
            info!("Sending out proposal {:?}", message);

            if let Err(e) = self.api.send_broadcast_message(message.clone()).await {
                consensus.metrics.failed_to_send_messages.add(1);
                warn!(?message, ?e, "Could not broadcast leader proposal");
            } else {
                consensus.metrics.outgoing_broadcast_messages.add(1);
            }
        } else {
            error!("Could not append state in high qc for proposal. Failed to send out proposal.");
        }

        self.high_qc.clone()
    }
}
