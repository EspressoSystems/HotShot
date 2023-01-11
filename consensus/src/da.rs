//! Contains the [`ValidatingLeader`] struct used for the leader step in the hotstuff consensus algorithm.

use crate::{utils::ViewInner, CommitmentMap, Consensus, ConsensusApi};
use async_compatibility_layer::{
    art::{async_sleep, async_timeout},
    async_primitives::subscribable_rwlock::{ReadView, SubscribableRwLock},
};
use async_lock::{RwLock, Mutex};
use commit::Committable;
use hotshot_types::{
    certificate::QuorumCertificate,
    data::{DALeaf, DAProposal},
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
use async_compatibility_layer::channel::UnboundedReceiver;
use std::{collections::HashSet, marker::PhantomData, sync::Arc, time::Instant};
use tracing::{error, info, instrument, warn};
use hotshot_types::traits::state::SequencingConsensus;
use hotshot_types::certificate::DACertificate;

/// This view's validating leader
#[derive(Debug, Clone)]
pub struct DALeader<
    A: ConsensusApi<TYPES, DALeaf<TYPES>, DAProposal<TYPES, ELECTION>>,
    TYPES: NodeType,
    ELECTION: Election<TYPES, LeafType = DALeaf<TYPES>>,
> where
    TYPES::StateType: TestableState,
    TYPES::BlockType: TestableBlock,
{
    /// id of node
    pub id: u64,
    /// Reference to consensus. Leader will require a read lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES, DALeaf<TYPES>>>>,
    /// The `high_qc` per spec
    pub high_qc: QuorumCertificate<TYPES, DALeaf<TYPES>>,
    /// The view number we're running on
    pub cur_view: TYPES::Time,
    /// Lock over the transactions list
    pub transactions: Arc<SubscribableRwLock<CommitmentMap<TYPES::Transaction>>>,
    /// Limited access to the consensus protocol
    pub api: A,

    pub vote_collection_chan: Arc<
    Mutex<
        UnboundedReceiver<
            ConsensusMessage<TYPES, DALeaf<TYPES>, DAProposal<TYPES, ELECTION>>,
        >,
    >,
>,
    #[allow(missing_docs)]
    #[allow(clippy::missing_docs_in_private_items)]
    pub _pd: PhantomData<ELECTION>,
}
impl<
        A: ConsensusApi<TYPES, DALeaf<TYPES>, DAProposal<TYPES, ELECTION>>,
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        ELECTION: Election<TYPES, LeafType = DALeaf<TYPES>>,
    > DALeader<A, TYPES, ELECTION>
where
    TYPES::StateType: TestableState,
    TYPES::BlockType: TestableBlock,
{
    async fn wait_for_transactions(&self) -> Vec<TYPES::Transaction> {
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
                        return vec!();
                    }
                    Ok(Ok(_)) => continue,
                }
            }
            return unclaimed_txns;
        }
    } 
    /// Run one view of the DA leader task
    #[instrument(skip(self), fields(id = self.id, view = *self.cur_view), name = "Sequencing DALeader Task", level = "error")]
    pub async fn run_view(self) -> Option<DACertificate<TYPES, DALeaf<TYPES>>> {
        // Prepare teh DA Proposal
        let starting_state = &parent_leaf.state;
        let mut block = starting_state.next_block();
        let txns = wait_for_transactions.await;
        for (_hash, txn) in txns {
            let new_block_check = block.add_transaction_raw(txn);
            if let Ok(new_block) = new_block_check {
                if starting_state.validate_block(&new_block, &self.cur_view) {
                    block = new_block;
                    continue;
                }
            } 
        }
        if let Ok(new_state) = starting_state.append(&block, &self.cur_view) {
            let leaf = DALeaf {
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
            let leaf: DAProposal<TYPES, ELECTION> = leaf.into();
            let message = ConsensusMessage::<
                TYPES,
                ValidatingLeaf<TYPES>,
                ValidatingProposal<TYPES, ELECTION>,
            >::Proposal { proposal, signature };
            // Brodcast DA proposal
            if let Err(e) = self.api.send_broadcast_message(message.clone()).await {
                consensus.metrics.failed_to_send_messages.add(1);
                warn!(?message, ?e, "Could not broadcast leader proposal");
            } else {
                consensus.metrics.outgoing_broadcast_messages.add(1);
            }
        } else {
            error!("Could not append state in high qc for proposal. Failed to send out proposal.");
        }

        // Wait for DA votes or Timeout
        let lock = self.vote_collection_chan.lock().await;
        while let Ok(msg) = lock.recv().await {
            if msg.view_number() != self.cur_view {
                continue;
            }
            match msg {
                ConsensusMessage::TimedOut(t) => {
                    qcs.insert(t.justify_qc);
                }
                ConsensusMessage::Vote(vote) => {
                    // if the signature on the vote is invalid,
                    // assume it's sent by byzantine node
                    // and ignore

                    if !self.api.is_valid_signature(
                        &vote.signature.0,
                        &vote.signature.1,
                        vote.leaf_commitment,
                        vote.current_view,
                        // Ignoring deserialization errors below since we are getting rid of it soon
                        Unchecked(vote.vote_token.clone()),
                    ) {
                        continue;
                    }

                    let (_bh, map) = vote_outcomes
                        .entry(vote.leaf_commitment)
                        .or_insert_with(|| (vote.block_commitment, BTreeMap::new()));
                    map.insert(
                        vote.signature.0.clone(),
                        (vote.signature.1.clone(), vote.vote_token.clone()),
                    );

                    stake_casted += u64::from(vote.vote_token.vote_count());

                    if stake_casted >= u64::from(threshold) {
                        let (_block_commitment, valid_signatures) =
                            vote_outcomes.remove(&vote.leaf_commitment).unwrap();

                        // construct QC
                        let qc = QuorumCertificate {
                            leaf_commitment: vote.leaf_commitment,
                            view_number: self.cur_view,
                            signatures: valid_signatures,
                            genesis: false,
                        };
                        return qc;
                    }
                }
                ConsensusMessage::NextViewInterrupt(_view_number) => {
                    warn!("DA leader received next view interupt");
                }
                ConsensusMessage::Proposal(_p) => {
                    warn!("The DA leader has received an unexpected proposal!");
                }
            }
        }
        None
    }
}
