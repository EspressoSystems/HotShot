//! Contains the [`DALeader`] struct used for the leader step in the consensus algorithm with DA
//! committee.

use crate::{utils::ViewInner, CommitmentMap, Consensus, ConsensusApi};
use async_compatibility_layer::channel::UnboundedReceiver;
use async_compatibility_layer::{
    art::async_timeout,
    async_primitives::subscribable_rwlock::{ReadView, SubscribableRwLock},
};
use async_lock::{Mutex, RwLock};
use commit::Commitment;
use commit::Committable;
use either::Either;
use either::Right;
use hotshot_types::certificate::DACertificate;
use hotshot_types::data::CommitmentProposal;
use hotshot_types::message::{ProcessedConsensusMessage, Vote};
use hotshot_types::traits::state::SequencingConsensus;
use hotshot_types::{
    certificate::QuorumCertificate,
    data::{DALeaf, DAProposal},
    message::{ConsensusMessage, Proposal},
    traits::{
        election::{Checked::Unchecked, Election, SignedCertificate, VoteData, VoteToken},
        node_implementation::NodeType,
        signature_key::SignatureKey,
        Block,
    },
};
use std::num::NonZeroU64;
use std::{
    collections::BTreeMap, collections::HashSet, marker::PhantomData, sync::Arc, time::Instant,
};
use tracing::{error, info, instrument, warn};

/// This view's DA committee leader
#[derive(Debug, Clone)]
pub struct DALeader<
    A: ConsensusApi<TYPES, DALeaf<TYPES>, DAProposal<TYPES, ELECTION>>,
    TYPES: NodeType,
    ELECTION: Election<TYPES, LeafType = DALeaf<TYPES>>,
