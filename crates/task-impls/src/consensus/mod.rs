use std::{collections::BTreeMap, sync::Arc};

#[cfg(not(feature = "dependency-tasks"))]
use anyhow::Result;
use async_broadcast::Sender;
use async_compatibility_layer::art::async_spawn;
use async_lock::RwLock;
#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
use committable::Committable;
use futures::future::join_all;
use hotshot_task::task::{Task, TaskState};
#[cfg(not(feature = "dependency-tasks"))]
use hotshot_types::data::VidDisperseShare;
#[cfg(not(feature = "dependency-tasks"))]
use hotshot_types::message::Proposal;
use hotshot_types::{
    consensus::{CommitmentAndMetadata, Consensus},
    data::{null_block, Leaf, QuorumProposal, ViewChangeEvidence},
    event::{Event, EventType},
    message::GeneralConsensusMessage,
    simple_certificate::{QuorumCertificate, TimeoutCertificate, UpgradeCertificate},
    simple_vote::{QuorumData, QuorumVote, TimeoutData, TimeoutVote},
    traits::{
        block_contents::BlockHeader,
        election::Membership,
        network::{ConnectedNetwork, ConsensusIntentEvent},
        node_implementation::{NodeImplementation, NodeType},
        signature_key::SignatureKey,
        storage::Storage,
        ValidatedState,
    },
    vid::vid_scheme,
    vote::{Certificate, HasViewNumber},
};
use jf_primitives::vid::VidScheme;
#[cfg(async_executor_impl = "tokio")]
use tokio::task::JoinHandle;
use tracing::{debug, error, info, instrument, warn};
use vbs::version::Version;

#[cfg(not(feature = "dependency-tasks"))]
use self::proposal_helpers::handle_quorum_proposal_validated;
#[cfg(not(feature = "dependency-tasks"))]
use crate::consensus::proposal_helpers::{handle_quorum_proposal_recv, publish_proposal_if_able};
use crate::{
    consensus::view_change::update_view,
    events::{HotShotEvent, HotShotTaskCompleted},
    helpers::{broadcast_event, cancel_task},
    vote_collection::{
        create_vote_accumulator, AccumulatorInfo, HandleVoteEvent, VoteCollectionTaskState,
    },
};

/// Helper functions to handle proposal-related functionality.
pub(crate) mod proposal_helpers;

/// Handles view-change related functionality.
pub(crate) mod view_change;

/// Alias for Optional type for Vote Collectors
type VoteCollectorOption<TYPES, VOTE, CERT> = Option<VoteCollectionTaskState<TYPES, VOTE, CERT>>;

/// The state for the consensus task.  Contains all of the information for the implementation
/// of consensus
pub struct ConsensusTaskState<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Our public key
    pub public_key: TYPES::SignatureKey,
    /// Our Private Key
    pub private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
    /// Reference to consensus. The replica will require a write lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES>>>,
    /// Immutable instance state
    pub instance_state: Arc<TYPES::InstanceState>,
    /// View timeout from config.
    pub timeout: u64,
    /// Round start delay from config, in milliseconds.
    pub round_start_delay: u64,
    /// View number this view is executing in.
    pub cur_view: TYPES::Time,

    /// The commitment to the current block payload and its metadata submitted to DA.
    pub payload_commitment_and_metadata: Option<CommitmentAndMetadata<TYPES>>,

    /// Network for all nodes
    pub quorum_network: Arc<I::QuorumNetwork>,

    /// Network for DA committee
    pub committee_network: Arc<I::CommitteeNetwork>,

    /// Membership for Timeout votes/certs
    pub timeout_membership: Arc<TYPES::Membership>,

    /// Membership for Quorum Certs/votes
    pub quorum_membership: Arc<TYPES::Membership>,

    /// Membership for DA committee Votes/certs
    pub committee_membership: Arc<TYPES::Membership>,

    /// Current Vote collection task, with it's view.
    pub vote_collector:
        RwLock<VoteCollectorOption<TYPES, QuorumVote<TYPES>, QuorumCertificate<TYPES>>>,

    /// Current timeout vote collection task with its view
    pub timeout_vote_collector:
        RwLock<VoteCollectorOption<TYPES, TimeoutVote<TYPES>, TimeoutCertificate<TYPES>>>,

    /// timeout task handle
    pub timeout_task: Option<JoinHandle<()>>,

    /// Spawned tasks related to a specific view, so we can cancel them when
    /// they are stale
    pub spawned_tasks: BTreeMap<TYPES::Time, Vec<JoinHandle<()>>>,

    /// The most recent upgrade certificate this node formed.
    /// Note: this is ONLY for certificates that have been formed internally,
    /// so that we can propose with them.
    ///
    /// Certificates received from other nodes will get reattached regardless of this fields,
    /// since they will be present in the leaf we propose off of.
    pub formed_upgrade_certificate: Option<UpgradeCertificate<TYPES>>,

    /// last View Sync Certificate or Timeout Certificate this node formed.
    pub proposal_cert: Option<ViewChangeEvidence<TYPES>>,

    /// most recent decided upgrade certificate
    pub decided_upgrade_cert: Option<UpgradeCertificate<TYPES>>,

    /// Globally shared reference to the current network version.
    pub version: Arc<RwLock<Version>>,

    /// Output events to application
    pub output_event_stream: async_broadcast::Sender<Event<TYPES>>,

    /// The most recent proposal we have, will correspond to the current view if Some()
    /// Will be none if the view advanced through timeout/view_sync
    pub current_proposal: Option<QuorumProposal<TYPES>>,

    // ED Should replace this with config information since we need it anyway
    /// The node's id
    pub id: u64,

    /// This node's storage ref
    pub storage: Arc<RwLock<I::Storage>>,
}

