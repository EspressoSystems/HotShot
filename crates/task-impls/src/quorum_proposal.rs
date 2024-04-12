use crate::{
    consensus::{create_and_send_proposal, validate_proposal},
    helpers::AnyhowTracing,
};
use std::{collections::HashMap, marker::PhantomData, sync::Arc, time::Duration};

use anyhow::{bail, ensure, Context, Result};
use async_broadcast::{Receiver, Sender};
use async_compatibility_layer::art::{async_sleep, async_spawn};
use async_lock::{RwLock, RwLockUpgradableReadGuard};
use bitvec::ptr::null;
use committable::Committable;
use either::Either;
use futures::future::FutureExt;
use hotshot_task::{
    dependency::{AndDependency, EventDependency, OrDependency},
    dependency_task::{DependencyTask, HandleDepOutput},
    task::{Task, TaskState},
};
use hotshot_types::{
    consensus::Consensus,
    constants::LOOK_AHEAD,
    data::{null_block, Leaf, QuorumProposal, ViewChangeEvidence, ViewNumber},
    event::{Event, EventType, LeafInfo},
    message::Proposal,
    simple_certificate::{SimpleCertificate, UpgradeCertificate},
    traits::{
        block_contents::BlockHeader,
        election::Membership,
        network::{ConnectedNetwork, ConsensusIntentEvent},
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
        signature_key::SignatureKey,
        states::ValidatedState,
        storage::Storage,
        BlockPayload,
    },
    utils::{View, ViewInner},
    vote::{Certificate, HasViewNumber},
};

#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
#[cfg(async_executor_impl = "tokio")]
use tokio::task::JoinHandle;
use tracing::{debug, error, info, instrument, warn};

use crate::{
    consensus::CommitmentAndMetadata,
    events::HotShotEvent,
    helpers::{broadcast_event, cancel_task},
};

/// Proposal dependency types. These types represent events that precipitate a proposal.
#[derive(PartialEq, Debug)]
enum ProposalDependency {
    /// For the `SendPayloadCommitmentAndMetadata` event.
    PayloadAndMetadata,

    /// For the `QCFormed` event.
    QC,

    /// For the `ViewSyncFinalizeCertificate2Recv` event.
    ViewSyncCert,

    /// For the `QCFormed` event timeout branch.
    TimeoutCert,

    /// For the `QuroumProposalRecv` event.
    Proposal,
}

/// Handler for the proposal dependency
struct ProposalDependencyHandle<TYPES: NodeType> {
    /// The view number to propose for.
    view_number: TYPES::Time,

    /// The event sender.
    sender: Sender<Arc<HotShotEvent<TYPES>>>,

    /// Reference to consensus. The replica will require a write lock on this.
    consensus: Arc<RwLock<Consensus<TYPES>>>,

    /// Output events to application
    #[allow(dead_code)]
    output_event_stream: async_broadcast::Sender<Event<TYPES>>,

    /// Membership for Timeout votes/certs
    #[allow(dead_code)]
    timeout_membership: Arc<TYPES::Membership>,

    /// Membership for Quorum Certs/votes
    quorum_membership: Arc<TYPES::Membership>,

    /// Our public key
    public_key: TYPES::SignatureKey,

    /// Our Private Key
    private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,

    /// View timeout from config.
    #[allow(dead_code)]
    timeout: u64,

    /// Round start delay from config, in milliseconds.
    round_start_delay: u64,

    /// The node's id
    #[allow(dead_code)]
    id: u64,
}

impl<TYPES: NodeType> ProposalDependencyHandle<TYPES> {
    /// Publishes a propoal if an upgrade certificate is valid within the internal consensus storage.
    ///
    /// # Errors
    /// Can return an error in a number of cases:
    /// 1. The cert is not in the interim period.
    /// 2. The null block failed to be computed (payload or commitment).
    async fn propose_from_upgrade(
        &self,
        view: TYPES::Time,
        event_stream: &Sender<Arc<HotShotEvent<TYPES>>>,
        parent_leaf: Leaf<TYPES>,
        state: Arc<<TYPES as NodeType>::ValidatedState>,
        upgrade_cert: UpgradeCertificate<TYPES>,
    ) -> Result<()> {
        ensure!(
            upgrade_cert.in_interim(self.view_number),
            format!(
                "View {:?} is not in the interim period, skipping proposal",
                self.view_number
            )
        );

        let (_payload, metadata) =
            <TYPES::BlockPayload as BlockPayload>::from_transactions(Vec::new())
                .context("Failed to build null block payload and metadata")?;

        let Some(null_block_commitment) =
            null_block::commitment(self.quorum_membership.total_nodes())
        else {
            bail!("Failed to calculate the null block commitment");
        };

        let pub_key = self.public_key.clone();
        let priv_key = self.private_key.clone();
        let consensus = self.consensus.clone();
        let sender = event_stream.clone();
        let delay = self.round_start_delay;
        let parent = parent_leaf.clone();
        let state = state.clone();
        create_and_send_proposal(
            pub_key,
            priv_key,
            consensus,
            sender,
            view,
            null_block_commitment,
            metadata,
            parent,
            state,
            Some(upgrade_cert),
            None,
            delay,
        )
        .await;

        Ok(())
    }