> {
    /// id of node
    pub id: u64,
    /// Reference to consensus. Leader will require a read lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES, DALeaf<TYPES>>>>,
    /// The `high_qc` per spec
    pub high_qc: ELECTION::QuorumCertificate,
    /// The view number we're running on
    pub cur_view: TYPES::Time,
    /// Lock over the transactions list
    pub transactions: Arc<SubscribableRwLock<CommitmentMap<TYPES::Transaction>>>,
    /// Limited access to the consensus protocol
    pub api: A,
    /// channel through which the leader collects votes
    #[allow(clippy::type_complexity)]
    pub vote_collection_chan: Arc<
        Mutex<
            UnboundedReceiver<
                ProcessedConsensusMessage<TYPES, DALeaf<TYPES>, DAProposal<TYPES, ELECTION>>,
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
{
    /// Accumulate votes for a proposal and return either the cert or None if the threshold was not reached in time
    /// TODO: Refactor this to use new `Elecetion` trait and call accumulate
    async fn wait_for_votes(
        &self,
        cur_view: TYPES::Time,
        threshold: NonZeroU64,
        block_commitment: Commitment<<TYPES as NodeType>::BlockType>,
    ) -> Option<DACertificate<TYPES>> {
        let lock = self.vote_collection_chan.lock().await;
        let mut valid_signatures = BTreeMap::new();
        let mut stake_casted = 0;

        while let Ok(msg) = lock.recv().await {
            if Into::<ConsensusMessage<_, _, _>>::into(msg.clone()).view_number() != cur_view {
                continue;
            }
            match msg {
                ProcessedConsensusMessage::Vote(vote_message, sender) => {
                    match vote_message {
                        Vote::DA(vote) => {
                            if vote.signature.0
                                != <TYPES::SignatureKey as SignatureKey>::to_bytes(&sender)
                            {
                                continue;
                            }

                            if !self.api.is_valid_vote(
                                &vote.signature.0,
                                &vote.signature.1,
                                VoteData::DA(block_commitment),
                                self.cur_view,
                                // Ignoring deserialization errors below since we are getting rid of it soon
                                Unchecked(vote.vote_token.clone()),
                            ) {
                                continue;
                            }

                            valid_signatures.insert(
                                vote.signature.0.clone(),
                                (vote.signature.1.clone(), vote.vote_token.clone()),
                            );

                            stake_casted += u64::from(vote.vote_token.vote_count());

                            if stake_casted >= u64::from(threshold) {
                                // construct QC
                                let qc = DACertificate {
                                    view_number: self.cur_view,
                                    signatures: valid_signatures,
                                };
                                return Some(qc);
                            }
                        }
                        _ => {
                            warn!("The DA leader has received an unexpected vote!");
                        }
                    }
                }
                ProcessedConsensusMessage::NextViewInterrupt(_view_number) => {
                    self.api.send_next_leader_timeout(self.cur_view).await;
                    break;
                }
                ProcessedConsensusMessage::Proposal(_p, _sender) => {
                    warn!("The next leader has received an unexpected proposal!");
                }
            }
        }
        None
    }
    /// Returns the parent leaf of the proposal we are building
    async fn parent_leaf(&self) -> Option<DALeaf<TYPES>> {
        let parent_view_number = &self.high_qc.view_number();
        let consensus = self.consensus.read().await;
        let parent_leaf = if let Some(parent_view) = consensus.state_map.get(parent_view_number) {
            match &parent_view.view_inner {
                ViewInner::Leaf { leaf } => {
                    if let Some(leaf) = consensus.saved_leaves.get(leaf) {
                        leaf
                    } else {
                        warn!("Failed to find high QC parent.");
                        return None;
                    }
                }
                ViewInner::Failed => {
                    warn!("Parent of high QC points to a failed QC");
                    return None;
                }
            }
        } else {
            warn!("Couldn't find high QC parent in state map.");
            return None;
        };
        Some(parent_leaf.clone())
    }
    /// return None if we can't get transactions
    async fn wait_for_transactions(&self) -> Option<Vec<TYPES::Transaction>> {
        let task_start_time = Instant::now();

        let parent_leaf = self.parent_leaf().await?;
        let previous_used_txns = match parent_leaf.deltas {
            Either::Left(block) => block.contained_transactions(),
            Either::Right(_commitment) => HashSet::new(),
        };
        let receiver = self.transactions.subscribe().await;

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
                        return None;
                    }
                    Ok(Ok(_)) => continue,
                }
            }
            let mut txns = vec![];
            for (_hash, txn) in unclaimed_txns {
                txns.push(txn.clone());
            }
            return Some(txns);
        }
        None
    }
    /// Run one view of the DA leader task
    #[instrument(skip(self), fields(id = self.id, view = *self.cur_view), name = "Sequencing DALeader Task", level = "error")]
    pub async fn run_view(self) -> Option<(DACertificate<TYPES>, TYPES::BlockType, DALeaf<TYPES>)> {
        // Prepare the DA Proposal
        let Some(parent_leaf) = self.parent_leaf().await else {
             warn!("Couldn't find high QC parent in state map.");
             return None;
         };

        let mut block = TYPES::BlockType::new();
        let txns = self.wait_for_transactions().await?;

        for txn in txns {
            if let Ok(new_block) = block.add_transaction_raw(&txn) {
                block = new_block;
                continue;
            }
        }
        let block_commitment = block.commit();

        let consensus = self.consensus.read().await;
        let signature = self.api.sign_da_proposal(&block.commit());
        let data: DAProposal<TYPES, ELECTION> = DAProposal {
            deltas: block.clone(),
            view_number: self.cur_view,
            _pd: PhantomData,
        };
        let message =
            ConsensusMessage::<TYPES, DALeaf<TYPES>, DAProposal<TYPES, ELECTION>>::Proposal(
                Proposal { data, signature },
            );
        // Brodcast DA proposal
        if let Err(e) = self.api.send_broadcast_message(message.clone()).await {
            consensus.metrics.failed_to_send_messages.add(1);
            warn!(?message, ?e, "Could not broadcast leader proposal");
        } else {
            consensus.metrics.outgoing_broadcast_messages.add(1);
        }

        // Wait for DA votes or Timeout
        if let Some(cert) = self
            .wait_for_votes(self.cur_view, self.api.threshold(), block_commitment)
            .await
        {
            return Some((cert, block, parent_leaf));
        }
        None
    }
}