#[cfg(not(feature = "dependency-tasks"))]
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
/// Check if we are able to vote, like whether the proposal is valid,
/// whether we have DAC and VID share, and if so, vote.
async fn vote_if_able<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    cur_view: TYPES::Time,
    proposal: QuorumProposal<TYPES>,
    public_key: TYPES::SignatureKey,
    private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
    state: Arc<RwLock<Consensus<TYPES>>>,
    storage: Arc<RwLock<I::Storage>>,
    decided_upgrade_cert: Option<UpgradeCertificate<TYPES>>,
    quorum_membership: Arc<TYPES::Membership>,
    committee_membership: Arc<TYPES::Membership>,
    instance_state: Arc<TYPES::InstanceState>,
    event_stream: Sender<Arc<HotShotEvent<TYPES>>>,
) -> bool {
    use async_lock::RwLockUpgradableReadGuard;
    use hotshot_types::utils::{View, ViewInner};

    if !quorum_membership.has_stake(&public_key) {
        debug!(
            "We were not chosen for consensus committee on {:?}",
            cur_view
        );
        return false;
    }

    let consensus = state.upgradable_read().await;
    // Only vote if you has seen the VID share for this view
    let Some(vid_shares) = consensus.vid_shares.get(&proposal.view_number) else {
        debug!(
            "We have not seen the VID share for this view {:?} yet, so we cannot vote.",
            proposal.view_number
        );
        return false;
    };
    let Some(vid_share) = vid_shares.get(&public_key).cloned() else {
        debug!("we have not seen our VID share yet");
        return false;
    };

    if let Some(upgrade_cert) = &decided_upgrade_cert {
        if upgrade_cert.in_interim(cur_view)
            && Some(proposal.block_header.payload_commitment())
                != null_block::commitment(quorum_membership.total_nodes())
        {
            info!("Refusing to vote on proposal because it does not have a null commitment, and we are between versions. Expected:\n\n{:?}\n\nActual:{:?}", null_block::commitment(quorum_membership.total_nodes()), Some(proposal.block_header.payload_commitment()));
            return false;
        }
    }

    // Only vote if you have the DA cert
    // ED Need to update the view number this is stored under?
    let Some(cert) = consensus.saved_da_certs.get(&cur_view) else {
        return false;
    };

    let view = cert.view_number;
    // TODO: do some of this logic without the vote token check, only do that when voting.
    let justify_qc = proposal.justify_qc.clone();
    let parent = consensus
        .saved_leaves
        .get(&justify_qc.get_data().leaf_commit)
        .cloned();

    // Justify qc's leaf commitment is not the same as the parent's leaf commitment, but it should be (in this case)
    let Some(parent) = parent else {
        warn!(
            "Proposal's parent missing from storage with commitment: {:?}, proposal view {:?}",
            justify_qc.get_data().leaf_commit,
            proposal.view_number,
        );
        return false;
    };
    let (Some(parent_state), _) = consensus.get_state_and_delta(parent.get_view_number()) else {
        warn!("Parent state not found! Consensus internally inconsistent");
        return false;
    };
    let Ok((validated_state, state_delta)) = parent_state
        .validate_and_apply_header(
            instance_state.as_ref(),
            &parent,
            &proposal.block_header.clone(),
            vid_share.data.common.clone(),
        )
        .await
    else {
        warn!("Block header doesn't extend the proposal!");
        return false;
    };

    let state = Arc::new(validated_state);
    let delta = Arc::new(state_delta);
    let parent_commitment = parent.commit();

    let proposed_leaf = Leaf::from_quorum_proposal(&proposal);
    if proposed_leaf.get_parent_commitment() != parent_commitment {
        return false;
    }

    // Validate the DAC.
    let message = if cert.is_valid_cert(committee_membership.as_ref()) {
        // Validate the block payload commitment for non-genesis DAC.
        if cert.get_data().payload_commit != proposal.block_header.payload_commitment() {
            error!(
                "Block payload commitment does not equal da cert payload commitment. View = {}",
                *view
            );
            return false;
        }
        if let Ok(vote) = QuorumVote::<TYPES>::create_signed_vote(
            QuorumData {
                leaf_commit: proposed_leaf.commit(),
            },
            view,
            &public_key,
            &private_key,
        ) {
            GeneralConsensusMessage::<TYPES>::Vote(vote)
        } else {
            error!("Unable to sign quorum vote!");
            return false;
        }
    } else {
        error!(
            "Invalid DAC in proposal! Skipping proposal. {:?} cur view is: {:?}",
            cert, cur_view
        );
        return false;
    };

    let mut consensus = RwLockUpgradableReadGuard::upgrade(consensus).await;
    consensus.validated_state_map.insert(
        cur_view,
        View {
            view_inner: ViewInner::Leaf {
                leaf: proposed_leaf.commit(),
                state: Arc::clone(&state),
                delta: Some(Arc::clone(&delta)),
            },
        },
    );

    if let Err(e) = storage
        .write()
        .await
        .update_undecided_state(
            consensus.saved_leaves.clone(),
            consensus.validated_state_map.clone(),
        )
        .await
    {
        warn!("Couldn't store undecided state.  Error: {:?}", e);
    }

    if let GeneralConsensusMessage::Vote(vote) = message {
        debug!(
            "Sending vote to next quorum leader {:?}",
            vote.get_view_number() + 1
        );
        // Add to the storage that we have received the VID disperse for a specific view
        if let Err(e) = storage.write().await.append_vid(&vid_share).await {
            error!(
                "Failed to store VID Disperse Proposal with error {:?}, aborting vote",
                e
            );
            return false;
        }
        broadcast_event(Arc::new(HotShotEvent::QuorumVoteSend(vote)), &event_stream).await;
        return true;
    }
    debug!(
        "Received VID share, but couldn't find DAC cert for view {:?}",
        *proposal.get_view_number(),
    );
    false
}
impl<TYPES: NodeType, I: NodeImplementation<TYPES>> ConsensusTaskState<TYPES, I> {
    /// Cancel all tasks the consensus tasks has spawned before the given view
    pub async fn cancel_tasks(&mut self, view: TYPES::Time) {
        let keep = self.spawned_tasks.split_off(&view);
        let mut cancel = Vec::new();
        while let Some((_, tasks)) = self.spawned_tasks.pop_first() {
            let mut to_cancel = tasks.into_iter().map(cancel_task).collect();
            cancel.append(&mut to_cancel);
        }
        self.spawned_tasks = keep;
        join_all(cancel).await;
    }

