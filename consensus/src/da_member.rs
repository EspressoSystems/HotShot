//! Contains the [`DAMember`] struct used for the committee member step in the consensus algorithm
//! with DA committee.

use crate::{
    utils::{Terminator, View, ViewInner},
    Consensus, ConsensusApi,
};
use async_compatibility_layer::channel::UnboundedReceiver;
use async_lock::{Mutex, RwLock, RwLockUpgradableReadGuard, RwLockWriteGuard};
use bincode::Options;
use commit::Committable;
use hotshot_types::{
    certificate::QuorumCertificate,
    data::{DALeaf, DAProposal},
    message::{ConsensusMessage, ProcessedConsensusMessage, TimeoutVote, Vote, YesOrNoVote},
    traits::{
        election::Election,
        node_implementation::NodeType,
        signature_key::SignatureKey,
        state::{TestableBlock, TestableState},
        Block, State,
    },
};
use hotshot_utils::bincode::bincode_opts;
use std::ops::Bound::{Excluded, Included};
use std::{collections::HashSet, sync::Arc};
use tracing::{error, info, instrument, warn};

/// This view's DA committee member.
#[derive(Debug, Clone)]
pub struct DAMember<
    A: ConsensusApi<TYPES, DALeaf<TYPES>, DAProposal<TYPES, ELECTION>>,
    TYPES: NodeType,
    ELECTION: Election<TYPES, LeafType = DALeaf<TYPES>>,
> where
    TYPES::StateType: TestableState,
    TYPES::BlockType: TestableBlock,
{
    /// ID of node.
    pub id: u64,
    /// Reference to consensus. DA committee member will require a write lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES, DALeaf<TYPES>>>>,
    /// Channel for accepting leader proposals and timeouts messages.
    #[allow(clippy::type_complexity)]
    pub proposal_collection_chan: Arc<
        Mutex<
            UnboundedReceiver<
                ProcessedConsensusMessage<TYPES, DALeaf<TYPES>, DAProposal<TYPES, ELECTION>>,
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
        A: ConsensusApi<TYPES, DALeaf<TYPES>, DAProposal<TYPES, ELECTION>>,
        TYPES: NodeType,
        ELECTION: Election<TYPES, LeafType = DALeaf<TYPES>>,
    > DAMember<A, TYPES, ELECTION>
where
    TYPES::StateType: TestableState,
    TYPES::BlockType: TestableBlock,
{
    /// Returns the parent leaf of the proposal we are voting on
    async fn parent_leaf(&self) -> Option<DALeaf<TYPES>> {
        let parent_view_number = &self.high_qc.view_number;
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
                // can happen if future api is whacked
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

    /// DA committee member task that spins until a valid QC can be signed or timeout is hit.
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "DA Member Task", level = "error")]
    #[allow(clippy::type_complexity)]
    async fn find_valid_msg<'a>(
        &self,
        view_leader_key: TYPES::SignatureKey,
        consensus: RwLockUpgradableReadGuard<'a, Consensus<TYPES, DALeaf<TYPES>>>,
    ) -> Option<DALeaf<TYPES>> {
        let lock = self.proposal_collection_chan.lock().await;
        let leaf = loop {
            let msg = lock.recv().await;
            info!("recv-ed message {:?}", msg.clone());
            if let Ok(msg) = msg {
                // If the message is for a different view number, skip it.
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
                        let parent_leaf = self.parent_leaf();
                        let justify_qc = self.high_qc;

                        let leaf = DALeaf {
                            view_number: self.cur_view,
                            height: parent.height + 1,
                            justify_qc,
                            parent_commitment: parent.commit(),
                            deltas: p.data.deltas,
                            state: parent.state.append(&p.data.deltas, &self.cur_view),
                            rejected: Vec::new(),
                            timestamp: time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
                            proposer_id: sender.to_bytes(),
                        };

                        let block_commitment = p.data.deltas.commit();
                        if !view_leader_key.validate(&p.signature, block_commitment.as_ref()) {
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
                                info!("We were chosen for committee on {:?}", self.cur_view);
                                let signature = self.api.sign_da_vote(block_commitment);

                                // Generate and send vote
                                let vote =
                                    ConsensusMessage::<
                                        TYPES,
                                        DALeaf<TYPES>,
                                        DAProposal<TYPES, ELECTION>,
                                    >::Vote(Vote::DA(DAVote {
                                        justify_qc_commitment: justify_qc.commit(),
                                        signature,
                                        block_commitment,
                                        current_view: self.cur_view,
                                        vote_token,
                                    }));

                                info!("Sending vote to the leader {:?}", vote);

                                if self.api.send_direct_message(sender, vote).await.is_err() {
                                    consensus.metrics.failed_to_send_messages.add(1);
                                    warn!("Failed to send vote to the leader");
                                } else {
                                    consensus.metrics.outgoing_direct_messages.add(1);
                                }
                            }
                        }
                        break leaf;
                    }
                    ProcessedConsensusMessage::NextViewInterrupt(_view_number) => {
                        warn!("DA committee member receieved a next view interrupt message. This is not what the member expects. Skipping.");
                        continue;
                    }
                    ProcessedConsensusMessage::Vote(_, _) => {
                        // Should only be for DA leader, never member.
                        warn!("DA committee member receieved a vote message. This is not what the member expects. Skipping.");
                        continue;
                    }
                }
            }
            // fall through logic if we did not receive successfully from channel
            warn!("DA committee member did not receive successfully from channel.");
            return None;
        };
        Some(leaf)
    }

    /// Run one view of DA committee member.
    #[instrument(skip(self), fields(id = self.id, view = *self.cur_view), name = "DA Member Task", level = "error")]
    pub async fn run_view(self)
    where
        TYPES::StateType: TestableState,
        TYPES::BlockType: TestableBlock,
    {
        info!("DA Committee Member task started!");
        let consensus = self.consensus.upgradable_read().await;
        let view_leader_key = self.api.get_leader(self.cur_view).await;

        let maybe_leaf = self.find_valid_msg(view_leader_key, consensus).await;

        let leaf = if let Some(leaf) = maybe_leaf {
            leaf
        } else {
            // We either timed out or for some reason could not accept a proposal.
            return;
        };

        let mut last_view_number_visited = self.cur_view;
        let mut new_decide_qc = None;
        let mut leaf_views = Vec::new();
        let mut included_txns = HashSet::new();
        let parent_view = leaf.justify_qc.view_number;
        let mut current_chain_length = 0usize;
        if parent_view + 1 == self.cur_view {
            current_chain_length += 1;
            if let Err(e) = consensus.visit_leaf_ancestors(
                parent_view,
                Terminator::Exclusive(old_anchor_view),
                true,
                |leaf| false,
            ) {
                self.api.send_view_error(self.cur_view, Arc::new(e)).await;
            }
        }

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

        consensus.saved_leaves.insert(leaf.commit(), leaf.clone());
        consensus.locked_view = self.cur_view;

        // We're only storing the last QC. We could store more but we're realistically only going to retrieve the last one.
        if let Err(e) = self.api.store_leaf(old_anchor_view, leaf).await {
            error!("Could not insert new anchor into the storage API: {:?}", e);
        }

        decide_sent.await;
    }
}