/// Implemenation of the consensus leader for a DA/Sequencing consensus.  Handles sending out a proposal to the entire network
/// For now this step happens after the `DALeader` completes it's proposal and collects enough votes.
pub struct DAConsensusLeader<
    A: ConsensusApi<TYPES, DALeaf<TYPES>, DAProposal<TYPES, ELECTION>>,
    TYPES: NodeType,
    ELECTION: Election<
        TYPES,
        LeafType = DALeaf<TYPES>,
        QuorumCertificate = QuorumCertificate<TYPES, DALeaf<TYPES>>,
        DACertificate = DACertificate<TYPES>,
    >,
> {
    /// id of node
    pub id: u64,
    /// Reference to consensus. Leader will require a read lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES, DALeaf<TYPES>>>>,
    /// The `high_qc` per spec
    pub high_qc: QuorumCertificate<TYPES, DALeaf<TYPES>>,
    /// The view number we're running on
    pub cur_view: TYPES::Time,
    /// The Certificate generated for the transactions commited to in the proposal the leader will build
    pub cert: DACertificate<TYPES>,
    /// The block corresponding to the DA cert
    pub block: TYPES::BlockType,
    /// Leaf this proposal will chain from
    pub parent: DALeaf<TYPES>,
    /// Limited access to the consensus protocol
    pub api: A,

    #[allow(missing_docs)]
    #[allow(clippy::missing_docs_in_private_items)]
    pub _pd: PhantomData<ELECTION>,
}
impl<
        A: ConsensusApi<TYPES, DALeaf<TYPES>, DAProposal<TYPES, ELECTION>>,
        TYPES: NodeType<ConsensusType = SequencingConsensus, ApplicationMetadataType = ()>,
        ELECTION: Election<
            TYPES,
            LeafType = DALeaf<TYPES>,
            QuorumCertificate = QuorumCertificate<TYPES, DALeaf<TYPES>>,
            DACertificate = DACertificate<TYPES>,
        >,
    > DAConsensusLeader<A, TYPES, ELECTION>
{
    /// Run one view of the DA leader task
    #[instrument(skip(self), fields(id = self.id, view = *self.cur_view), name = "Sequencing DALeader Task", level = "error")]
    pub async fn run_view(self) -> Option<QuorumCertificate<TYPES, DALeaf<TYPES>>> {
        let block_commitment = self.block.commit();
        let leaf = DALeaf {
            view_number: self.cur_view,
            height: self.parent.height + 1,
            justify_qc: self.high_qc.clone(),
            parent_commitment: self.parent.commit(),
            // Use the block commitment rather than the block, so that the replica can construct
            // the same leaf with the commitment.
            deltas: Right(block_commitment),
            rejected: vec![],
            timestamp: time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            proposer_id: self.api.public_key().to_bytes(),
        };
        let signature = self
            .api
            .sign_validating_or_commitment_proposal(&leaf.commit());
        // TODO: DA cert is sent as part of the proposal here, we should split this out so we don't have to wait for it.
        let proposal = CommitmentProposal {
            block_commitment,
            view_number: leaf.view_number,
            justify_qc: self.high_qc.clone(),
            dac: self.cert,
            proposer_id: leaf.proposer_id,
            application_metadata: {},
        };

        let message =
            ConsensusMessage::<TYPES, DALeaf<TYPES>, CommitmentProposal<TYPES, ELECTION>>::Proposal(
                Proposal {
                    data: proposal,
                    signature,
                },
            );
        if let Err(e) = self.api.send_da_broadcast(message.clone()).await {
            warn!(?message, ?e, "Could not broadcast leader proposal");
            return None;
        }
        Some(self.high_qc)
    }
}

