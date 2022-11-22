//! Contains the [`Replica`] struct used for the leader step in the hotstuff consensus algorithm.

use crate::{
    utils::{Terminator, View, ViewInner},
    Consensus, ConsensusApi,
};
use async_lock::{Mutex, RwLock, RwLockUpgradableReadGuard, RwLockWriteGuard};
use commit::Committable;
use hotshot_types::{
    data::{Leaf, QuorumCertificate},
    message::{ConsensusMessage, TimedOut, Vote},
    traits::{node_implementation::NodeTypes, signature_key::SignatureKey, Block, State},
};
use hotshot_utils::channel::UnboundedReceiver;
use std::ops::Bound::{Excluded, Included};
use std::{collections::HashSet, sync::Arc};
use tracing::{error, info, instrument, warn};
/// This view's replica
#[derive(Debug, Clone)]
pub struct Replica<A: ConsensusApi<TYPES>, TYPES: NodeTypes> {
    /// id of node
    pub id: u64,
    /// Reference to consensus. Replica will require a write lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES>>>,
    /// channel for accepting leader proposals and timeouts messages
    pub proposal_collection_chan: Arc<Mutex<UnboundedReceiver<ConsensusMessage<TYPES>>>>,
    /// view number this view is executing in
    pub cur_view: TYPES::Time,
    /// genericQC from the pseudocode
    pub high_qc: QuorumCertificate<TYPES>,
    /// hotshot consensus api
    pub api: A,
}