    async fn propose_from_payload_commitment_and_metadata(
        &self,
        view: TYPES::Time,
        event_stream: &Sender<Arc<HotShotEvent<TYPES>>>,
        parent_leaf: Leaf<TYPES>,
        state: Arc<<TYPES as NodeType>::ValidatedState>,
        commit_and_metadata: CommitmentAndMetadata<TYPES::BlockPayload>,
        proposal_cert: Option<ViewChangeEvidence<TYPES>>,
    ) -> Result<()> {
        let mut consensus = self.consensus.write().await;

        // In order of priority, we should try to attach:
        //   - the parent certificate if it exists, or
        //   - our own certificate that we formed.
        // In either case, we need to ensure that the certificate is still relevant.
        //
        // Note: once we reach a point of potentially propose with our formed upgrade certificate, we will ALWAYS drop it. If we cannot immediately use it for whatever reason, we choose to discard it.
        // It is possible that multiple nodes form separate upgrade certificates for the some upgrade if we are not careful about voting. But this shouldn't bother us: the first leader to propose is the one whose certificate will be used. And if that fails to reach a decide for whatever reason, we may lose our own certificate, but something will likely have gone wrong there anyway.
        let formed_upgrade_certificate = consensus.formed_upgrade_certificate.take();
        let mut proposal_upgrade_certificate = parent_leaf
            .get_upgrade_certificate()
            .or(formed_upgrade_certificate);

        if !proposal_upgrade_certificate.clone().is_some_and(|cert| {
            cert.is_relevant(view, consensus.decided_upgrade_cert.clone())
                .is_ok()
        }) {
            proposal_upgrade_certificate = None;
        }

        // We only want to proposal to be attached if any of them are valid.
        let proposal_certificate = proposal_cert
            .as_ref()
            .filter(|cert| cert.is_valid_for_view(&view))
            .cloned();

        let pub_key = self.public_key.clone();
        let priv_key = self.private_key.clone();
        let consensus = self.consensus.clone();
        let sender = event_stream.clone();
        let commit = commit_and_metadata.commitment;
        let metadata = commit_and_metadata.metadata.clone();
        let state = state.clone();
        let delay = self.round_start_delay;
        create_and_send_proposal(
            pub_key,
            priv_key,
            consensus,
            sender,
            view,
            commit,
            metadata,
            parent_leaf.clone(),
            state,
            proposal_upgrade_certificate,
            proposal_certificate,
            delay,
        )
        .await;

        Ok(())
    }