/// Implenting the next leader.  Collect votes on the previous leaders proposal and return the QC
pub struct DANextLeader<
    A: ConsensusApi<TYPES, DALeaf<TYPES>, DAProposal<TYPES, ELECTION>>,
    TYPES: NodeType,
    ELECTION: Election<TYPES, LeafType = DALeaf<TYPES>>,
> {
    /// id of node
    pub id: u64,
    /// Reference to consensus. Leader will require a read lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES, DALeaf<TYPES>>>>,
    /// The view number we're running on
    pub cur_view: TYPES::Time,
    /// Limited access to the consensus protocol
    pub api: A,
    /// generic_qc before starting this
    pub generic_qc: QuorumCertificate<TYPES, DALeaf<TYPES>>,
    /// channel through which the leader collects votes
    #[allow(clippy::type_complexity)]
    pub vote_collection_chan: Arc<
        Mutex<
            UnboundedReceiver<
                ProcessedConsensusMessage<TYPES, DALeaf<TYPES>, DAProposal<TYPES, ELECTION>>,
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
    > DANextLeader<A, TYPES, ELECTION>
{
    /// Run one view of the next leader, collect votes and build a QC for the last views `CommitmentProposal`
    /// # Panics
    /// While we are unwrapping, this function can logically never panic
    /// unless there is a bug in std
    pub async fn run_view(self) -> QuorumCertificate<TYPES, DALeaf<TYPES>> {
        let mut qcs = HashSet::<QuorumCertificate<TYPES, DALeaf<TYPES>>>::new();
        qcs.insert(self.generic_qc.clone());

        let mut valid_signatures = BTreeMap::new();
        let threshold = self.api.threshold();
        let mut stake_casted = 0;

        let lock = self.vote_collection_chan.lock().await;
        while let Ok(msg) = lock.recv().await {
            // If the message is for a different view number, skip it.
            // If the message is for a different view number, skip it.
            if Into::<ConsensusMessage<_, _, _>>::into(msg.clone()).view_number() != self.cur_view {
                continue;
            }
            match msg {
                ProcessedConsensusMessage::Vote(vote_message, sender) => {
                    match vote_message {
                        Vote::Yes(vote) => {
                            if vote.signature.0
                                != <TYPES::SignatureKey as SignatureKey>::to_bytes(&sender)
                            {
                                continue;
                            }

                            // If the signature on the vote is invalid, assume it's sent by
                            // byzantine node and ignore.
                            if !self.api.is_valid_vote(
                                &vote.signature.0,
                                &vote.signature.1,
                                VoteData::Yes(vote.leaf_commitment),
                                self.cur_view,
                                // Ignoring deserialization errors below since we are getting rid of it soon
                                Unchecked(vote.vote_token.clone()),
                            ) {
                                continue;
                            }

                            // Valid votes are guaranteed to have the same block commitment, so we
                            // don't have to group them by the commitment.
                            valid_signatures.insert(
                                vote.signature.0.clone(),
                                (vote.signature.1.clone(), vote.vote_token.clone()),
                            );

                            stake_casted += u64::from(vote.vote_token.vote_count());

                            if stake_casted >= u64::from(threshold) {
                                // construct QC
                                let qc = QuorumCertificate {
                                    leaf_commitment: vote.leaf_commitment,
                                    view_number: self.cur_view,
                                    signatures: valid_signatures,
                                    is_genesis: false,
                                };
                                return qc;
                            }
                        }
                        Vote::Timeout(vote) => {
                            qcs.insert(vote.justify_qc);
                        }
                        _ => {
                            warn!("The next leader has received an unexpected vote!");
                        }
                    }
                }
                ProcessedConsensusMessage::NextViewInterrupt(_view_number) => {
                    self.api.send_next_leader_timeout(self.cur_view).await;
                    break;
                }
                ProcessedConsensusMessage::Proposal(_p, _sender) => {
                    warn!("The next leader has received an unexpected proposal!");
                }
            }
        }
        qcs.into_iter().max_by_key(|qc| qc.view_number).unwrap()
    }
}
