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
use std::{collections::HashSet, marker::PhantomData, sync::Arc, time::Instant};
use tracing::{error, info, instrument, warn};

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
    async fn wait_for_transactions() -> Vec<TYPES::Transaction> {
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
                        return ;
                    }
                    Ok(Ok(_)) => continue,
                }
            }
        }
    } 
    /// Run one view of the DA leader task
    #[instrument(skip(self), fields(id = self.id, view = *self.cur_view), name = "Sequencing DALeader Task", level = "error")]
    pub async fn run_view(self) -> DACertificate<TYPES, DALeaf<TYPES>> {
        // Prepare teh DA Proposal
        // Brodcast DA proposal
        // Wait for DA votes or Timeout
        // 
    }
}