impl<A: ConsensusApi<TYPES>, TYPES: NodeTypes> Replica<A, TYPES> {
    /// portion of the replica task that spins until a valid QC can be signed or
    /// timeout is hit.
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "Replica Task", level = "error")]
    async fn find_valid_msg<'a>(
        &self,
        view_leader_key: TYPES::SignatureKey,
        consensus: RwLockUpgradableReadGuard<'a, Consensus<TYPES>>,
    ) -> (
        RwLockUpgradableReadGuard<'a, Consensus<TYPES>>,
        Option<Leaf<TYPES>>,
    ) {
        let lock = self.proposal_collection_chan.lock().await;
        let mut invalid_qcs = 0;
        let leaf = loop {
            let msg = lock.recv().await;
            info!("recv-ed message {:?}", msg.clone());
            if let Ok(msg) = msg {
                // stale/newer view messages should never reach this specific task's receive channel
                if msg.view_number() != self.cur_view {
                    continue;
                }
                match msg {
                    ConsensusMessage::Proposal(p) => {
                        let parent = if let Some(parent) =
                            consensus.saved_leaves.get(&p.leaf.parent_commitment)
                        {
                            parent
                        } else {
                            warn!("Proposal's parent missing from storage");
                            continue;
                        };

                        let justify_qc = p.leaf.justify_qc;

                        // go no further if the parent view number does not
                        // match the justify_qc. We can't accept this
                        if parent.view_number != justify_qc.view_number {
                            warn!(
                                "Inconsistency in recv-ed proposal. The parent's view number, {:?} did not match the justify_qc view number, {:?}",
                                parent.view_number, justify_qc.view_number
                            );
                            continue;
                        }

                        // check that the justify_qc is valid
                        if !self.api.validate_qc(&justify_qc) {
                            invalid_qcs += 1;
                            warn!("Invalid justify_qc in proposal! Skipping proposal.");
                            continue;
                        }

                        // check that we can indeed create the state
                        let leaf = if let Ok(state) =
                            parent.state.append(&p.leaf.deltas, &self.cur_view)
                        {
                            // check the commitment
                            if state.commit() != p.leaf.state_commitment {
                                warn!("Rejected proposal! After applying deltas to parent state, resulting commitment did not match proposal's");
                                continue;
                            }
                            Leaf::new(
                                state,
                                p.leaf.deltas,
                                p.leaf.parent_commitment,
                                justify_qc.clone(),
                                self.cur_view,
                                Vec::new(),
                                time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
                                p.leaf.proposer_id,
                            )
                        } else {
                            warn!("State of proposal didn't match parent + deltas");
                            continue;
                        };

                        if !view_leader_key.validate(&p.signature, leaf.commit().as_ref()) {
                            warn!(?p.signature, "Could not verify proposal.");
                            continue;
                        }

                        let liveness_check = justify_qc.view_number > consensus.locked_view + 2;

                        // check if proposal extends from the locked leaf
                        let outcome = consensus.visit_leaf_ancestors(
                            parent.view_number,
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

                        // NOTE safenode check is here
                        // if !safenode, continue
                        // if !(safety_check || liveness_check)
                        // if !safety_check && !liveness_check
                        if !safety_check && !liveness_check {
                            warn!("Failed safety check and liveness check");
                            continue;
                        }

                        let leaf_commitment = leaf.commit();
                        let vote_token =
                            self.api.generate_vote_token(self.cur_view, leaf_commitment);

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
                                info!("We were chosen for committee on {:?}", self.cur_view);
                                let signature = self.api.sign_vote(&leaf_commitment, self.cur_view);

                                // Generate and send vote
                                let vote = ConsensusMessage::<TYPES>::Vote(Vote {
                                    block_commitment: leaf.deltas.commit(),
                                    justify_qc_commitment: leaf.justify_qc.commit(),
                                    signature,
                                    leaf_commitment,
                                    current_view: self.cur_view,
                                    vote_token,
                                });

                                let next_leader = self.api.get_leader(self.cur_view + 1).await;

                                info!("Sending vote to next leader {:?}", vote);

                                if self
                                    .api
                                    .send_direct_message(next_leader, vote)
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
                        break leaf;
                    }
                    ConsensusMessage::NextViewInterrupt(_view_number) => {
                        let next_leader = self.api.get_leader(self.cur_view + 1).await;

                        consensus.metrics.number_of_timeouts.add(1);

                        let timed_out_msg = ConsensusMessage::TimedOut(TimedOut {
                            current_view: self.cur_view,
                            justify_qc: self.high_qc.clone(),
                        });
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

                        return (consensus, None);
                    }
                    ConsensusMessage::Vote(_) | ConsensusMessage::TimedOut(_) => {
                        // should only be for leader, never replica
                        warn!("Replica receieved a vote or timed out message. This is not what the replica expects. Skipping.");
                        continue;
                    }
                }
            }
            // fall through logic if we did not receive successfully from channel
            warn!("Replica did not receive successfully from channel. Terminating Replica.");
            let mut consensus = RwLockUpgradableReadGuard::upgrade(consensus).await;
            consensus.invalid_qc += invalid_qcs;
            self.api.send_replica_timeout(self.cur_view).await;
            return (RwLockWriteGuard::downgrade_to_upgradable(consensus), None);
        };
        let mut consensus = RwLockUpgradableReadGuard::upgrade(consensus).await;
        consensus.invalid_qc += invalid_qcs;
        (
            RwLockWriteGuard::downgrade_to_upgradable(consensus),
            Some(leaf),
        )
    }

    /// run one view of replica
    /// returns the `high_qc`
    #[instrument(skip(self), fields(id = self.id, view = *self.cur_view), name = "Replica Task", level = "error")]
    pub async fn run_view(self) -> QuorumCertificate<TYPES> {
        info!("Replica task started!");
        let consensus = self.consensus.upgradable_read().await;
        let view_leader_key = self.api.get_leader(self.cur_view).await;

        let (consensus, maybe_leaf) = self.find_valid_msg(view_leader_key, consensus).await;

        let leaf = if let Some(leaf) = maybe_leaf {
            leaf
        } else {
            // we either timed out or for some reason
            // could not accept a proposal
            return self.high_qc;
        };

        let mut new_anchor_view = consensus.last_decided_view;
        let mut new_locked_view = consensus.locked_view;
        let mut last_view_number_visited = self.cur_view;
        let mut new_commit_reached: bool = false;
        let mut new_decide_reached = false;
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
                        let txns = leaf.deltas.contained_transactions();
                        for txn in txns {
                            included_txns.insert(txn);
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
                .add_point((num_views_since_last_anchor - views_seen as f64) as f64);
            consensus
                .metrics
                .number_of_views_per_decide_event
                .add_point(num_views_since_last_anchor);
            consensus
                .metrics
                .invalid_qc_views
                .add_point(consensus.invalid_qc as f64);
            consensus
                .transactions
                .modify(|txns| {
                    *txns = txns
                        .drain()
                        .filter(|(txn_hash, _txn)| !included_txns_set.contains(txn_hash))
                        .collect();
                })
                .await;

            consensus
                .metrics
                .rejected_transactions
                .add(leaf.rejected.len());

            let decide_sent = self
                .api
                .send_decide(consensus.last_decided_view, leaf_views);
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
