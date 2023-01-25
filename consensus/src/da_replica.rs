//! Contains the [`DAReplica`] struct used for the replica step in the consensus algorithm with DA
//! committee, i.e. in the sequencing consensus.

use crate::{
    utils::{Terminator, View, ViewInner},
    Consensus, ConsensusApi,
};
use async_compatibility_layer::channel::UnboundedReceiver;
use async_lock::{Mutex, RwLock, RwLockUpgradableReadGuard};
use commit::Committable;
use either::Left;
use hotshot_types::{
    certificate::QuorumCertificate,
    data::{CommitmentProposal, CommitmentProposal, DALeaf},
    message::{ConsensusMessage, DAVote, ProcessedConsensusMessage, Vote},
    traits::{
        election::{Election, SignedCertificate},
        node_implementation::NodeType,
        signature_key::SignatureKey,
        state::{TestableBlock, TestableState},
        State,
    },
};
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
        QuorumCertificate = QuorumCertificate<TYPES, DALeaf<TYPES>>,
    >,
> where
    TYPES::StateType: TestableState,
    TYPES::BlockType: TestableBlock,
{
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
            QuorumCertificate = QuorumCertificate<TYPES, DALeaf<TYPES>>,
        >,
    > DAReplica<A, TYPES, ELECTION>
where
    TYPES::StateType: TestableState,
    TYPES::BlockType: TestableBlock,
{
    /// Replica task that spins until a valid QC can be signed or timeout is hit.
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
        let mut invalid_qcs = 0;
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
                                // Validate the `justify_qc`.
                                let justify_qc = p.data.justify_qc;
                                if !self.api.is_valid_qc(&justify_qc) {
                                    invalid_qcs += 1;
                                    warn!("Invalid justify_qc in proposal!.");
                                    continue;
                                }

                                // Validate the DAC.
                                if !self.api.is_valid_dac(&p.data.dac) {
                                    warn!("Invalid DAC in proposal! Skipping proposal.");
                                    continue;
                                }

                                // Validate the signature.
                                let leaf_commitment = 
                                if !view_leader_key
                                    .validate(&p.signature, p.justify_qc.leaf_commitment().as_ref())
                                {
                                    warn!(?p.signature, "Could not verify proposal.");
                                    continue;
                                }

                                // Liveness check.
                                let liveness_check =
                                    justify_qc.view_number > consensus.locked_view + 2;

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
                                    continue;
                                }

                                let leaf_commitment = leaf.commit();

                                info!("We were chosen for committee on {:?}", self.cur_view);
                                let signature = self.api.sign_yes_vote(leaf_commitment);

                                // Generate and send vote
                                let vote = ConsensusMessage::<
                                    TYPES,
                                    DALeaf<TYPES>,
                                    CommitmentProposal<TYPES, ELECTION>,
                                >::Vote(Vote::Yes(
                                    YesOrNoVote {
                                        justify_qc_commitment: leaf.justify_qc.commit(),
                                        signature,
                                        leaf_commitment,
                                        current_view: self.cur_view,
                                        vote_token,
                                    },
                                ));

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
                    ProcessedConsensusMessage::NextViewInterrupt(_view_number) => {
                        let next_leader = self.api.get_leader(self.cur_view + 1).await;

                        consensus.metrics.number_of_timeouts.add(1);

                        let signature = self.api.sign_timeout_vote(self.cur_view);
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
                                let timed_out_msg =
                                    ConsensusMessage::Vote(Vote::Timeout(TimeoutVote {
                                        justify_qc: self.high_qc.clone(),
                                        signature,
                                        current_view: self.cur_view,
                                        vote_token,
                                    }));
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

    /// Run one view of DA committee member.
    #[instrument(skip(self), fields(id = self.id, view = *self.cur_view), name = "DA Member Task", level = "error")]
    pub async fn run_view(self)
    where
        TYPES::StateType: TestableState,
        TYPES::BlockType: TestableBlock,
    {
        info!("DA Committee Member task started!");
        let view_leader_key = self.api.get_leader(self.cur_view).await;

        let maybe_leaf = self.find_valid_msg(view_leader_key).await;

        let leaf = if let Some(leaf) = maybe_leaf {
            leaf
        } else {
            // We either timed out or for some reason could not accept a proposal.
            return;
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
    }
}