    /// Ignores old vote behavior and lets `QuorumVoteTask` take over.
    #[cfg(feature = "dependency-tasks")]
    async fn vote_if_able(&mut self, _event_stream: &Sender<Arc<HotShotEvent<TYPES>>>) -> bool {
        false
    }

    /// Validates whether the VID Dispersal Proposal is correctly signed
    #[cfg(not(feature = "dependency-tasks"))]
    fn validate_disperse(&self, disperse: &Proposal<TYPES, VidDisperseShare<TYPES>>) -> bool {
        let view = disperse.data.get_view_number();
        let payload_commitment = disperse.data.payload_commitment;

        // Check whether the data satisfies one of the following.
        // * From the right leader for this view.
        // * Calculated and signed by the current node.
        // * Signed by one of the staked DA committee members.
        if !self
            .quorum_membership
            .get_leader(view)
            .validate(&disperse.signature, payload_commitment.as_ref())
            && !self
                .public_key
                .validate(&disperse.signature, payload_commitment.as_ref())
        {
            let mut validated = false;
            for da_member in self.committee_membership.get_staked_committee(view) {
                if da_member.validate(&disperse.signature, payload_commitment.as_ref()) {
                    validated = true;
                    break;
                }
            }
            if !validated {
                return false;
            }
        }

        // Validate the VID share.
        if vid_scheme(self.quorum_membership.total_nodes())
            .verify_share(
                &disperse.data.share,
                &disperse.data.common,
                &payload_commitment,
            )
            .is_err()
        {
            debug!("Invalid VID share.");
            return false;
        }

        true
    }

