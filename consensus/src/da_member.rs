//! Contains the [`DAMember`] struct used for the committee member step in the consensus algorithm
//! with DA committee, i.e. in the sequencing consensus.

use crate::{
    utils::{View, ViewInner},
    Consensus, ConsensusApi,
};
use async_compatibility_layer::channel::UnboundedReceiver;
use async_lock::{Mutex, RwLock, RwLockUpgradableReadGuard};
use commit::Committable;
use either::Left;
use hotshot_types::{
    certificate::QuorumCertificate,
    data::{DAProposal, SequencingLeaf},
    message::{ConsensusMessage, ProcessedConsensusMessage},
    traits::{
        election::{Election, SignedCertificate},
        node_implementation::NodeType,
        signature_key::SignatureKey,
    },
};
use std::sync::Arc;
use tracing::{error, info, instrument, warn};

/// This view's DA committee member.
#[derive(Debug, Clone)]
pub struct DAMember<
    A: ConsensusApi<TYPES, SequencingLeaf<TYPES>, DAProposal<TYPES>>,
    TYPES: NodeType,
    ELECTION: Election<
        TYPES,
        LeafType = SequencingLeaf<TYPES>,
        QuorumCertificate = QuorumCertificate<TYPES, SequencingLeaf<TYPES>>,
    >,
> {
    /// ID of node.
    pub id: u64,
    /// Reference to consensus. DA committee member will require a write lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES, SequencingLeaf<TYPES>>>>,
    /// Channel for accepting leader proposals and timeouts messages.
    #[allow(clippy::type_complexity)]
    pub proposal_collection_chan: Arc<
        Mutex<
            UnboundedReceiver<
                ProcessedConsensusMessage<
                    TYPES,
                    SequencingLeaf<TYPES>,
                    DAProposal<TYPES>,
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
        A: ConsensusApi<TYPES, SequencingLeaf<TYPES>, DAProposal<TYPES>>,
        TYPES: NodeType,
        ELECTION: Election<
            TYPES,
            LeafType = SequencingLeaf<TYPES>,
            QuorumCertificate = QuorumCertificate<TYPES, SequencingLeaf<TYPES>>,
        >,
    > DAMember<A, TYPES, ELECTION>
{
    /// Returns the parent leaf of the proposal we are voting on
    async fn parent_leaf(&self) -> Option<SequencingLeaf<TYPES>> {
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

    /// DA committee member task that spins until a valid DA proposal can be signed or timeout is
    /// hit.
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "DA Member Task", level = "error")]
    #[allow(clippy::type_complexity)]
    async fn find_valid_msg<'a>(
        &self,
        view_leader_key: TYPES::SignatureKey,
    ) -> Option<SequencingLeaf<TYPES>> {
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
                        let parent = self.parent_leaf().await?;
                        let leaf = SequencingLeaf {
                            view_number: self.cur_view,
                            height: parent.height + 1,
                            justify_qc: self.high_qc.clone(),
                            parent_commitment: parent.commit(),
                            deltas: Left(p.data.deltas.clone()),
                            rejected: Vec::new(),
                            timestamp: time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
                            proposer_id: sender.to_bytes(),
                        };

                        let block_commitment = p.data.deltas.commit();
                        if !view_leader_key.validate(&p.signature, block_commitment.as_ref()) {
                            warn!(?p.signature, "Could not verify proposal.");
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
                                info!("We were not chosen for DA committee on {:?}", self.cur_view);
                            }
                            Ok(Some(vote_token)) => {
                                info!("We were chosen for DA committee on {:?}", self.cur_view);

                                // Generate and send vote
                                let message = self.api.create_da_message(
                                    self.high_qc.commit(),
                                    block_commitment,
                                    self.cur_view,
                                    vote_token,
                                );

                                info!("Sending vote to the leader {:?}", message);

                                let consensus = self.consensus.read().await;
                                if self.api.send_direct_message(sender, message).await.is_err() {
                                    consensus.metrics.failed_to_send_messages.add(1);
                                    warn!("Failed to send vote to the leader");
                                } else {
                                    consensus.metrics.outgoing_direct_messages.add(1);
                                }
                            }
                        }
                        break leaf;
                    }
                    ProcessedConsensusMessage::InternalTrigger(_trigger) => {
                        warn!("DA committee member receieved an internal trigger message. This is not what the member expects. Skipping.");
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
    pub async fn run_view(self) -> QuorumCertificate<TYPES, SequencingLeaf<TYPES>> {
        info!("DA Committee Member task started!");
        let view_leader_key = self.api.get_leader(self.cur_view).await;

        let maybe_leaf = self.find_valid_msg(view_leader_key).await;

        let Some(leaf) = maybe_leaf else {
            // We either timed out or for some reason could not accept a proposal.
            return self.high_qc;
        };

        // Update state map and leaves.
        let consensus = self.consensus.upgradable_read().await;
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

        // We're only storing the last QC. We could store more but we're realistically only going to retrieve the last one.
        if let Err(e) = self.api.store_leaf(self.cur_view, leaf).await {
            error!("Could not insert new anchor into the storage API: {:?}", e);
        }
        self.high_qc
    }
}
