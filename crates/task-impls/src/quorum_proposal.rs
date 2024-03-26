use std::{collections::HashMap, sync::Arc};

use async_broadcast::{Receiver, Sender};
use async_lock::RwLock;
use hotshot_task::{
    dependency::{AndDependency, EventDependency},
    dependency_task::{DependencyTask, HandleDepOutput},
    task::{Task, TaskState},
};
use hotshot_types::{
    consensus::Consensus,
    constants::LOOK_AHEAD,
    data::{QuorumProposal, ViewChangeEvidence},
    event::Event,
    message::Proposal,
    simple_certificate::ViewSyncFinalizeCertificate2,
    traits::{
        block_contents::BlockHeader,
        election::Membership,
        network::{ConnectedNetwork, ConsensusIntentEvent},
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
    },
    vote::{Certificate, HasViewNumber},
};

#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
#[cfg(async_executor_impl = "tokio")]
use tokio::task::JoinHandle;
use tracing::{debug, error, instrument};

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

    /// Any of the 3 optional proposal certs is successfully received and sent downstream in the
    /// task. The certs are `ViewSyncFinalizeCertificate2Recv`, `QuorumProposalRecv`, and `QCFormed`.
    ProposalCertificate,
}

/// Handler for the proposal dependency
struct ProposalDependencyHandle<TYPES: NodeType> {
    /// The view number to propose for.
    view_number: TYPES::Time,

    /// The event sender.
    sender: Sender<Arc<HotShotEvent<TYPES>>>,

    /// Reference to consensus. The replica will require a write lock on this.
    #[allow(dead_code)]
    consensus: Arc<RwLock<Consensus<TYPES>>>,
}

impl<TYPES: NodeType> HandleDepOutput for ProposalDependencyHandle<TYPES> {
    type Output = Vec<Arc<HotShotEvent<TYPES>>>;