    #[cfg(not(feature = "dependency-tasks"))]
    /// Publishes a proposal
    async fn publish_proposal(
        &mut self,
        view: TYPES::Time,
        event_stream: Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> Result<()> {
        let create_and_send_proposal_handle = publish_proposal_if_able(
            self.cur_view,
            view,
            event_stream,
            Arc::clone(&self.quorum_membership),
            self.public_key.clone(),
            self.private_key.clone(),
            Arc::clone(&self.consensus),
            self.round_start_delay,
            self.formed_upgrade_certificate.clone(),
            self.decided_upgrade_cert.clone(),
            self.payload_commitment_and_metadata.clone(),
            self.proposal_cert.clone(),
            Arc::clone(&self.instance_state),
        )
        .await?;

        self.spawned_tasks
            .entry(view)
            .or_default()
            .push(create_and_send_proposal_handle);

        Ok(())
    }

    #[cfg(not(feature = "dependency-tasks"))]
    /// Tries to vote then Publishes a proposal
    fn vote_and_publish_proposal(
        &mut self,
        vote_view: TYPES::Time,
        propose_view: TYPES::Time,
        proposal: QuorumProposal<TYPES>,
        event_stream: Sender<Arc<HotShotEvent<TYPES>>>,
    ) {
        use crate::consensus::proposal_helpers::publish_proposal_from_commitment_and_metadata;

        use self::proposal_helpers::publish_proposal_from_upgrade_cert;

        let upgrade = self.decided_upgrade_cert.clone();
        let pub_key = self.public_key.clone();
        let priv_key = self.private_key.clone();
        let consensus = Arc::clone(&self.consensus);
        let storage = Arc::clone(&self.storage);
        let quorum_mem = Arc::clone(&self.quorum_membership);
        let committee_mem = Arc::clone(&self.committee_membership);
        let instance_state = Arc::clone(&self.instance_state);
        let commitment_and_metadata = self.payload_commitment_and_metadata.clone();
        let cur_view = self.cur_view;
        let sender = event_stream.clone();
        let decided_upgrade_cert = self.decided_upgrade_cert.clone();
        let delay = self.round_start_delay;
        let formed_upgrade_certificate = self.formed_upgrade_certificate.clone();
        let proposal_cert = self.proposal_cert.clone();
        let handle = async_spawn(async move {
            vote_if_able::<TYPES, I>(
                vote_view,
                proposal,
                pub_key.clone(),
                priv_key.clone(),
                Arc::clone(&consensus),
                storage,
                upgrade,
                Arc::clone(&quorum_mem),
                committee_mem,
                Arc::clone(&instance_state),
                event_stream,
            )
            .await;
            if let Some(upgrade_cert) = decided_upgrade_cert {
                let _ = publish_proposal_from_upgrade_cert(
                    cur_view,
                    propose_view,
                    sender,
                    quorum_mem,
                    pub_key,
                    priv_key,
                    consensus,
                    upgrade_cert,
                    delay,
                    instance_state,
                )
                .await;
            } else {
                let _ = publish_proposal_from_commitment_and_metadata(
                    cur_view,
                    propose_view,
                    sender,
                    quorum_mem,
                    pub_key,
                    priv_key,
                    consensus,
                    delay,
                    formed_upgrade_certificate,
                    decided_upgrade_cert,
                    commitment_and_metadata,
                    proposal_cert,
                    instance_state,
                )
                .await;
            }
        });

        self.spawned_tasks
            .entry(propose_view)
            .or_default()
            .push(handle);
    }

    /// Handles a consensus event received on the event stream
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "Consensus replica task", level = "error")]
    pub async fn handle(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        event_stream: Sender<Arc<HotShotEvent<TYPES>>>,
    ) {
        match event.as_ref() {
            #[cfg(not(feature = "dependency-tasks"))]
            HotShotEvent::QuorumProposalRecv(proposal, sender) => {
                match handle_quorum_proposal_recv(proposal, sender, event_stream.clone(), self)
                    .await
                {
                    Ok(Some(current_proposal)) => {
                        let view = current_proposal.get_view_number();
                        let proposal = current_proposal.clone();
                        let upgrade = self.decided_upgrade_cert.clone();
                        self.current_proposal = Some(current_proposal);
                        let pub_key = self.public_key.clone();
                        let priv_key = self.private_key.clone();
                        let consensus = Arc::clone(&self.consensus);
                        let storage = Arc::clone(&self.storage);
                        let quorum_mem = Arc::clone(&self.quorum_membership);
                        let committee_mem = Arc::clone(&self.committee_membership);
                        let instance_state = Arc::clone(&self.instance_state);
                        let handle = async_spawn(async move {
                            vote_if_able::<TYPES, I>(
                                view,
                                proposal,
                                pub_key,
                                priv_key,
                                consensus,
                                storage,
                                upgrade,
                                quorum_mem,
                                committee_mem,
                                instance_state,
                                event_stream,
                            )
                            .await;
                        });
                        self.spawned_tasks.entry(view).or_default().push(handle);
                    }
                    Ok(None) => {}
                    Err(e) => debug!("Failed to propose {e:#}"),
                }
            }
            #[cfg(not(feature = "dependency-tasks"))]
            HotShotEvent::QuorumProposalValidated(proposal, _) => {
                if let Err(e) =
                    handle_quorum_proposal_validated(proposal, event_stream.clone(), self).await
                {
                    debug!("Failed to handle QuorumProposalValidated event {e:#}");
                }
            }
            HotShotEvent::QuorumVoteRecv(ref vote) => {
                debug!("Received quorum vote: {:?}", vote.get_view_number());
                if self
                    .quorum_membership
                    .get_leader(vote.get_view_number() + 1)
                    != self.public_key
                {
                    error!(
                        "We are not the leader for view {} are we the leader for view + 1? {}",
                        *vote.get_view_number() + 1,
                        self.quorum_membership
                            .get_leader(vote.get_view_number() + 2)
                            == self.public_key
                    );
                    return;
                }
                let mut collector = self.vote_collector.write().await;

                if collector.is_none() || vote.get_view_number() > collector.as_ref().unwrap().view
                {
                    debug!("Starting vote handle for view {:?}", vote.get_view_number());
                    let info = AccumulatorInfo {
                        public_key: self.public_key.clone(),
                        membership: Arc::clone(&self.quorum_membership),
                        view: vote.get_view_number(),
                        id: self.id,
                    };
                    *collector = create_vote_accumulator::<
                        TYPES,
                        QuorumVote<TYPES>,
                        QuorumCertificate<TYPES>,
                    >(&info, vote.clone(), event, &event_stream)
                    .await;
                } else {
                    let result = collector
                        .as_mut()
                        .unwrap()
                        .handle_event(Arc::clone(&event), &event_stream)
                        .await;

                    if result == Some(HotShotTaskCompleted) {
                        *collector = None;
                        // The protocol has finished
                        return;
                    }
                }
            }
            HotShotEvent::TimeoutVoteRecv(ref vote) => {
                if self
                    .timeout_membership
                    .get_leader(vote.get_view_number() + 1)
                    != self.public_key
                {
                    error!(
                        "We are not the leader for view {} are we the leader for view + 1? {}",
                        *vote.get_view_number() + 1,
                        self.timeout_membership
                            .get_leader(vote.get_view_number() + 2)
                            == self.public_key
                    );
                    return;
                }
                let mut collector = self.timeout_vote_collector.write().await;

                if collector.is_none() || vote.get_view_number() > collector.as_ref().unwrap().view
                {
                    debug!("Starting vote handle for view {:?}", vote.get_view_number());
                    let info = AccumulatorInfo {
                        public_key: self.public_key.clone(),
                        membership: Arc::clone(&self.quorum_membership),
                        view: vote.get_view_number(),
                        id: self.id,
                    };
                    *collector = create_vote_accumulator::<
                        TYPES,
                        TimeoutVote<TYPES>,
                        TimeoutCertificate<TYPES>,
                    >(&info, vote.clone(), event, &event_stream)
                    .await;
                } else {
                    let result = collector
                        .as_mut()
                        .unwrap()
                        .handle_event(Arc::clone(&event), &event_stream)
                        .await;

                    if result == Some(HotShotTaskCompleted) {
                        *collector = None;
                        // The protocol has finished
                        return;
                    }
                }
            }
            #[cfg(not(feature = "dependency-tasks"))]
            HotShotEvent::QCFormed(cert) => {
                match cert {
                    either::Right(qc) => {
                        self.proposal_cert = Some(ViewChangeEvidence::Timeout(qc.clone()));
                        // cancel poll for votes
                        self.quorum_network
                            .inject_consensus_info(ConsensusIntentEvent::CancelPollForVotes(
                                *qc.view_number,
                            ))
                            .await;

                        debug!(
                            "Attempting to publish proposal after forming a TC for view {}",
                            *qc.view_number
                        );

                        if let Err(e) = self
                            .publish_proposal(qc.view_number + 1, event_stream)
                            .await
                        {
                            warn!("Failed to propose; error = {e:?}");
                        };
                    }
                    either::Left(qc) => {
                        if let Err(e) = self.storage.write().await.update_high_qc(qc.clone()).await
                        {
                            warn!("Failed to store High QC of QC we formed. Error: {:?}", e);
                        }

                        let mut consensus = self.consensus.write().await;
                        consensus.high_qc = qc.clone();

                        // cancel poll for votes
                        self.quorum_network
                            .inject_consensus_info(ConsensusIntentEvent::CancelPollForVotes(
                                *qc.view_number,
                            ))
                            .await;

                        drop(consensus);
                        debug!(
                            "Attempting to publish proposal after forming a QC for view {}",
                            *qc.view_number
                        );

                        if let Err(e) = self
                            .publish_proposal(qc.view_number + 1, event_stream)
                            .await
                        {
                            warn!("Failed to propose; error = {e:?}");
                        };
                    }
                }
            }
            HotShotEvent::UpgradeCertificateFormed(cert) => {
                debug!(
                    "Upgrade certificate received for view {}!",
                    *cert.view_number
                );

                // Update our current upgrade_cert as long as we still have a chance of reaching a decide on it in time.
                if cert.data.decide_by >= self.cur_view + 3 {
                    debug!("Updating current formed_upgrade_certificate");

                    self.formed_upgrade_certificate = Some(cert.clone());
                }
            }
            #[cfg(not(feature = "dependency-tasks"))]
            HotShotEvent::DACertificateRecv(cert) => {
                debug!("DAC Received for view {}!", *cert.view_number);
                let view = cert.view_number;

                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForDAC(*view))
                    .await;

                self.committee_network
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForVotes(*view))
                    .await;

                self.consensus
                    .write()
                    .await
                    .saved_da_certs
                    .insert(view, cert.clone());
                let Some(proposal) = self.current_proposal.clone() else {
                    return;
                };
                if proposal.get_view_number() != view {
                    return;
                }
                let upgrade = self.decided_upgrade_cert.clone();
                let pub_key = self.public_key.clone();
                let priv_key = self.private_key.clone();
                let consensus = Arc::clone(&self.consensus);
                let storage = Arc::clone(&self.storage);
                let quorum_mem = Arc::clone(&self.quorum_membership);
                let committee_mem = Arc::clone(&self.committee_membership);
                let instance_state = Arc::clone(&self.instance_state);
                let handle = async_spawn(async move {
                    vote_if_able::<TYPES, I>(
                        view,
                        proposal,
                        pub_key,
                        priv_key,
                        consensus,
                        storage,
                        upgrade,
                        quorum_mem,
                        committee_mem,
                        instance_state,
                        event_stream,
                    )
                    .await;
                });
                self.spawned_tasks.entry(view).or_default().push(handle);
            }
            #[cfg(not(feature = "dependency-tasks"))]
            HotShotEvent::VIDShareRecv(disperse) => {
                let view = disperse.data.get_view_number();

                debug!(
                    "VID disperse received for view: {:?} in consensus task",
                    view
                );

                // Allow VID disperse date that is one view older, in case we have updated the
                // view.
                // Adding `+ 1` on the LHS rather than `- 1` on the RHS, to avoid the overflow
                // error due to subtracting the genesis view number.
                if view + 1 < self.cur_view {
                    warn!("Throwing away VID disperse data that is more than one view older");
                    return;
                }

                debug!("VID disperse data is not more than one view older.");

                if !self.validate_disperse(disperse) {
                    warn!("Could not verify VID dispersal/share sig.");
                    return;
                }

                self.consensus
                    .write()
                    .await
                    .vid_shares
                    .entry(view)
                    .or_default()
                    .insert(disperse.data.recipient_key.clone(), disperse.clone());
                if disperse.data.recipient_key != self.public_key {
                    return;
                }
                // stop polling for the received disperse after verifying it's valid
                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForVIDDisperse(
                        *disperse.data.view_number,
                    ))
                    .await;
                let Some(proposal) = self.current_proposal.clone() else {
                    return;
                };
                let upgrade = self.decided_upgrade_cert.clone();
                let pub_key = self.public_key.clone();
                let priv_key = self.private_key.clone();
                let consensus = Arc::clone(&self.consensus);
                let storage = Arc::clone(&self.storage);
                let quorum_mem = Arc::clone(&self.quorum_membership);
                let committee_mem = Arc::clone(&self.committee_membership);
                let instance_state = Arc::clone(&self.instance_state);
                let handle = async_spawn(async move {
                    vote_if_able::<TYPES, I>(
                        view,
                        proposal,
                        pub_key,
                        priv_key,
                        consensus,
                        storage,
                        upgrade,
                        quorum_mem,
                        committee_mem,
                        instance_state,
                        event_stream,
                    )
                    .await;
                });
                self.spawned_tasks.entry(view).or_default().push(handle);
            }
            HotShotEvent::ViewChange(new_view) => {
                let new_view = *new_view;
                debug!("View Change event for view {} in consensus task", *new_view);

                let old_view_number = self.cur_view;

                // Start polling for VID disperse for the new view
                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::PollForVIDDisperse(
                        *old_view_number + 1,
                    ))
                    .await;

                // If we have a decided upgrade certificate,
                // we may need to upgrade the protocol version on a view change.
                if let Some(ref cert) = self.decided_upgrade_cert {
                    if new_view == cert.data.new_version_first_view {
                        warn!(
                            "Updating version based on a decided upgrade cert: {:?}",
                            cert
                        );
                        let mut version = self.version.write().await;
                        *version = cert.data.new_version;

                        broadcast_event(
                            Arc::new(HotShotEvent::VersionUpgrade(cert.data.new_version)),
                            &event_stream,
                        )
                        .await;
                    }
                }

                if let Some(commitment_and_metadata) = &self.payload_commitment_and_metadata {
                    if commitment_and_metadata.block_view < old_view_number {
                        self.payload_commitment_and_metadata = None;
                    }
                }

                // update the view in state to the one in the message
                // Publish a view change event to the application
                // Returns if the view does not need updating.
                if let Err(e) = update_view::<TYPES, I>(
                    self.public_key.clone(),
                    new_view,
                    &event_stream,
                    Arc::clone(&self.quorum_membership),
                    Arc::clone(&self.quorum_network),
                    self.timeout,
                    Arc::clone(&self.consensus),
                    &mut self.cur_view,
                    &mut self.timeout_task,
                )
                .await
                {
                    tracing::trace!("Failed to update view; error = {e}");
                    return;
                }

                broadcast_event(
                    Event {
                        view_number: old_view_number,
                        event: EventType::ViewFinished {
                            view_number: old_view_number,
                        },
                    },
                    &self.output_event_stream,
                )
                .await;
            }
            HotShotEvent::Timeout(view) => {
                let view = *view;
                // NOTE: We may optionally have the timeout task listen for view change events
                if self.cur_view >= view {
                    return;
                }
                if !self.timeout_membership.has_stake(&self.public_key) {
                    debug!(
                        "We were not chosen for consensus committee on {:?}",
                        self.cur_view
                    );
                    return;
                }

                // cancel poll for votes
                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForVotes(*view))
                    .await;

                // cancel poll for proposal
                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForProposal(*view))
                    .await;

                let Ok(vote) = TimeoutVote::create_signed_vote(
                    TimeoutData { view },
                    view,
                    &self.public_key,
                    &self.private_key,
                ) else {
                    error!("Failed to sign TimeoutData!");
                    return;
                };

                broadcast_event(Arc::new(HotShotEvent::TimeoutVoteSend(vote)), &event_stream).await;
                broadcast_event(
                    Event {
                        view_number: view,
                        event: EventType::ViewTimeout { view_number: view },
                    },
                    &self.output_event_stream,
                )
                .await;
                debug!(
                    "We did not receive evidence for view {} in time, sending timeout vote for that view!",
                    *view
                );

                broadcast_event(
                    Event {
                        view_number: view,
                        event: EventType::ReplicaViewTimeout { view_number: view },
                    },
                    &self.output_event_stream,
                )
                .await;
                let consensus = self.consensus.read().await;
                consensus.metrics.number_of_timeouts.add(1);
            }
            HotShotEvent::SendPayloadCommitmentAndMetadata(
                payload_commitment,
                builder_commitment,
                metadata,
                view,
                fee,
            ) => {
                let view = *view;
                debug!("got commit and meta {:?}", payload_commitment);
                self.payload_commitment_and_metadata = Some(CommitmentAndMetadata {
                    commitment: *payload_commitment,
                    builder_commitment: builder_commitment.clone(),
                    metadata: metadata.clone(),
                    fee: fee.clone(),
                    block_view: view,
                });
                #[cfg(not(feature = "dependency-tasks"))]
                {
                    if self.quorum_membership.get_leader(view) == self.public_key
                        && self.consensus.read().await.high_qc.get_view_number() + 1 == view
                    {
                        if let Err(e) = self.publish_proposal(view, event_stream.clone()).await {
                            warn!("Failed to propose; error = {e:?}");
                        };
                    }

                    if let Some(cert) = &self.proposal_cert {
                        match cert {
                            ViewChangeEvidence::Timeout(tc) => {
                                if self.quorum_membership.get_leader(tc.get_view_number() + 1)
                                    == self.public_key
                                {
                                    if let Err(e) = self.publish_proposal(view, event_stream).await
                                    {
                                        error!("Failed to propose; error = {e:?}");
                                    };
                                }
                            }
                            ViewChangeEvidence::ViewSync(vsc) => {
                                if self.quorum_membership.get_leader(vsc.get_view_number())
                                    == self.public_key
                                {
                                    if let Err(e) = self.publish_proposal(view, event_stream).await
                                    {
                                        warn!("Failed to propose; error = {e:?}");
                                    };
                                }
                            }
                        }
                    }
                }
            }
            #[cfg(not(feature = "dependency-tasks"))]
            HotShotEvent::ViewSyncFinalizeCertificate2Recv(certificate) => {
                if !certificate.is_valid_cert(self.quorum_membership.as_ref()) {
                    warn!(
                        "View Sync Finalize certificate {:?} was invalid",
                        certificate.get_data()
                    );
                    return;
                }

                self.proposal_cert = Some(ViewChangeEvidence::ViewSync(certificate.clone()));

                // cancel poll for votes
                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForVotes(
                        *certificate.view_number - 1,
                    ))
                    .await;

                let view = certificate.view_number;

                if self.quorum_membership.get_leader(view) == self.public_key {
                    debug!(
                        "Attempting to publish proposal after forming a View Sync Finalized Cert for view {}",
                        *certificate.view_number
                    );

                    if let Err(e) = self.publish_proposal(view, event_stream).await {
                        error!("Failed to propose; error = {e:?}");
                    };
                }
            }
            HotShotEvent::QuorumVoteSend(vote) => {
                let Some(proposal) = self.current_proposal.clone() else {
                    return;
                };

                let new_view = proposal.get_view_number() + 1;
                // In future we can use the mempool model where we fetch the proposal if we don't have it, instead of having to wait for it here
                // This is for the case where we form a QC but have not yet seen the previous proposal ourselves
                let should_propose = self.quorum_membership.get_leader(new_view) == self.public_key
                    && self.consensus.read().await.high_qc.view_number
                        == proposal.get_view_number();
                // todo get rid of this clone
                let qc = self.consensus.read().await.high_qc.clone();

                if should_propose {
                    debug!(
                        "Attempting to publish proposal after voting; now in view: {}",
                        *new_view
                    );
                    let _ = self
                        .publish_proposal(qc.view_number + 1, event_stream.clone())
                        .await;
                }
                if proposal.get_view_number() <= vote.get_view_number()
                    && self.public_key != self.quorum_membership.get_leader(vote.get_view_number())
                {
                    self.current_proposal = None;
                }
            }
            _ => {}
        }
    }
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> TaskState for ConsensusTaskState<TYPES, I> {
    type Event = Arc<HotShotEvent<TYPES>>;
    type Output = ();
    fn filter(&self, event: &Arc<HotShotEvent<TYPES>>) -> bool {
        !matches!(
            event.as_ref(),
            HotShotEvent::QuorumProposalRecv(_, _)
                | HotShotEvent::QuorumVoteRecv(_)
                | HotShotEvent::QuorumProposalValidated(..)
                | HotShotEvent::QCFormed(_)
                | HotShotEvent::UpgradeCertificateFormed(_)
                | HotShotEvent::DACertificateRecv(_)
                | HotShotEvent::ViewChange(_)
                | HotShotEvent::SendPayloadCommitmentAndMetadata(..)
                | HotShotEvent::Timeout(_)
                | HotShotEvent::TimeoutVoteRecv(_)
                | HotShotEvent::VIDShareRecv(..)
                | HotShotEvent::ViewSyncFinalizeCertificate2Recv(_)
                | HotShotEvent::QuorumVoteSend(_)
                | HotShotEvent::Shutdown,
        )
    }
    async fn handle_event(event: Self::Event, task: &mut Task<Self>) -> Option<()>
    where
        Self: Sized,
    {
        let sender = task.clone_sender();
        tracing::trace!("sender queue len {}", sender.len());
        task.state_mut().handle(event, sender).await;
        None
    }
    fn should_shutdown(event: &Self::Event) -> bool {
        matches!(event.as_ref(), HotShotEvent::Shutdown)
    }
}