    /// Sends a proposal if possible from the high qc we have
    #[allow(clippy::too_many_lines)]
    pub async fn publish_proposal_if_able(
        &self,
        view: TYPES::Time,
        event_stream: &Sender<Arc<HotShotEvent<TYPES>>>,
        commit_and_metadata: CommitmentAndMetadata<TYPES::BlockPayload>,
        proposal_certificate: Option<ViewChangeEvidence<TYPES>>,
    ) -> Result<()> {
        ensure!(
            self.quorum_membership.get_leader(view) == self.public_key,
            format!(
                "Somehow we formed a QC, but we are not the leader for the next view: {view:?}"
            ),
        );
        let consensus = self.consensus.read().await;
        let parent_view_number = &consensus.high_qc.get_view_number();
        let mut reached_decided = false;

        let parent_view = consensus
            .validated_state_map
            .get(parent_view_number)
            .context(format!(
                "Couldn't find parent view in state map, parent view number: {}",
                **parent_view_number
            ))?;
        let (leaf_commitment, state) = parent_view.get_leaf_and_state().context(format!(
            "Parent view ({:?}, {:?}) of high QC points to a view without a proposal",
            parent_view_number, parent_view
        ))?;

        if leaf_commitment != consensus.high_qc.get_data().leaf_commit {
            // NOTE: This happens on the genesis block
            debug!(
                "They don't equal: {:?}   {:?}",
                leaf_commitment,
                consensus.high_qc.get_data().leaf_commit
            );
        }
        let leaf = consensus
            .saved_leaves
            .get(&leaf_commitment)
            .context("Failed to find high QC of parent")?;

        if leaf.get_view_number() == consensus.last_decided_view {
            reached_decided = true;
        }

        let parent_leaf = leaf.clone();

        let original_parent_hash = parent_leaf.commit();

        let mut next_parent_hash = original_parent_hash;

        // Walk back until we find a decide
        if !reached_decided {
            debug!(
                "We have not reached decide from view {:?}",
                self.view_number
            );
            while let Some(next_parent_leaf) = consensus.saved_leaves.get(&next_parent_hash) {
                if next_parent_leaf.get_view_number() <= consensus.last_decided_view {
                    break;
                }
                next_parent_hash = next_parent_leaf.get_parent_commitment();
            }
            debug!("updated saved leaves");
            // TODO do some sort of sanity check on the view number that it matches decided
        }

        match &consensus.decided_upgrade_cert {
            Some(upgrade_cert) => {
                self.propose_from_upgrade(
                    view,
                    event_stream,
                    parent_leaf,
                    state.clone(),
                    upgrade_cert.clone(),
                )
                .await
            }
            None => {
                self.propose_from_payload_commitment_and_metadata(
                    view,
                    event_stream,
                    parent_leaf,
                    state.clone(),
                    commit_and_metadata,
                    proposal_certificate,
                )
                .await
            }
        }
    }
}

impl<TYPES: NodeType> HandleDepOutput for ProposalDependencyHandle<TYPES> {
    type Output = Vec<Vec<Arc<HotShotEvent<TYPES>>>>;

    #[allow(clippy::no_effect_underscore_binding)]
    async fn handle_dep_result(self, res: Self::Output) {
        let mut payload_commitment = None;
        let mut commit_and_metadata: Option<CommitmentAndMetadata<TYPES::BlockPayload>> = None;
        let mut _quorum_certificate = None;
        let mut _timeout_certificate = None;
        let mut _view_sync_finalize_cert = None;
        for event in res.iter().flatten() {
            match event.as_ref() {
                HotShotEvent::QuorumProposalValidated(proposal, _) => {
                    let proposal_payload_comm = proposal.block_header.payload_commitment();
                    if let Some(comm) = payload_commitment {
                        if proposal_payload_comm != comm {
                            return;
                        }
                    } else {
                        payload_commitment = Some(proposal_payload_comm);
                    }
                }
                HotShotEvent::SendPayloadCommitmentAndMetadata(
                    payload_commitment,
                    metadata,
                    _view,
                ) => {
                    debug!("Got commit and meta {:?}", payload_commitment);
                    commit_and_metadata = Some(CommitmentAndMetadata {
                        commitment: *payload_commitment,
                        metadata: metadata.clone(),
                    });
                }
                HotShotEvent::QCFormed(cert) => match cert {
                    either::Right(timeout) => {
                        _timeout_certificate = Some(timeout.clone());
                    }
                    either::Left(qc) => {
                        _quorum_certificate = Some(qc.clone());
                    }
                },
                HotShotEvent::ViewSyncFinalizeCertificate2Recv(cert) => {
                    _view_sync_finalize_cert = Some(cert.clone());
                }
                _ => {}
            }
        }

        if commit_and_metadata.is_none() {
            error!(
                "Somehow completed the proposal dependency task without a commitment and metadata"
            );
            return;
        }

        let proposal_certificate = {
            if let Some(tc) = _timeout_certificate {
                Some(ViewChangeEvidence::Timeout(tc))
            } else if let Some(vsc) = _view_sync_finalize_cert {
                Some(ViewChangeEvidence::ViewSync(vsc))
            } else {
                None
            }
        };

        if let Err(e) = self
            .publish_proposal_if_able(
                self.view_number,
                &self.sender,
                commit_and_metadata.unwrap(),
                proposal_certificate,
            )
            .await
        {
            error!("Failed to publish proposal; error = {:?}", e);
        }
    }
}

