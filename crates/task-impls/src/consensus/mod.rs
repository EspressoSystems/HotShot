// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use anyhow::Result;
use async_broadcast::{Receiver, Sender};
use async_compatibility_layer::art::{async_sleep, async_spawn};
use async_lock::RwLock;
#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
use async_trait::async_trait;
use futures::future::join_all;
use hotshot_task::task::TaskState;
use hotshot_types::{
    consensus::{CommitmentAndMetadata, OuterConsensus},
    data::{QuorumProposal, VidDisperseShare, ViewChangeEvidence},
    event::{Event, EventType},
    message::{Proposal, UpgradeLock},
    simple_certificate::{QuorumCertificate, TimeoutCertificate, UpgradeCertificate},
    simple_vote::{QuorumVote, TimeoutData, TimeoutVote},
    traits::{
        election::Membership,
        node_implementation::{NodeImplementation, NodeType, Versions},
        signature_key::SignatureKey,
        storage::Storage,
    },
    vid::vid_scheme,
    vote::{Certificate, HasViewNumber},
};
use jf_vid::VidScheme;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
#[cfg(async_executor_impl = "tokio")]
use tokio::task::JoinHandle;
use tracing::{debug, error, info, instrument, warn};

use crate::{
    consensus::handlers::{
        handle_quorum_proposal_recv, handle_quorum_proposal_validated, publish_proposal_if_able,
        update_state_and_vote_if_able,
    },
    events::{HotShotEvent, HotShotTaskCompleted},
    helpers::{
        broadcast_event, cancel_task, fetch_proposal, update_view, DONT_SEND_VIEW_CHANGE_EVENT,
    },
    vote_collection::{
        create_vote_accumulator, AccumulatorInfo, HandleVoteEvent, VoteCollectionTaskState,
    },
};

/// Helper functions to handle proposal-related functionality.
pub(crate) mod handlers;

/// Alias for Optional type for Vote Collectors
type VoteCollectorOption<TYPES, VOTE, CERT> = Option<VoteCollectionTaskState<TYPES, VOTE, CERT>>;

/// The state for the consensus task.  Contains all of the information for the implementation
/// of consensus
pub struct ConsensusTaskState<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions> {
    /// Our public key
    pub public_key: TYPES::SignatureKey,
    /// Our Private Key
    pub private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
    /// Reference to consensus. The replica will require a write lock on this.
    pub consensus: OuterConsensus<TYPES>,
    /// Immutable instance state
    pub instance_state: Arc<TYPES::InstanceState>,
    /// View timeout from config.
    pub timeout: u64,
    /// Round start delay from config, in milliseconds.
    pub round_start_delay: u64,
    /// View number this view is executing in.
    pub cur_view: TYPES::Time,

    /// Timestamp this view starts at.
    pub cur_view_time: i64,

    /// The commitment to the current block payload and its metadata submitted to DA.
    pub payload_commitment_and_metadata: Option<CommitmentAndMetadata<TYPES>>,

    /// The underlying network
    pub network: Arc<I::Network>,

    /// Membership for Timeout votes/certs
    pub timeout_membership: Arc<TYPES::Membership>,

    /// Membership for Quorum Certs/votes
    pub quorum_membership: Arc<TYPES::Membership>,

    /// Membership for DA committee Votes/certs
    pub da_membership: Arc<TYPES::Membership>,

    /// Current Vote collection task, with it's view.
    pub vote_collector:
        RwLock<VoteCollectorOption<TYPES, QuorumVote<TYPES>, QuorumCertificate<TYPES>>>,

    /// Current timeout vote collection task with its view
    pub timeout_vote_collector:
        RwLock<VoteCollectorOption<TYPES, TimeoutVote<TYPES>, TimeoutCertificate<TYPES>>>,

    /// timeout task handle
    pub timeout_task: JoinHandle<()>,

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