    #[allow(clippy::no_effect_underscore_binding)]
    async fn handle_dep_result(self, res: Self::Output) {
        let mut payload_commitment = None;
        let mut commit_and_metadata: Option<CommitmentAndMetadata<TYPES::BlockPayload>> = None;
        let mut _quorum_certificate = None;
        let mut _timeout_certificate = None;
        let mut _view_sync_finalize_cert = None;
        for event in res {
            match event.as_ref() {
                HotShotEvent::QuorumProposalRecv(proposal, _) => {
                    let proposal_payload_comm = proposal.data.block_header.payload_commitment();
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

        broadcast_event(
            Arc::new(HotShotEvent::QuorumProposalDependenciesValidated(
                self.view_number,
            )),
            &self.sender,
        )
        .await;
        broadcast_event(
            Arc::new(HotShotEvent::DummyQuorumProposalSend(self.view_number)),
            &self.sender,
        )
        .await;
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

    /// The node's id
    pub id: u64,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> QuorumProposalTaskState<TYPES, I> {
    /// Create an event dependency
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view), name = "Quorum proposal create event dependency", level = "error")]
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
                debug!("Dependency {:?} got event {:?}", dependency_type, event);
                let event_view = match dependency_type {
                    ProposalDependency::ProposalCertificate => match event {
                        HotShotEvent::ViewSyncFinalizeCertificate2Recv(view_sync_cert) => {
                            view_sync_cert.view_number
                        }
                        HotShotEvent::QCFormed(cert) => match cert {
                            either::Right(timeout) => timeout.view_number,
                            either::Left(qc) => qc.view_number,
                        },
                        HotShotEvent::QuorumProposalRecv(proposal, _) => proposal.data.view_number,
                        _ => return false,
                    },
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
                event_view == view_number
            }),
        )
    }

    /// Create and store an [`AndDependency`] combining [`EventDependency`]s associated with the
    /// given view number if it doesn't exist. Also takes in the received `event` to seed a
    /// dependency as already completed. This allows for the task to receive a proposable event
    /// without losing the data that it received, as the dependency task would otherwise have no
    /// ability to receive the event and, thus, would never propose.
    fn create_dependency_task_if_new(
        &mut self,
        view_number: TYPES::Time,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
        event_sender: Sender<Arc<HotShotEvent<TYPES>>>,
        event: Arc<HotShotEvent<TYPES>>,
    ) {
        debug!("Attempting to make dependency task for event {:?}", event);
        if self.propose_dependencies.get(&view_number).is_some() {
            debug!("Task already exists");
            return;
        }

        let mut proposal_cert_validated_dependency = self.create_event_dependency(
            ProposalDependency::ProposalCertificate,
            view_number,
            event_receiver.clone(),
        );

        let mut payload_commitment_dependency = self.create_event_dependency(
            ProposalDependency::PayloadAndMetadata,
            view_number,
            event_receiver,
        );

        match event.as_ref() {
            HotShotEvent::SendPayloadCommitmentAndMetadata(_, _, _) => {
                payload_commitment_dependency.mark_as_completed(event.clone());
            }
            HotShotEvent::QuorumProposalRecv(_, _)
            | HotShotEvent::QCFormed(_)
            | HotShotEvent::ViewSyncFinalizeCertificate2Recv(_) => {
                proposal_cert_validated_dependency.mark_as_completed(event);
            }
            _ => {}
        };

        // We must always have the payload commitment and metadata, but can have any of the other
        // events that include a proposable certificate
        let proposal_dependency = AndDependency::from_deps(vec![
            payload_commitment_dependency,
            proposal_cert_validated_dependency,
        ]);

        let dependency_task = DependencyTask::new(
            proposal_dependency,
            ProposalDependencyHandle {
                view_number,
                sender: event_sender,
                consensus: self.consensus.clone(),
            },
        );
        self.propose_dependencies
            .insert(view_number, dependency_task.run());
    }

    /// Update the latest proposed view number.
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view), name = "Quorum proposal update latest proposed view", level = "error")]
    async fn update_latest_proposed_view(&mut self, new_view: TYPES::Time) -> bool {
        if *self.latest_proposed_view < *new_view {
            debug!(
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

    /// Validate a quorum proposal.
    async fn validate_quorum_proposal(
        &mut self,
        view: TYPES::Time,
        quorum_proposal: Proposal<TYPES, QuorumProposal<TYPES>>,
        sender: TYPES::SignatureKey,
    ) -> bool {
        let view_leader_key = self.quorum_membership.get_leader(view);
        if view_leader_key != sender {
            return false;
        }

        if quorum_proposal.data.justify_qc.get_view_number() != view - 1 {
            if let Some(received_quorum_proposal_cert) =
                quorum_proposal.data.proposal_certificate.clone()
            {
                match received_quorum_proposal_cert {
                    ViewChangeEvidence::Timeout(timeout_cert) => {
                        if timeout_cert.get_data().view != view - 1 {
                            tracing::warn!("Timeout certificate for view {} was not for the immediately preceding view", *view);
                            return false;
                        }

                        if !timeout_cert.is_valid_cert(self.timeout_membership.as_ref()) {
                            tracing::warn!("Timeout certificate for view {} was invalid", *view);
                            return false;
                        }
                    }
                    ViewChangeEvidence::ViewSync(view_sync_cert) => {
                        if view_sync_cert.view_number != view {
                            debug!(
                                "Cert view number {:?} does not match proposal view number {:?}",
                                view_sync_cert.view_number, view
                            );
                            return false;
                        }

                        // View sync certs must also be valid.
                        if !view_sync_cert.is_valid_cert(self.quorum_membership.as_ref()) {
                            debug!("Invalid ViewSyncFinalize cert provided");
                            return false;
                        }
                    }
                }
            } else {
                tracing::warn!(
                    "Quorum proposal for view {} needed a timeout or view sync certificate, but did not have one",
                    *view);
                return false;
            };
        }

        let justify_qc = quorum_proposal.data.justify_qc.clone();
        if !justify_qc.is_valid_cert(self.quorum_membership.as_ref()) {
            error!("Invalid justify_qc in proposal for view {}", *view);
            let consensus = self.consensus.write().await;
            consensus.metrics.invalid_qc.update(1);
            return false;
        }

        // Validate the upgrade certificate, if one is attached.
        // Continue unless the certificate is invalid.
        //
        // Note: we are *not* directly voting on the upgrade certificate here.
        // Once a certificate has been (allegedly) formed, it has already been voted on.
        // The certificate is either valid or invalid, and we are simply validating it.
        //
        // SS: It is possible that we may wish to vote against any quorum proposal
        // if it attaches an upgrade certificate that we cannot support.
        // But I don't think there's much point in this -- if the UpgradeCertificate
        // threshhold (90%) has been reached, voting against the QuorumProposal on that basis
        // will probably be completely symbolic anyway.
        //
        // We should just make sure we don't *sign* an UpgradeCertificate for an upgrade
        // that we do not support.
        if let Some(ref upgrade_cert) = quorum_proposal.data.upgrade_certificate {
            if upgrade_cert.is_valid_cert(self.quorum_membership.as_ref()) {
                self.consensus
                    .write()
                    .await
                    .saved_upgrade_certs
                    .insert(view, upgrade_cert.clone());
            } else {
                tracing::error!("Invalid upgrade_cert in proposal for view {}", *view);
                return false;
            }
        }

        true
    }

    /// Validate a view sync cert or a timeout cert.
    fn validate_view_sync_finalize_certificate(
        &self,
        certificate: &ViewSyncFinalizeCertificate2<TYPES>,
    ) -> bool {
        certificate.is_valid_cert(self.quorum_membership.as_ref())
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

                        self.create_dependency_task_if_new(
                            view,
                            event_receiver,
                            event_sender,
                            event.clone(),
                        );
                    }
                    either::Left(qc) => {
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
                        debug!(
                            "Attempting to publish proposal after forming a QC for view {}",
                            *qc.view_number
                        );

                        let view = qc.view_number + 1;

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
                if view < self.latest_proposed_view {
                    debug!(
                        "Payload commitment is from an older view {:?}",
                        view.clone()
                    );
                    return;
                }

                debug!("Got payload commitment and meta {:?}", payload_commitment);

                self.create_dependency_task_if_new(
                    view,
                    event_receiver,
                    event_sender,
                    event.clone(),
                );
            }
            HotShotEvent::ViewSyncFinalizeCertificate2Recv(view_sync_finalize_cert) => {
                let view = view_sync_finalize_cert.view_number;
                if view < self.latest_proposed_view {
                    debug!(
                        "View sync certificate is from an old view {:?}",
                        view.clone()
                    );
                    return;
                }

                if !self.validate_view_sync_finalize_certificate(view_sync_finalize_cert) {
                    return;
                }

                self.create_dependency_task_if_new(view, event_receiver, event_sender, event);
            }
            HotShotEvent::QuorumProposalRecv(proposal, sender) => {
                let view = proposal.data.get_view_number();
                if view < self.latest_proposed_view {
                    debug!("Proposal is from an older view {:?}", proposal.data.clone());
                    return;
                }

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

                if !self
                    .validate_quorum_proposal(view, proposal.clone(), sender.clone())
                    .await
                {
                    return;
                }
                broadcast_event(
                    Arc::new(HotShotEvent::QuorumProposalValidated(proposal.data.clone())),
                    &event_sender.clone(),
                )
                .await;

                self.create_dependency_task_if_new(
                    view,
                    event_receiver,
                    event_sender,
                    event.clone(),
                );
            }
            HotShotEvent::QuorumProposalDependenciesValidated(view) => {
                debug!("All proposal dependencies verified for view {:?}", view);
                if !self.update_latest_proposed_view(*view).await {
                    debug!("proposal not updated");
                    return;
                }
            }
            _ => {}
        }
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