/// The state for the quorum proposal task.
pub struct QuorumProposalTaskState<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Latest view number that has been proposed for.
    pub latest_proposed_view: TYPES::Time,

    /// Table for the in-progress proposal depdencey tasks.
    pub propose_dependencies: HashMap<TYPES::Time, JoinHandle<()>>,

    /// Network for all nodes
    pub quorum_network: Arc<I::QuorumNetwork>,

    /// Network for DA committee
    pub committee_network: Arc<I::CommitteeNetwork>,

    /// Output events to application
    pub output_event_stream: async_broadcast::Sender<Event<TYPES>>,

    /// Reference to consensus. The replica will require a write lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES>>>,

    /// Membership for Timeout votes/certs
    pub timeout_membership: Arc<TYPES::Membership>,

    /// Membership for Quorum Certs/votes
    pub quorum_membership: Arc<TYPES::Membership>,

    /// Our public key
    pub public_key: TYPES::SignatureKey,

    /// Our Private Key
    pub private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,

    /// View timeout from config.
    pub timeout: u64,

    /// Round start delay from config, in milliseconds.
    pub round_start_delay: u64,

    /// The view number that this node is executing in.
    pub cur_view: TYPES::Time,

    /// timeout task handle
    pub timeout_task: Option<JoinHandle<()>>,

    /// This node's storage ref
    pub storage: Arc<RwLock<I::Storage>>,

    // /// most recent decided upgrade certificate
    // pub decided_upgrade_cert: Option<UpgradeCertificate<TYPES>>,
    /// The node's id
    pub id: u64,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> QuorumProposalTaskState<TYPES, I> {
    /// Create an event dependency
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view, view = *self.cur_view), name = "Quorum proposal create event dependency", level = "error")]
    fn create_event_dependency(
        &self,
        dependency_type: ProposalDependency,
        view_number: TYPES::Time,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
    ) -> EventDependency<Arc<HotShotEvent<TYPES>>> {
        EventDependency::new(
            event_receiver,
            Box::new(move |event| {
                let event = event.as_ref();
                let event_view = match dependency_type {
                    ProposalDependency::QC => {
                        if let HotShotEvent::QCFormed(either::Left(qc)) = event {
                            warn!(
                                "QC View number {} View number {}",
                                *qc.view_number, *view_number
                            );
                            qc.view_number + 1
                        } else {
                            return false;
                        }
                    }
                    ProposalDependency::TimeoutCert => {
                        if let HotShotEvent::QCFormed(either::Right(timeout)) = event {
                            timeout.view_number
                        } else {
                            return false;
                        }
                    }
                    ProposalDependency::ViewSyncCert => {
                        if let HotShotEvent::ViewSyncFinalizeCertificate2Recv(view_sync_cert) =
                            event
                        {
                            view_sync_cert.view_number
                        } else {
                            return false;
                        }
                    }

                    ProposalDependency::Proposal => {
                        if let HotShotEvent::QuorumProposalValidated(proposal, _) = event {
                            proposal.view_number
                        } else {
                            return false;
                        }
                    }
                    ProposalDependency::PayloadAndMetadata => {
                        if let HotShotEvent::SendPayloadCommitmentAndMetadata(
                            _payload_commitment,
                            _metadata,
                            view,
                        ) = event
                        {
                            *view
                        } else {
                            return false;
                        }
                    }
                };
                let valid = event_view == view_number;
                if valid {
                    info!("Dependency {:?} is complete!", dependency_type);
                }
                valid
            }),
        )
    }

    /// Create and store an [`AndDependency`] combining [`EventDependency`]s associated with the
    /// given view number if it doesn't exist. Also takes in the received `event` to seed a
    /// dependency as already completed. This allows for the task to receive a proposable event
    /// without losing the data that it received, as the dependency task would otherwise have no
    /// ability to receive the event and, thus, would never propose.
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view, view = *self.cur_view), name = "Quorum proposal create dependency task", level = "error")]
    fn create_dependency_task_if_new(
        &mut self,
        view_number: TYPES::Time,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
        event_sender: Sender<Arc<HotShotEvent<TYPES>>>,
        event: Arc<HotShotEvent<TYPES>>,
    ) {
        info!("Attempting to make dependency task for event {:?}", event);
        if self.propose_dependencies.get(&view_number).is_some() {
            debug!("Task already exists");
            return;
        }

        let mut proposal_dependency = self.create_event_dependency(
            ProposalDependency::Proposal,
            view_number,
            event_receiver.clone(),
        );

        let mut qc_dependency = self.create_event_dependency(
            ProposalDependency::QC,
            view_number,
            event_receiver.clone(),
        );

        let mut view_sync_dependency = self.create_event_dependency(
            ProposalDependency::ViewSyncCert,
            view_number,
            event_receiver.clone(),
        );

        let mut timeout_dependency = self.create_event_dependency(
            ProposalDependency::TimeoutCert,
            view_number,
            event_receiver.clone(),
        );

        let mut payload_commitment_dependency = self.create_event_dependency(
            ProposalDependency::PayloadAndMetadata,
            view_number,
            event_receiver,
        );

        match event.as_ref() {
            HotShotEvent::SendPayloadCommitmentAndMetadata(..) => {
                payload_commitment_dependency.mark_as_completed(event.clone());
                info!(
                    "Node {} Dependency PayloadAndMetadata is complete for view  {}!",
                    self.id, *view_number
                );
            }
            HotShotEvent::QuorumProposalValidated(..) => {
                proposal_dependency.mark_as_completed(event);
                info!(
                    "Node {} Dependency Proposal is complete for view {}!",
                    self.id, *view_number
                );
            }
            HotShotEvent::QCFormed(quorum_certificate) => match quorum_certificate {
                Either::Right(_) => {
                    timeout_dependency.mark_as_completed(event);
                    info!(
                        "Node {} Dependency TimeoutCert is complete for view {}!",
                        self.id, *view_number
                    );
                }
                Either::Left(_) => {
                    qc_dependency.mark_as_completed(event);
                    info!(
                        "Node {} Dependency QC is complete for view {}!",
                        self.id, *view_number
                    );
                }
            },
            HotShotEvent::ViewSyncFinalizeCertificate2Recv(_) => {
                view_sync_dependency.mark_as_completed(event);
                info!(
                    "Node {} Dependency ViewSyncCert is complete for view {}!",
                    self.id, *view_number
                );
            }
            _ => {}
        };

        // We have three cases to consider:
        let combined = if *view_number > 1 {
            AndDependency::from_deps(vec![
                OrDependency::from_deps(vec![AndDependency::from_deps(vec![
                    payload_commitment_dependency,
                ])]),
                OrDependency::from_deps(vec![
                    // 1. A QCFormed event and QuorumProposalValidated event
                    AndDependency::from_deps(vec![qc_dependency, proposal_dependency]),
                    // 2. A timeout cert was received
                    AndDependency::from_deps(vec![timeout_dependency]),
                    // 3. A view sync cert was received.
                    AndDependency::from_deps(vec![view_sync_dependency]),
                ]),
            ])
        } else {
            AndDependency::from_deps(vec![
                OrDependency::from_deps(vec![AndDependency::from_deps(vec![
                    payload_commitment_dependency,
                ])]),
                OrDependency::from_deps(vec![
                    // 1. A QCFormed event and QuorumProposalValidated event
                    AndDependency::from_deps(vec![qc_dependency]),
                    // 2. A timeout cert was received
                    AndDependency::from_deps(vec![timeout_dependency]),
                    // 3. A view sync cert was received.
                    AndDependency::from_deps(vec![view_sync_dependency]),
                ]),
            ])
        };

        let dependency_task = DependencyTask::new(
            combined,
            ProposalDependencyHandle {
                view_number,
                sender: event_sender.clone(),
                consensus: self.consensus.clone(),
                output_event_stream: self.output_event_stream.clone(),
                timeout_membership: self.timeout_membership.clone(),
                quorum_membership: self.quorum_membership.clone(),
                public_key: self.public_key.clone(),
                private_key: self.private_key.clone(),
                timeout: self.timeout,
                round_start_delay: self.round_start_delay,
                id: self.id,
            },
        );
        self.propose_dependencies
            .insert(view_number, dependency_task.run());
    }

    /// Update the latest proposed view number.
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view), name = "Quorum proposal update latest proposed view", level = "error")]
    async fn update_latest_proposed_view(&mut self, new_view: TYPES::Time) -> bool {
        if *self.latest_proposed_view < *new_view {
            info!(
                "Updating next proposal view from {} to {} in the quorum proposal task",
                *self.latest_proposed_view, *new_view
            );

            // Cancel the old dependency tasks.
            for view in (*self.latest_proposed_view + 1)..=(*new_view) {
                if let Some(dependency) = self.propose_dependencies.remove(&TYPES::Time::new(view))
                {
                    cancel_task(dependency).await;
                }
            }

            self.latest_proposed_view = new_view;

            return true;
        }
        false
    }

    /// Handles a consensus event received on the event stream
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view), name = "Quorum proposal handle", level = "error")]
    pub async fn handle(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
        event_sender: Sender<Arc<HotShotEvent<TYPES>>>,
    ) {
        match event.as_ref() {
            HotShotEvent::QCFormed(cert) => {
                match cert.clone() {
                    either::Right(timeout_cert) => {
                        // cancel poll for votes
                        self.quorum_network
                            .inject_consensus_info(ConsensusIntentEvent::CancelPollForVotes(
                                *timeout_cert.view_number,
                            ))
                            .await;

                        let view = timeout_cert.view_number + 1;
                        info!("Making QC Timeout dependency for view {view:?}");

                        self.create_dependency_task_if_new(
                            view,
                            event_receiver,
                            event_sender,
                            event.clone(),
                        );
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

                        // We need to drop our handle here to make the borrow checker happy.
                        drop(consensus);

                        let view = qc.view_number + 1;
                        info!("Making QC Dependency for view {view:?}");

                        self.create_dependency_task_if_new(
                            view,
                            event_receiver,
                            event_sender,
                            event.clone(),
                        );
                    }
                }
            }
            HotShotEvent::SendPayloadCommitmentAndMetadata(payload_commitment, _metadata, view) => {
                let view = *view;
                debug!(
                    "Got payload commitment {:?} for view {view:?}",
                    payload_commitment
                );

                self.create_dependency_task_if_new(
                    view,
                    event_receiver,
                    event_sender,
                    event.clone(),
                );
            }
            HotShotEvent::ViewSyncFinalizeCertificate2Recv(certificate) => {
                if !certificate.is_valid_cert(self.quorum_membership.as_ref()) {
                    warn!(
                        "View Sync Finalize certificate {:?} was invalid",
                        certificate.get_data()
                    );
                    return;
                }

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
                    self.create_dependency_task_if_new(view, event_receiver, event_sender, event);
                }
            }
            HotShotEvent::QuorumProposalRecv(proposal, sender) => {
                let sender = sender.clone();
                debug!(
                    "Received Quorum Proposal for view {}",
                    *proposal.data.view_number
                );

                // stop polling for the received proposal
                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForProposal(
                        *proposal.data.view_number,
                    ))
                    .await;

                let view = proposal.data.get_view_number();
                if view < self.cur_view {
                    debug!("Proposal is from an older view {:?}", proposal.data.clone());
                    return;
                }

                let view_leader_key = self.quorum_membership.get_leader(view);
                if view_leader_key != sender {
                    warn!("Leader key does not match key in proposal");
                    return;
                }

                if proposal.data.justify_qc.get_view_number() != view - 1 {
                    if let Some(ref received_proposal_cert) = proposal.data.proposal_certificate {
                        match received_proposal_cert {
                            ViewChangeEvidence::Timeout(timeout_cert) => {
                                if timeout_cert.get_data().view != view - 1 {
                                    warn!("Timeout certificate for view {} was not for the immediately preceding view", *view);
                                    return;
                                }

                                if !timeout_cert.is_valid_cert(self.timeout_membership.as_ref()) {
                                    warn!("Timeout certificate for view {} was invalid", *view);
                                    return;
                                }
                            }
                            ViewChangeEvidence::ViewSync(view_sync_cert) => {
                                if view_sync_cert.view_number != view {
                                    debug!(
                                        "Cert view number {:?} does not match proposal view number {:?}",
                                        view_sync_cert.view_number, view
                                    );
                                    return;
                                }

                                // View sync certs must also be valid.
                                if !view_sync_cert.is_valid_cert(self.quorum_membership.as_ref()) {
                                    debug!("Invalid ViewSyncFinalize cert provided");
                                    return;
                                }
                            }
                        }
                    } else {
                        warn!(
                            "Quorum proposal for view {} needed a timeout or view sync certificate, but did not have one",
                            *view);
                        return;
                    }
                }

                let justify_qc = proposal.data.justify_qc.clone();

                if !justify_qc.is_valid_cert(self.quorum_membership.as_ref()) {
                    error!("Invalid justify_qc in proposal for view {}", *view);
                    let consensus = self.consensus.write().await;
                    consensus.metrics.invalid_qc.update(1);
                    return;
                }

                // Validate the upgrade certificate -- this is just a signature validation.
                // Note that we don't do anything with the certificate directly if this passes; it eventually gets stored as part of the leaf if nothing goes wrong.
                if let Err(e) = UpgradeCertificate::validate(
                    &proposal.data.upgrade_certificate,
                    &self.quorum_membership,
                ) {
                    warn!("{:?}", e);

                    return;
                }

                // NOTE: We could update our view with a valid TC but invalid QC, but that is not what we do here
                self.update_view(view, &event_sender).await;

                let consensus = self.consensus.upgradable_read().await;

                // Get the parent leaf and state.
                let parent = match consensus
                    .saved_leaves
                    .get(&justify_qc.get_data().leaf_commit)
                    .cloned()
                {
                    Some(leaf) => {
                        if let (Some(state), _) =
                            consensus.get_state_and_delta(leaf.get_view_number())
                        {
                            Some((leaf, state.clone()))
                        } else {
                            error!("Parent state not found! Consensus internally inconsistent");
                            return;
                        }
                    }
                    None => None,
                };

                if justify_qc.get_view_number() > consensus.high_qc.view_number {
                    if let Err(e) = self
                        .storage
                        .write()
                        .await
                        .update_high_qc(justify_qc.clone())
                        .await
                    {
                        warn!("Failed to store High QC not voting. Error: {:?}", e);
                        return;
                    }
                }

                let mut consensus = RwLockUpgradableReadGuard::upgrade(consensus).await;

                if justify_qc.get_view_number() > consensus.high_qc.view_number {
                    debug!("Updating high QC");
                    consensus.high_qc = justify_qc.clone();
                }

                // Justify qc's leaf commitment is not the same as the parent's leaf commitment, but it should be (in this case)
                let Some((parent_leaf, parent_state)) = parent else {
                    warn!(
                        "Proposal's parent missing from storage with commitment: {:?}",
                        justify_qc.get_data().leaf_commit
                    );

                    let leaf = Leaf::from_quorum_proposal(&proposal.data);

                    let state = Arc::new(
                        <TYPES::ValidatedState as ValidatedState<TYPES>>::from_header(
                            &proposal.data.block_header,
                        ),
                    );

                    consensus.validated_state_map.insert(
                        view,
                        View {
                            view_inner: ViewInner::Leaf {
                                leaf: leaf.commit(),
                                state,
                                delta: None,
                            },
                        },
                    );
                    consensus.saved_leaves.insert(leaf.commit(), leaf.clone());

                    if let Err(e) = self
                        .storage
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

                    // If we are missing the parent from storage, the safety check will fail.  But we can
                    // still vote if the liveness check succeeds.
                    let liveness_check = justify_qc.get_view_number() > consensus.locked_view;

                    let high_qc = consensus.high_qc.clone();
                    let locked_view = consensus.locked_view;

                    drop(consensus);

                    if liveness_check {
                        let current_proposal = proposal.data.clone();
                        let new_view = proposal.data.view_number + 1;

                        // This is for the case where we form a QC but have not yet seen the previous proposal ourselves
                        let should_propose = self.quorum_membership.get_leader(new_view)
                            == self.public_key
                            && high_qc.view_number == current_proposal.view_number;
                        let qc = high_qc.clone();
                        if should_propose {
                            debug!(
                                "Attempting to publish proposal after voting; now in view: {}",
                                *new_view
                            );

                            self.create_dependency_task_if_new(
                                qc.view_number + 1,
                                event_receiver,
                                event_sender,
                                event.clone(),
                            );
                        }
                    }
                    warn!("Failed liveneess check; cannot find parent either\n High QC is {:?}  Proposal QC is {:?}  Locked view is {:?}", high_qc, proposal.data.clone(), locked_view);

                    return;
                };
                async_spawn(
                    validate_proposal(
                        proposal.clone(),
                        parent_leaf,
                        self.consensus.clone(),
                        None,
                        self.quorum_membership.clone(),
                        parent_state.clone(),
                        view_leader_key,
                        event_sender.clone(),
                        sender,
                        self.output_event_stream.clone(),
                        self.storage.clone(),
                    )
                    .map(AnyhowTracing::err_as_debug),
                );
            }
            HotShotEvent::QuorumProposalValidated(proposal, _) => {
                let current_proposal = Some(proposal.clone());
                let new_view = current_proposal.clone().unwrap().view_number + 1;

                info!(
                    "Node {} creating dependency task for view {:?} from QuorumProposalRecv",
                    self.id, new_view
                );

                self.create_dependency_task_if_new(
                    new_view,
                    event_receiver,
                    event_sender,
                    event.clone(),
                );
            }
            HotShotEvent::QuorumProposalSend(proposal, _) => {
                let view = proposal.data.view_number;
                if !self.update_latest_proposed_view(view).await {
                    warn!("Failed to update latest proposed view");
                    return;
                }
            }
            _ => {}
        }
    }

    /// Must only update the view and GC if the view actually changes
    #[instrument(skip_all, fields(
        id = self.id,
        view = *self.cur_view
    ), name = "Consensus update view", level = "error")]
    async fn update_view(
        &mut self,
        new_view: TYPES::Time,
        event_stream: &Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> bool {
        if *self.cur_view < *new_view {
            debug!(
                "Updating view from {} to {} in consensus task",
                *self.cur_view, *new_view
            );

            if *self.cur_view / 100 != *new_view / 100 {
                // TODO (https://github.com/EspressoSystems/HotShot/issues/2296):
                // switch to info! when INFO logs become less cluttered
                error!("Progress: entered view {:>6}", *new_view);
            }

            // cancel the old timeout task
            if let Some(timeout_task) = self.timeout_task.take() {
                cancel_task(timeout_task).await;
            }
            self.cur_view = new_view;

            // Poll the future leader for lookahead
            let lookahead_view = new_view + LOOK_AHEAD;
            if self.quorum_membership.get_leader(lookahead_view) != self.public_key {
                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::PollFutureLeader(
                        *lookahead_view,
                        self.quorum_membership.get_leader(lookahead_view),
                    ))
                    .await;
            }

            // Start polling for proposals for the new view
            self.quorum_network
                .inject_consensus_info(ConsensusIntentEvent::PollForProposal(*self.cur_view + 1))
                .await;

            self.quorum_network
                .inject_consensus_info(ConsensusIntentEvent::PollForDAC(*self.cur_view + 1))
                .await;

            if self.quorum_membership.get_leader(self.cur_view + 1) == self.public_key {
                debug!("Polling for quorum votes for view {}", *self.cur_view);
                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::PollForVotes(*self.cur_view))
                    .await;
            }

            broadcast_event(Arc::new(HotShotEvent::ViewChange(new_view)), event_stream).await;

            // Spawn a timeout task if we did actually update view
            let timeout = self.timeout;
            self.timeout_task = Some(async_spawn({
                let stream = event_stream.clone();
                // Nuance: We timeout on the view + 1 here because that means that we have
                // not seen evidence to transition to this new view
                let view_number = self.cur_view + 1;
                async move {
                    async_sleep(Duration::from_millis(timeout)).await;
                    broadcast_event(
                        Arc::new(HotShotEvent::Timeout(TYPES::Time::new(*view_number))),
                        &stream,
                    )
                    .await;
                }
            }));
            let consensus = self.consensus.upgradable_read().await;
            consensus
                .metrics
                .current_view
                .set(usize::try_from(self.cur_view.get_u64()).unwrap());
            // Do the comparison before the subtraction to avoid potential overflow, since
            // `last_decided_view` may be greater than `cur_view` if the node is catching up.
            if usize::try_from(self.cur_view.get_u64()).unwrap()
                > usize::try_from(consensus.last_decided_view.get_u64()).unwrap()
            {
                consensus.metrics.number_of_views_since_last_decide.set(
                    usize::try_from(self.cur_view.get_u64()).unwrap()
                        - usize::try_from(consensus.last_decided_view.get_u64()).unwrap(),
                );
            }
            let mut consensus = RwLockUpgradableReadGuard::upgrade(consensus).await;
            consensus.update_view(new_view);
            drop(consensus);

            return true;
        }
        false
    }
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> TaskState
    for QuorumProposalTaskState<TYPES, I>
{
    type Event = Arc<HotShotEvent<TYPES>>;
    type Output = ();
    fn filter(&self, event: &Arc<HotShotEvent<TYPES>>) -> bool {
        !matches!(
            event.as_ref(),
            HotShotEvent::QuorumProposalRecv(_, _)
                | HotShotEvent::QuorumProposalValidated(..)
                | HotShotEvent::QCFormed(_)
                | HotShotEvent::SendPayloadCommitmentAndMetadata(..)
                | HotShotEvent::ViewSyncFinalizeCertificate2Recv(_)
                | HotShotEvent::Shutdown,
        )
    }
    async fn handle_event(event: Self::Event, task: &mut Task<Self>) -> Option<()>
    where
        Self: Sized,
    {
        let receiver = task.subscribe();
        let sender = task.clone_sender();
        tracing::trace!("sender queue len {}", sender.len());
        task.state_mut().handle(event, receiver, sender).await;
        None
    }
    fn should_shutdown(event: &Self::Event) -> bool {
        matches!(event.as_ref(), HotShotEvent::Shutdown)
    }
}