    /// Lock for a decided upgrade
    pub upgrade_lock: UpgradeLock<TYPES, V>,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions> ConsensusTaskState<TYPES, I, V> {
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

    /// Validate the VID disperse is correctly signed and has the correct share.
    fn validate_disperse(&self, disperse: &Proposal<TYPES, VidDisperseShare<TYPES>>) -> bool {
        let view = disperse.data.view_number();
        let payload_commitment = disperse.data.payload_commitment;

        // Check whether the data satisfies one of the following.
        // * From the right leader for this view.
        // * Calculated and signed by the current node.
        // * Signed by one of the staked DA committee members.
        if !self
            .quorum_membership
            .leader(view)
            .validate(&disperse.signature, payload_commitment.as_ref())
            && !self
                .public_key
                .validate(&disperse.signature, payload_commitment.as_ref())
        {
            let mut validated = false;
            for da_member in self.da_membership.staked_committee(view) {
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

    /// Publishes a proposal
    #[instrument(skip_all, target = "ConsensusTaskState", fields(id = self.id, view = *self.cur_view))]
    async fn publish_proposal(
        &mut self,
        view: TYPES::Time,
        event_stream: Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> Result<()> {
        let create_and_send_proposal_handle = publish_proposal_if_able(
            view,
            event_stream,
            Arc::clone(&self.quorum_membership),
            self.public_key.clone(),
            self.private_key.clone(),
            OuterConsensus::new(Arc::clone(&self.consensus.inner_consensus)),
            self.round_start_delay,
            self.formed_upgrade_certificate.clone(),
            self.upgrade_lock.clone(),
            self.payload_commitment_and_metadata.clone(),
            self.proposal_cert.clone(),
            Arc::clone(&self.instance_state),
            self.id,
        )
        .await?;

        self.spawned_tasks
            .entry(view)
            .or_default()
            .push(create_and_send_proposal_handle);

        Ok(())
    }

    /// Spawn a vote task for the given view.  Will try to vote
    /// and emit a `QuorumVoteSend` event we should vote on the current proposal
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), target = "ConsensusTaskState")]
    async fn spawn_vote_task(
        &mut self,
        view: TYPES::Time,
        event_stream: Sender<Arc<HotShotEvent<TYPES>>>,
    ) {
        let Some(proposal) = self.current_proposal.clone() else {
            return;
        };
        if proposal.view_number() != view {
            return;
        }
        let upgrade = self.upgrade_lock.clone();
        let pub_key = self.public_key.clone();
        let priv_key = self.private_key.clone();
        let consensus = OuterConsensus::new(Arc::clone(&self.consensus.inner_consensus));
        let storage = Arc::clone(&self.storage);
        let quorum_mem = Arc::clone(&self.quorum_membership);
        let da_mem = Arc::clone(&self.da_membership);
        let instance_state = Arc::clone(&self.instance_state);
        let id = self.id;
        let handle = async_spawn(async move {
            update_state_and_vote_if_able::<TYPES, I, V>(
                view,
                proposal,
                pub_key,
                consensus,
                storage,
                quorum_mem,
                instance_state,
                (priv_key, upgrade, da_mem, event_stream),
                id,
            )
            .await;
        });
        self.spawned_tasks.entry(view).or_default().push(handle);
    }

    /// Handles a consensus event received on the event stream
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "Consensus replica task", level = "error", target = "ConsensusTaskState")]
    pub async fn handle(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        event_stream: Sender<Arc<HotShotEvent<TYPES>>>,
    ) {
        match event.as_ref() {
            HotShotEvent::QuorumProposalRecv(proposal, sender) => {
                debug!("proposal recv view: {:?}", proposal.data.view_number());
                match handle_quorum_proposal_recv(proposal, sender, event_stream.clone(), self)
                    .await
                {
                    Ok(Some(current_proposal)) => {
                        let view = current_proposal.view_number();
                        self.current_proposal = Some(current_proposal);
                        self.spawn_vote_task(view, event_stream).await;
                    }
                    Ok(None) => {}
                    Err(e) => debug!("Failed to propose {e:#}"),
                }
            }
            HotShotEvent::QuorumProposalValidated(proposal, _) => {
                debug!("proposal validated view: {:?}", proposal.view_number());
                if let Err(e) =
                    handle_quorum_proposal_validated(proposal, event_stream.clone(), self).await
                {
                    warn!("Failed to handle QuorumProposalValidated event {e:#}");
                }
            }
            HotShotEvent::QuorumVoteRecv(ref vote) => {
                let mut retries = 0;
                // This is a workaround: we might have already received a vote for a leaf that we haven't yet seen in a proposal.
                let vote_view_number = loop {
                    if let Some(view_number) =
                        self.consensus.read().await.quorum_vote_view_number(vote)
                    {
                        break view_number;
                    }
                    if retries > 5 {
                        warn!("We have received a Quorum vote but we haven't seen this leaf yet!");
                        return;
                    }
                    async_sleep(Duration::from_millis(10)).await;
                    retries += 1;
                };
                debug!("Received quorum vote: {:?}", vote_view_number);
                if self.quorum_membership.leader(vote_view_number + 1) != self.public_key {
                    error!(
                        "We are not the leader for view {} are we the leader for view + 1? {}",
                        *vote_view_number + 1,
                        self.quorum_membership.leader(vote_view_number + 2) == self.public_key
                    );
                    return;
                }
                let mut collector = self.vote_collector.write().await;

                if collector.is_none() || vote_view_number > collector.as_ref().unwrap().view {
                    debug!("Starting vote handle for view {:?}", vote_view_number);
                    let info = AccumulatorInfo {
                        public_key: self.public_key.clone(),
                        membership: Arc::clone(&self.quorum_membership),
                        view: vote_view_number,
                        id: self.id,
                    };
                    *collector = create_vote_accumulator::<
                        TYPES,
                        QuorumVote<TYPES>,
                        QuorumCertificate<TYPES>,
                    >(&info, event, &event_stream)
                    .await;
                } else {
                    let result = collector
                        .as_mut()
                        .unwrap()
                        .handle_vote_event(Arc::clone(&event), &event_stream)
                        .await;

                    if result == Some(HotShotTaskCompleted) {
                        *collector = None;
                        // The protocol has finished
                        return;
                    }
                }
            }
            HotShotEvent::TimeoutVoteRecv(ref vote) => {
                if self.timeout_membership.leader(vote.view_number() + 1) != self.public_key {
                    error!(
                        "We are not the leader for view {} are we the leader for view + 1? {}",
                        *vote.view_number() + 1,
                        self.timeout_membership.leader(vote.view_number() + 2) == self.public_key
                    );
                    return;
                }
                let mut collector = self.timeout_vote_collector.write().await;

                if collector.is_none() || vote.view_number() > collector.as_ref().unwrap().view {
                    debug!("Starting vote handle for view {:?}", vote.view_number());
                    let info = AccumulatorInfo {
                        public_key: self.public_key.clone(),
                        membership: Arc::clone(&self.quorum_membership),
                        view: vote.view_number(),
                        id: self.id,
                    };
                    *collector = create_vote_accumulator::<
                        TYPES,
                        TimeoutVote<TYPES>,
                        TimeoutCertificate<TYPES>,
                    >(&info, event, &event_stream)
                    .await;
                } else {
                    let result = collector
                        .as_mut()
                        .unwrap()
                        .handle_vote_event(Arc::clone(&event), &event_stream)
                        .await;

                    if result == Some(HotShotTaskCompleted) {
                        *collector = None;
                        // The protocol has finished
                        return;
                    }
                }
            }
            HotShotEvent::QcFormed(cert) => match cert {
                either::Right(qc) => {
                    self.proposal_cert = Some(ViewChangeEvidence::Timeout(qc.clone()));

                    debug!(
                        "Attempting to publish proposal after forming a TC for view {}",
                        *qc.view_number()
                    );

                    if let Err(e) = self
                        .publish_proposal(qc.view_number() + 1, event_stream)
                        .await
                    {
                        debug!("Failed to propose; error = {e:?}");
                    };
                }
                either::Left(qc) => {
                    let mut retries = 5;
                    let qc_view_number = loop {
                        if let Some(qc_view_number) = self.consensus.read().await.qc_view_number(qc)
                        {
                            break qc_view_number;
                        }
                        if retries < 1 {
                            warn!("We haven't seen this leaf yet!");
                            return;
                        }
                        retries -= 1;
                        let _ = fetch_proposal(
                            qc,
                            event_stream.clone(),
                            Arc::clone(&self.quorum_membership),
                            OuterConsensus::new(Arc::clone(&self.consensus.inner_consensus)),
                        )
                        .await;
                    };
                    if let Err(e) = self.storage.write().await.update_high_qc(qc.clone()).await {
                        error!("Failed to store High QC of QC we formed. Error: {:?}", e);
                    }

                    if let Err(e) = self.consensus.write().await.update_high_qc(qc.clone()) {
                        tracing::error!("{e:?}");
                    }
                    debug!(
                        "Attempting to publish proposal after forming a QC for view {}",
                        *qc_view_number
                    );

                    if let Err(e) = self
                        .publish_proposal(qc_view_number + 1, event_stream)
                        .await
                    {
                        debug!("Failed to propose; error = {e:?}");
                    };
                }
            },
            #[cfg(not(feature = "dependency-tasks"))]
            HotShotEvent::UpgradeCertificateFormed(cert) => {
                debug!(
                    "Upgrade certificate received for view {}!",
                    *cert.view_number()
                );

                // Update our current upgrade_cert as long as we still have a chance of reaching a decide on it in time.
                if cert.data().decide_by >= self.cur_view + 3 {
                    debug!("Updating current formed_upgrade_certificate");

                    self.formed_upgrade_certificate = Some(cert.clone());
                }
            }
            HotShotEvent::DaCertificateRecv(cert) => {
                // let cert_view_number = cert.view_number();
                let mut retries = 0;
                // This is a workaround: we might have already received a DAC for VID that we haven't yet seen.
                let cert_view_number = loop {
                    if let Some(view_number) = self.consensus.read().await.dac_view_number(cert) {
                        break view_number;
                    }
                    if retries > 5 {
                        warn!(
                            "We have received a DAC but we haven't seen this VID commitment yet!"
                        );
                        return;
                    }
                    async_sleep(Duration::from_millis(10)).await;
                    retries += 1;
                };

                debug!("DAC Received for view {}!", *cert_view_number);

                self.consensus
                    .write()
                    .await
                    .update_saved_da_certs(cert_view_number, cert.clone());
                let Some(proposal) = self.current_proposal.clone() else {
                    return;
                };
                if proposal.view_number() != cert_view_number {
                    return;
                }
                self.spawn_vote_task(cert_view_number, event_stream).await;
            }
            HotShotEvent::VidShareRecv(disperse) => {
                let view = disperse.data.view_number();

                debug!(
                    "VID disperse received for view: {:?} in consensus task",
                    view
                );

                // Allow VID disperse date that is one view older, in case we have updated the
                // view.
                // Adding `+ 1` on the LHS rather than `- 1` on the RHS, to avoid the overflow
                // error due to subtracting the genesis view number.
                if view + 1 < self.cur_view {
                    info!("Throwing away VID disperse data that is more than one view older");
                    return;
                }

                debug!("VID disperse data is not more than one view older.");

                if !self.validate_disperse(disperse) {
                    warn!("Failed to validated the VID dispersal/share sig.");
                    return;
                }

                self.consensus
                    .write()
                    .await
                    .update_vid_shares(view, disperse.clone());
                self.consensus
                    .write()
                    .await
                    .update_vid_commit_view(disperse.data.payload_commitment, view);
                if disperse.data.recipient_key != self.public_key {
                    return;
                }
                let Some(proposal) = self.current_proposal.clone() else {
                    return;
                };
                if proposal.view_number() != view {
                    return;
                }
                self.spawn_vote_task(view, event_stream.clone()).await;
            }
            HotShotEvent::ViewChange(new_view) => {
                let new_view = *new_view;
                tracing::trace!("View Change event for view {} in consensus task", *new_view);

                let old_view_number = self.cur_view;

                // If we have a decided upgrade certificate, the protocol version may also have
                // been upgraded.
                if let Some(cert) = self
                    .upgrade_lock
                    .decided_upgrade_certificate
                    .read()
                    .await
                    .clone()
                {
                    if new_view == cert.data().new_version_first_view {
                        error!(
                            "Version upgraded based on a decided upgrade cert: {:?}",
                            cert
                        );
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
                if let Err(e) = update_view::<TYPES>(
                    new_view,
                    &event_stream,
                    self.timeout,
                    OuterConsensus::new(Arc::clone(&self.consensus.inner_consensus)),
                    &mut self.cur_view,
                    &mut self.cur_view_time,
                    &mut self.timeout_task,
                    &self.output_event_stream,
                    DONT_SEND_VIEW_CHANGE_EVENT,
                    self.quorum_membership.leader(old_view_number) == self.public_key,
                )
                .await
                {
                    tracing::trace!("Failed to update view; error = {e}");
                    return;
                }
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
                if self.quorum_membership.leader(view) == self.public_key {
                    consensus.metrics.number_of_timeouts_as_leader.add(1);
                }
            }
            HotShotEvent::SendPayloadCommitmentAndMetadata(
                payload_commitment,
                builder_commitment,
                metadata,
                view,
                fees,
                auction_result,
            ) => {
                let view = *view;
                debug!(
                    "got commit and meta {:?}, view {:?}",
                    payload_commitment, view
                );
                self.payload_commitment_and_metadata = Some(CommitmentAndMetadata {
                    commitment: *payload_commitment,
                    builder_commitment: builder_commitment.clone(),
                    metadata: metadata.clone(),
                    fees: fees.clone(),
                    block_view: view,
                    auction_result: auction_result.clone(),
                });
                let Some(high_qc_view_number) = self.consensus.read().await.high_qc_view_number()
                else {
                    warn!("We haven't seen this leaf yet!");
                    return;
                };
                if self.quorum_membership.leader(view) == self.public_key
                    && high_qc_view_number + 1 == view
                {
                    if let Err(e) = self.publish_proposal(view, event_stream.clone()).await {
                        error!("Failed to propose; error = {e:?}");
                    };
                }

                if let Some(cert) = &self.proposal_cert {
                    if !cert.is_valid_for_view(&view) {
                        self.proposal_cert = None;
                        info!("Failed to propose off SendPayloadCommitmentAndMetadata because we had view change evidence, but it was not current.");
                        return;
                    }
                    match cert {
                        ViewChangeEvidence::Timeout(tc) => {
                            if self.quorum_membership.leader(tc.view_number() + 1)
                                == self.public_key
                            {
                                if let Err(e) = self.publish_proposal(view, event_stream).await {
                                    debug!("Failed to propose; error = {e:?}");
                                };
                            }
                        }
                        ViewChangeEvidence::ViewSync(vsc) => {
                            if self.quorum_membership.leader(vsc.view_number()) == self.public_key {
                                if let Err(e) = self.publish_proposal(view, event_stream).await {
                                    debug!("Failed to propose; error = {e:?}");
                                };
                            }
                        }
                    }
                }
            }
            HotShotEvent::ViewSyncFinalizeCertificate2Recv(certificate) => {
                if !certificate.is_valid_cert(self.quorum_membership.as_ref()) {
                    error!(
                        "View Sync Finalize certificate {:?} was invalid",
                        certificate.data()
                    );
                    return;
                }

                let view = certificate.view_number();

                if self.quorum_membership.leader(view) == self.public_key {
                    self.proposal_cert = Some(ViewChangeEvidence::ViewSync(certificate.clone()));

                    debug!(
                        "Attempting to publish proposal after forming a View Sync Finalized Cert for view {}",
                        *certificate.view_number()
                    );

                    if let Err(e) = self.publish_proposal(view, event_stream).await {
                        debug!("Failed to propose; error = {e:?}");
                    };
                }
            }
            HotShotEvent::QuorumVoteSend(vote) => {
                let Some(proposal) = self.current_proposal.clone() else {
                    return;
                };
                let new_view = proposal.view_number() + 1;
                // In future we can use the mempool model where we fetch the proposal if we don't have it, instead of having to wait for it here
                // This is for the case where we form a QC but have not yet seen the previous proposal ourselves
                let Some(high_qc_view_number) = self.consensus.read().await.high_qc_view_number()
                else {
                    warn!("We haven't seen this leaf yet!");
                    return;
                };
                let should_propose = self.quorum_membership.leader(new_view) == self.public_key
                    && high_qc_view_number == proposal.view_number();

                if should_propose {
                    debug!(
                        "Attempting to publish proposal after voting; now in view: {}",
                        *new_view
                    );
                    if let Err(e) = self.publish_proposal(new_view, event_stream.clone()).await {
                        debug!("failed to propose e = {:?}", e);
                    }
                }
                if proposal.view_number() <= vote.view_number() {
                    self.current_proposal = None;
                }
            }
            HotShotEvent::QuorumProposalSend(proposal, _) => {
                if self
                    .payload_commitment_and_metadata
                    .as_ref()
                    .is_some_and(|p| p.block_view <= proposal.data.view_number())
                {
                    self.payload_commitment_and_metadata = None;
                }
                if let Some(cert) = &self.proposal_cert {
                    let view = match cert {
                        ViewChangeEvidence::Timeout(tc) => tc.view_number() + 1,
                        ViewChangeEvidence::ViewSync(vsc) => vsc.view_number(),
                    };
                    if view < proposal.data.view_number() {
                        self.proposal_cert = None;
                    }
                }
            }
            _ => {}
        }
    }
}

#[async_trait]
impl<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions> TaskState
    for ConsensusTaskState<TYPES, I, V>
{
    type Event = HotShotEvent<TYPES>;

    async fn handle_event(
        &mut self,
        event: Arc<Self::Event>,
        sender: &Sender<Arc<Self::Event>>,
        _receiver: &Receiver<Arc<Self::Event>>,
    ) -> Result<()> {
        self.handle(event, sender.clone()).await;

        Ok(())
    }

    async fn cancel_subtasks(&mut self) {
        while !self.spawned_tasks.is_empty() {
            let Some((_, handles)) = self.spawned_tasks.pop_first() else {
                break;
            };

            for handle in handles {
                #[cfg(async_executor_impl = "async-std")]
                handle.cancel().await;
                #[cfg(async_executor_impl = "tokio")]
                handle.abort();
            }
        }
    }
}
