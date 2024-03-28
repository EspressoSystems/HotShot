use anyhow::{bail, Result};
use async_compatibility_layer::art::{async_sleep, async_spawn};
use commit::Committable;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use async_broadcast::{Receiver, Sender};
use async_lock::{RwLock, RwLockUpgradableReadGuard};
use chrono::Utc;
use either::Either;
use hotshot_task::{
    dependency::{AndDependency, EventDependency, OrDependency},
    dependency_task::{DependencyTask, HandleDepOutput},
    task::{Task, TaskState},
};
use hotshot_types::{
    consensus::Consensus,
    constants::LOOK_AHEAD,
    data::{Leaf, QuorumProposal, VidDisperseShare, ViewChangeEvidence},
    event::{Event, EventType, LeafInfo},
    message::Proposal,
    simple_certificate::ViewSyncFinalizeCertificate2,
    traits::{
        block_contents::BlockHeader,
        consensus_api::ConsensusApi,
        election::Membership,
        network::{ConnectedNetwork, ConsensusIntentEvent},
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
        signature_key::SignatureKey,
        BlockPayload, ValidatedState,
    },
    utils::{Terminator, View, ViewInner},
    vote::{Certificate, HasViewNumber},
};

#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
#[cfg(async_executor_impl = "tokio")]
use tokio::task::JoinHandle;
use tracing::{debug, error, info, instrument};

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
                HotShotEvent::QuorumProposalValidated(proposal) => {
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

    /// Timeout task handle
    pub timeout_task: Option<JoinHandle<()>>,

    /// Our public key
    pub public_key: TYPES::SignatureKey,

    /// View timeout from config.
    pub timeout: u64,

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
                let event_view = match dependency_type {
                    ProposalDependency::QC => {
                        if let HotShotEvent::QCFormed(either::Left(qc)) = event {
                            qc.view_number
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

                    ProposalDependency::ProposalCertificate => {
                        if let HotShotEvent::QuorumProposalValidated(proposal) = event {
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
                    debug!("Depencency {:?} is complete!", dependency_type);
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

        let mut proposal_cert_dependency = self.create_event_dependency(
            ProposalDependency::ProposalCertificate,
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
            HotShotEvent::SendPayloadCommitmentAndMetadata(_, _, _) => {
                payload_commitment_dependency.mark_as_completed(event.clone());
            }
            HotShotEvent::QuorumProposalValidated(_) => {
                proposal_cert_dependency.mark_as_completed(event);
            }
            HotShotEvent::QCFormed(quorum_certificate) => match quorum_certificate {
                Either::Right(_) => {
                    timeout_dependency.mark_as_completed(event);
                }
                Either::Left(_) => {
                    qc_dependency.mark_as_completed(event);
                }
            },
            HotShotEvent::ViewSyncFinalizeCertificate2Recv(_) => {
                view_sync_dependency.mark_as_completed(event);
            }
            _ => {}
        };

        // We have three cases to consider:
        let combined = AndDependency::from_deps(vec![
            OrDependency::from_deps(vec![AndDependency::from_deps(vec![
                payload_commitment_dependency,
            ])]),
            OrDependency::from_deps(vec![
                // 1. A QCFormed event and QuorumProposalValidated event
                AndDependency::from_deps(vec![qc_dependency, proposal_cert_dependency]),
                // 2. A timeout cert was received
                AndDependency::from_deps(vec![timeout_dependency]),
                // 3. A view sync cert was received.
                AndDependency::from_deps(vec![view_sync_dependency]),
            ]),
        ]);

        let dependency_task = DependencyTask::new(
            combined,
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

    /// Validates view change evidence and logs if a failure occurs
    fn validate_view_change_evidence(
        &self,
        view: TYPES::Time,
        evidence: ViewChangeEvidence<TYPES>,
    ) -> bool {
        match evidence {
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
            HotShotEvent::QuorumProposalValidated(proposal) => {
                let view = proposal.get_view_number() + 1;
                if view < self.latest_proposed_view {
                    debug!("Proposal is from an older view {:?}", proposal.clone());
                    return;
                }

                debug!(
                    "Received Quorum Proposal for view {}",
                    *proposal.view_number
                );

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
            HotShotEvent::QuorumProposalValidated(_)
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
