//! Contains the [`DAReplica`] struct used for the replica step in the consensus algorithm with DA
//! committee, i.e. in the sequencing consensus.

use crate::{
    utils::{Terminator, View, ViewInner},
    Consensus, ConsensusApi,
};
use async_compatibility_layer::channel::UnboundedReceiver;
use async_lock::{Mutex, RwLock, RwLockUpgradableReadGuard, RwLockWriteGuard};
use bincode::Options;
use commit::Committable;
use either::{Left, Right};
use hotshot_types::{
    certificate::{DACertificate, QuorumCertificate},
    data::{CommitmentProposal, DALeaf},
    message::{ConsensusMessage, ProcessedConsensusMessage},
    traits::{
        election::{Election, SignedCertificate},
        node_implementation::NodeType,
        signature_key::SignatureKey,
        Block,
    },
};
use hotshot_utils::bincode::bincode_opts;
use std::collections::HashSet;
use std::ops::Bound::{Excluded, Included};
use std::sync::Arc;
use tracing::{error, info, instrument, warn};

/// This view's replica for sequencing consensus.
#[derive(Debug, Clone)]
pub struct DAReplica<
    A: ConsensusApi<TYPES, DALeaf<TYPES>, CommitmentProposal<TYPES, ELECTION>>,
    TYPES: NodeType,
    ELECTION: Election<
        TYPES,
        LeafType = DALeaf<TYPES>,
        DACertificate = DACertificate<TYPES>,
        QuorumCertificate = QuorumCertificate<TYPES, DALeaf<TYPES>>,
    >,
> {
    /// ID of node.
    pub id: u64,
    /// Reference to consensus. The replica will require a write lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES, DALeaf<TYPES>>>>,
    /// Channel for accepting leader proposals and timeouts messages.
    #[allow(clippy::type_complexity)]
    pub proposal_collection_chan: Arc<
        Mutex<
            UnboundedReceiver<
                ProcessedConsensusMessage<
                    TYPES,
                    DALeaf<TYPES>,
                    CommitmentProposal<TYPES, ELECTION>,
                >,
            >,
        >,
    >,
    /// View number this view is executing in.
    pub cur_view: TYPES::Time,
    /// The High QC.
    pub high_qc: ELECTION::QuorumCertificate,
    /// HotShot consensus API.
    pub api: A,
}

impl<
        A: ConsensusApi<TYPES, DALeaf<TYPES>, CommitmentProposal<TYPES, ELECTION>>,
        TYPES: NodeType,
        ELECTION: Election<
            TYPES,
            LeafType = DALeaf<TYPES>,
            DACertificate = DACertificate<TYPES>,
            QuorumCertificate = QuorumCertificate<TYPES, DALeaf<TYPES>>,
        >,
    > DAReplica<A, TYPES, ELECTION>
{
    // TODO (da) Move this function so that it can be used by leader, replica, and committee member logic.
    /// Returns the parent leaf of the proposal we are voting on
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

    /// Replica task for sequencing consensus that spins until a vote can be made or timeout is
    /// hit.
    ///
    /// Returns the new leaf if it's valid.
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "Sequencing Replica Task", level = "error")]
    #[allow(clippy::type_complexity)]
    async fn find_valid_msg<'a>(
        &self,
        view_leader_key: TYPES::SignatureKey,
        consensus: RwLockUpgradableReadGuard<'a, Consensus<TYPES, DALeaf<TYPES>>>,
    ) -> (
        RwLockUpgradableReadGuard<'a, Consensus<TYPES, DALeaf<TYPES>>>,
        Option<DALeaf<TYPES>>,
    ) {
        let lock = self.proposal_collection_chan.lock().await;
        let mut invalid_qc = false;
        let leaf = loop {
            let msg = lock.recv().await;
            info!("recv-ed message {:?}", msg.clone());
            if let Ok(msg) = msg {
                // stale/newer view messages should never reach this specific task's receive channel
                if Into::<ConsensusMessage<_, _, _>>::into(msg.clone()).view_number()
                    != self.cur_view
                {
                    continue;
                }
                match msg {
                    ProcessedConsensusMessage::Proposal(p, sender) => {
                        if view_leader_key != sender {
                            continue;
                        }

                        let mut valid_leaf = None;
                        let vote_token = self.api.make_vote_token(self.cur_view);
                        match vote_token {
                            Err(e) => {
                                error!(
                                    "Failed to generate vote token for {:?} {:?}",
                                    self.cur_view, e
                                );
                            }
                            Ok(None) => {
                                info!(
                                    "We were not chosen for consensus committee on {:?}",
                                    self.cur_view
                                );
                            }
                            Ok(Some(vote_token)) => {
                                info!(
                                    "We were chosen for consensus committee on {:?}",
                                    self.cur_view
                                );

                                let message;

                                // Construct the leaf.
                                let justify_qc = p.data.justify_qc;
                                let parent_commitment = match self.parent_leaf().await {
                                    Some(parent) => parent.commit(),
                                    None => {
                                        break None;
                                    }
                                };
                                let block_commitment = p.data.block_commitment;
                                let leaf = DALeaf {
                                    view_number: self.cur_view,
                                    height: p.data.height,
                                    justify_qc: justify_qc.clone(),
                                    parent_commitment,
                                    deltas: Right(p.data.block_commitment),
                                    rejected: Vec::new(),
                                    timestamp: time::OffsetDateTime::now_utc()
                                        .unix_timestamp_nanos(),
                                    proposer_id: sender.to_bytes(),
                                };
                                let justify_qc_commitment = justify_qc.commit();
                                let leaf_commitment = leaf.commit();

                                // Validate the `justify_qc`.
                                if !self.api.is_valid_qc(&justify_qc) {
                                    invalid_qc = true;
                                    warn!("Invalid justify_qc in proposal!.");
                                    message = self.api.create_no_message(
                                        justify_qc_commitment,
                                        leaf_commitment,
                                        self.cur_view,
                                        vote_token,
                                    );
                                }
                                // Validate the DAC.
                                else if !self.api.is_valid_dac(&p.data.dac, block_commitment) {
                                    warn!("Invalid DAC in proposal! Skipping proposal.");
                                    message = self.api.create_no_message(
                                        justify_qc_commitment,
                                        leaf_commitment,
                                        self.cur_view,
                                        vote_token,
                                    );
                                }
                                // Validate the signature.
                                else if !view_leader_key
                                    .validate(&p.signature, leaf_commitment.as_ref())
                                {
                                    warn!(?p.signature, "Could not verify proposal.");
                                    message = self.api.create_no_message(
                                        justify_qc_commitment,
                                        leaf_commitment,
                                        self.cur_view,
                                        vote_token,
                                    );
                                }
                                // Create a positive vote if either liveness or safety check
                                // passes.
                                else {
                                    // Liveness check.
                                    let liveness_check =
                                        justify_qc.view_number > consensus.locked_view;

                                    // Safety check.
                                    // Check if proposal extends from the locked leaf.
                                    let outcome = consensus.visit_leaf_ancestors(
                                        justify_qc.view_number,
                                        Terminator::Inclusive(consensus.locked_view),
                                        false,
                                        |leaf| {
                                            // if leaf view no == locked view no then we're done, report success by
                                            // returning true
                                            leaf.view_number != consensus.locked_view
                                        },
                                    );
                                    let safety_check = outcome.is_ok();
                                    if let Err(e) = outcome {
                                        self.api.send_view_error(self.cur_view, Arc::new(e)).await;
                                    }

                                    // Skip if both saftey and liveness checks fail.
                                    if !safety_check && !liveness_check {
                                        warn!("Failed safety check and liveness check");
                                        message = self.api.create_no_message(
                                            justify_qc_commitment,
                                            leaf_commitment,
                                            self.cur_view,
                                            vote_token,
                                        );
                                    } else {
                                        // A valid leaf is found.
                                        valid_leaf = Some(leaf);

                                        // Generate a message with yes vote.
                                        message = self.api.create_yes_message(
                                            justify_qc_commitment,
                                            leaf_commitment,
                                            self.cur_view,
                                            vote_token,
                                        );
                                    }
                                }

                                info!("Sending vote to next leader {:?}", message);
                                let next_leader = self.api.get_leader(self.cur_view + 1).await;
                                if self
                                    .api
                                    .send_direct_message(next_leader, message)
                                    .await
                                    .is_err()
                                {
                                    consensus.metrics.failed_to_send_messages.add(1);
                                    warn!("Failed to send vote to next leader");
                                } else {
                                    consensus.metrics.outgoing_direct_messages.add(1);
                                }
                            }
                        }
                        break valid_leaf;
                    }
                    ProcessedConsensusMessage::NextViewInterrupt(_view_number) => {
                        let next_leader = self.api.get_leader(self.cur_view + 1).await;

                        consensus.metrics.number_of_timeouts.add(1);

                        let vote_token = self.api.make_vote_token(self.cur_view);

                        match vote_token {
                            Err(e) => {
                                error!(
                                    "Failed to generate vote token for {:?} {:?}",
                                    self.cur_view, e
                                );
                            }
                            Ok(None) => {
                                info!("We were not chosen for committee on {:?}", self.cur_view);
                            }
                            Ok(Some(vote_token)) => {
                                let timed_out_msg = self.api.create_timeout_message(
                                    self.high_qc.clone(),
                                    self.cur_view,
                                    vote_token,
                                );
                                warn!(
                                    "Timed out! Sending timeout to next leader {:?}",
                                    timed_out_msg
                                );

                                // send timedout message to the next leader
                                if let Err(e) = self
                                    .api
                                    .send_direct_message(next_leader.clone(), timed_out_msg)
                                    .await
                                {
                                    consensus.metrics.failed_to_send_messages.add(1);
                                    warn!(
                                        ?next_leader,
                                        ?e,
                                        "Could not send time out message to next_leader"
                                    );
                                } else {
                                    consensus.metrics.outgoing_direct_messages.add(1);
                                }

                                // exits from entire function
                                self.api.send_replica_timeout(self.cur_view).await;
                            }
                        }
                        return (consensus, None);
                    }
                    ProcessedConsensusMessage::Vote(_, _) => {
                        // should only be for leader, never replica
                        warn!("Replica receieved a vote message. This is not what the replica expects. Skipping.");
                        continue;
                    }
                }
            }
            // fall through logic if we did not receive successfully from channel
            warn!("Replica did not receive successfully from channel. Terminating Replica.");
            return (consensus, None);
        };
        let mut consensus = RwLockUpgradableReadGuard::upgrade(consensus).await;
        if invalid_qc {
            consensus.invalid_qc += 1;
        }
        (RwLockWriteGuard::downgrade_to_upgradable(consensus), leaf)
    }

    /// Run one view of the replica for sequencing consensus.
    #[instrument(skip(self), fields(id = self.id, view = *self.cur_view), name = "Sequencing Replica Task", level = "error")]
    pub async fn run_view(self) -> ELECTION::QuorumCertificate {
        info!("Sequencing replica task started!");
        let view_leader_key = self.api.get_leader(self.cur_view).await;
        let consensus = self.consensus.upgradable_read().await;

        let (consensus, maybe_leaf) = self.find_valid_msg(view_leader_key, consensus).await;

        let Some(leaf) = maybe_leaf else {
            // We either timed out or for some reason could not vote on a proposal.
            return self.high_qc;
        };

        let mut new_anchor_view = consensus.last_decided_view;
        let mut new_locked_view = consensus.locked_view;
        let mut last_view_number_visited = self.cur_view;
        let mut new_commit_reached: bool = false;
        let mut new_decide_reached = false;
        let mut new_decide_qc = None;
        let mut leaf_views = Vec::new();
        let mut included_txns = HashSet::new();
        let old_anchor_view = consensus.last_decided_view;
        let parent_view = leaf.justify_qc.view_number;
        let mut current_chain_length = 0usize;
        if parent_view + 1 == self.cur_view {
            current_chain_length += 1;
            if let Err(e) = consensus.visit_leaf_ancestors(
                parent_view,
                Terminator::Exclusive(old_anchor_view),
                true,
                |leaf| {
                    if !new_decide_reached {
                        if last_view_number_visited == leaf.view_number + 1 {
                            last_view_number_visited = leaf.view_number;
                            current_chain_length += 1;
                            if current_chain_length == 2 {
                                new_locked_view = leaf.view_number;
                                new_commit_reached = true;
                                // The next leaf in the chain, if there is one, is decided, so this
                                // leaf's justify_qc would become the QC for the decided chain.
                                new_decide_qc = Some(leaf.justify_qc.clone());
                            } else if current_chain_length == 3 {
                                new_anchor_view = leaf.view_number;
                                new_decide_reached = true;
                            }
                        } else {
                            // nothing more to do here... we don't have a new chain extension
                            return false;
                        }
                    }
                    // starting from the first iteration with a three chain, e.g. right after the else if case nested in the if case above
                    if new_decide_reached {
                        leaf_views.push(leaf.clone());
                        if let Left(block) = &leaf.deltas {
                            let txns = block.contained_transactions();
                            for txn in txns {
                                included_txns.insert(txn);
                            }
                        }
                    }
                    true
                },
            ) {
                self.api.send_view_error(self.cur_view, Arc::new(e)).await;
            }
        }
        let high_qc = leaf.justify_qc.clone();

        let included_txns_set: HashSet<_> = if new_decide_reached {
            included_txns
        } else {
            HashSet::new()
        };

        // promote lock here
        let mut consensus = RwLockUpgradableReadGuard::upgrade(consensus).await;
        consensus.state_map.insert(
            self.cur_view,
            View {
                view_inner: ViewInner::Leaf {
                    leaf: leaf.commit(),
                },
            },
        );

        consensus.metrics.number_of_views_since_last_commit.set(
            consensus
                .state_map
                .range((
                    Excluded(consensus.last_decided_view),
                    Included(self.cur_view),
                ))
                .count(),
        );

        consensus.saved_leaves.insert(leaf.commit(), leaf.clone());
        if new_commit_reached {
            consensus.locked_view = new_locked_view;
        }
        #[allow(clippy::cast_precision_loss)]
        if new_decide_reached {
            let num_views_since_last_anchor =
                (*self.cur_view - *consensus.last_decided_view) as f64;
            let views_seen = consensus
                .state_map
                .range((
                    Excluded(consensus.last_decided_view),
                    Included(self.cur_view),
                ))
                .count();
            // A count of all veiws we saw that aren't in the current chain (so won't be commited)
            consensus
                .metrics
                .discarded_views_per_decide_event
                .add_point((views_seen - current_chain_length) as f64);
            // An empty view is one we didn't see a leaf for but we moved past that view number
            consensus
                .metrics
                .empty_views_per_decide_event
                .add_point(num_views_since_last_anchor - views_seen as f64);
            consensus
                .metrics
                .number_of_views_per_decide_event
                .add_point(num_views_since_last_anchor);
            consensus
                .metrics
                .invalid_qc_views
                .add_point(consensus.invalid_qc as f64);

            let mut included_txn_size = 0;
            consensus
                .transactions
                .modify(|txns| {
                    *txns = txns
                        .drain()
                        .filter(|(txn_hash, txn)| {
                            #[allow(clippy::cast_possible_wrap)]
                            if included_txns_set.contains(txn_hash) {
                                included_txn_size +=
                                    bincode_opts().serialized_size(txn).unwrap_or_default() as i64;
                                false
                            } else {
                                true
                            }
                        })
                        .collect();
                })
                .await;
            consensus
                .metrics
                .outstanding_transactions
                .update(-(included_txns_set.len() as i64));
            consensus
                .metrics
                .outstanding_transactions_memory_size
                .update(-included_txn_size);

            consensus
                .metrics
                .rejected_transactions
                .add(leaf.rejected.len());

            let decide_sent = self.api.send_decide(
                consensus.last_decided_view,
                leaf_views,
                new_decide_qc.unwrap(),
            );
            let old_anchor_view = consensus.last_decided_view;
            consensus
                .collect_garbage(old_anchor_view, new_anchor_view)
                .await;
            consensus.last_decided_view = new_anchor_view;
            consensus.invalid_qc = 0;

            // We're only storing the last QC. We could store more but we're realistically only going to retrieve the last one.
            if let Err(e) = self.api.store_leaf(old_anchor_view, leaf).await {
                error!("Could not insert new anchor into the storage API: {:?}", e);
            }

            decide_sent.await;
        }
        high_qc
    }
}
