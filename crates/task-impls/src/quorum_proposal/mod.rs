use std::{collections::HashMap, sync::Arc};

use async_broadcast::{Receiver, Sender};
use async_lock::RwLock;
#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
use either::Either;
use hotshot_task::{
    cancel_task,
    dependency::{AndDependency, EventDependency, OrDependency},
    dependency_task::DependencyTask,
    task::{Task, TaskState},
};
use hotshot_types::{
    consensus::Consensus,
    event::Event,
    hotshot_event::HotShotEvent,
    traits::{
        election::Membership,
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
        signature_key::SignatureKey,
        storage::Storage,
    },
    vote::Certificate,
};
#[cfg(async_executor_impl = "tokio")]
use tokio::task::JoinHandle;
use tracing::{debug, instrument, warn};

#[cfg(feature = "dependency-tasks")]
use crate::consensus::helpers::handle_quorum_proposal_validated;

use self::dependency_handle::{ProposalDependency, ProposalDependencyHandle};

mod dependency_handle;

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

    /// Immutable instance state
    pub instance_state: Arc<TYPES::InstanceState>,

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
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view), name = "Create event dependency", level = "info")]
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
                            _builder_commitment,
                            _metadata,
                            view,
                            _fee,
                        ) = event
                        {
                            *view
                        } else {
                            return false;
                        }
                    }
                    ProposalDependency::ProposeNow => {
                        if let HotShotEvent::ProposeNow(view, _) = event {
                            *view
                        } else {
                            return false;
                        }
                    }
                };
                let valid = event_view == view_number;
                if valid {
                    debug!("Dependency {dependency_type:?} is complete for view {event_view:?}!",);
                }
                valid
            }),
        )
    }

    /// Creates the requisite dependencies for the Quorum Proposal task. It also handles any event forwarding.
    fn create_and_complete_dependencies(
        &self,
        view_number: TYPES::Time,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
        event: Arc<HotShotEvent<TYPES>>,
    ) -> AndDependency<Vec<Vec<Arc<HotShotEvent<TYPES>>>>> {
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
            event_receiver.clone(),
        );

        let mut propose_now_dependency = self.create_event_dependency(
            ProposalDependency::ProposeNow,
            view_number,
            event_receiver,
        );

        match event.as_ref() {
            HotShotEvent::ProposeNow(..) => {
                propose_now_dependency.mark_as_completed(Arc::clone(&event));
            }
            HotShotEvent::SendPayloadCommitmentAndMetadata(..) => {
                payload_commitment_dependency.mark_as_completed(Arc::clone(&event));
            }
            HotShotEvent::QuorumProposalValidated(..) => {
                proposal_dependency.mark_as_completed(event);
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
        let mut secondary_deps = vec![
            // 2. A timeout cert was received
            AndDependency::from_deps(vec![timeout_dependency]),
            // 3. A view sync cert was received.
            AndDependency::from_deps(vec![view_sync_dependency]),
        ];

        // 1. A QCFormed event and QuorumProposalValidated event
        if *view_number > 1 {
            secondary_deps.push(AndDependency::from_deps(vec![
                qc_dependency,
                proposal_dependency,
            ]));
        } else {
            secondary_deps.push(AndDependency::from_deps(vec![qc_dependency]));
        }

        AndDependency::from_deps(vec![OrDependency::from_deps(vec![
            AndDependency::from_deps(vec![AndDependency::from_deps(vec![propose_now_dependency])]),
            AndDependency::from_deps(vec![
                OrDependency::from_deps(vec![AndDependency::from_deps(vec![
                    payload_commitment_dependency,
                ])]),
                OrDependency::from_deps(secondary_deps),
            ]),
        ])])
    }

    /// Create and store an [`AndDependency`] combining [`EventDependency`]s associated with the
    /// given view number if it doesn't exist. Also takes in the received `event` to seed a
    /// dependency as already completed. This allows for the task to receive a proposable event
    /// without losing the data that it received, as the dependency task would otherwise have no
    /// ability to receive the event and, thus, would never propose.
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view), name = "Create dependency task", level = "error")]
    fn create_dependency_task_if_new(
        &mut self,
        view_number: TYPES::Time,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
        event_sender: Sender<Arc<HotShotEvent<TYPES>>>,
        event: Arc<HotShotEvent<TYPES>>,
    ) {
        // Don't even bother making the task if we are not entitled to propose anyay.
        if self.quorum_membership.get_leader(view_number) != self.public_key {
            return;
        }

        // Don't try to propose twice for the same view.
        if view_number <= self.latest_proposed_view {
            return;
        }

        debug!("Attempting to make dependency task for view {view_number:?} and event {event:?}");
        if self.propose_dependencies.contains_key(&view_number) {
            debug!("Task already exists");
            return;
        }

        let dependency_chain =
            self.create_and_complete_dependencies(view_number, event_receiver, event);

        let dependency_task = DependencyTask::new(
            dependency_chain,
            ProposalDependencyHandle {
                latest_proposed_view: self.latest_proposed_view,
                view_number,
                sender: event_sender,
                consensus: Arc::clone(&self.consensus),
                output_event_stream: self.output_event_stream.clone(),
                timeout_membership: Arc::clone(&self.timeout_membership),
                quorum_membership: Arc::clone(&self.quorum_membership),
                public_key: self.public_key.clone(),
                private_key: self.private_key.clone(),
                timeout: self.timeout,
                round_start_delay: self.round_start_delay,
                id: self.id,
                instance_state: Arc::clone(&self.instance_state),
            },
        );
        self.propose_dependencies
            .insert(view_number, dependency_task.run());
    }

    /// Update the latest proposed view number.
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view), name = "Update latest proposed view", level = "error")]
    async fn update_latest_proposed_view(&mut self, new_view: TYPES::Time) -> bool {
        if *self.latest_proposed_view < *new_view {
            debug!(
                "Updating latest proposed view from {} to {}",
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
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view), name = "handle method", level = "error")]
    pub async fn handle(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
        event_sender: Sender<Arc<HotShotEvent<TYPES>>>,
    ) {
        match event.as_ref() {
            HotShotEvent::ProposeNow(view, _) => {
                self.create_dependency_task_if_new(
                    *view,
                    event_receiver,
                    event_sender,
                    Arc::clone(&event),
                );
            }
            HotShotEvent::QCFormed(cert) => {
                match cert.clone() {
                    either::Right(timeout_cert) => {
                        let view = timeout_cert.view_number + 1;

                        self.create_dependency_task_if_new(
                            view,
                            event_receiver,
                            event_sender,
                            Arc::clone(&event),
                        );
                    }
                    either::Left(qc) => {
                        if let Err(e) = self.storage.write().await.update_high_qc(qc.clone()).await
                        {
                            warn!("Failed to store High QC of QC we formed; error = {:?}", e);
                        }

                        let mut consensus = self.consensus.write().await;
                        if let Err(e) = consensus.update_high_qc(qc.clone()) {
                            tracing::trace!("{e:?}");
                        }

                        // We need to drop our handle here to make the borrow checker happy.
                        drop(consensus);

                        let view = qc.view_number + 1;

                        self.create_dependency_task_if_new(
                            view,
                            event_receiver,
                            event_sender,
                            Arc::clone(&event),
                        );
                    }
                }
            }
            HotShotEvent::SendPayloadCommitmentAndMetadata(
                _payload_commitment,
                _builder_commitment,
                _metadata,
                view,
                _fee,
            ) => {
                let view = *view;

                self.create_dependency_task_if_new(
                    view,
                    event_receiver,
                    event_sender,
                    Arc::clone(&event),
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

                let view = certificate.view_number;

                self.create_dependency_task_if_new(view, event_receiver, event_sender, event);
            }
            #[cfg(feature = "dependency-tasks")]
            HotShotEvent::QuorumProposalValidated(proposal, _) => {
                let new_view = proposal.view_number;

                if !self.update_latest_proposed_view(new_view).await {
                    tracing::trace!("Failed to update latest proposed view");
                    return;
                }

                if let Err(e) =
                    handle_quorum_proposal_validated(proposal, event_sender.clone(), self).await
                {
                    debug!("Failed to handle QuorumProposalValidated event; error = {e:#}");
                }

                self.create_dependency_task_if_new(
                    new_view + 1,
                    event_receiver,
                    event_sender,
                    Arc::clone(&event),
                );
            }
            HotShotEvent::QuorumProposalSend(proposal, _) => {
                let view = proposal.data.view_number;
                if !self.update_latest_proposed_view(view).await {
                    tracing::trace!("Failed to update latest proposed view");
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
            HotShotEvent::QuorumProposalValidated(..)
                | HotShotEvent::QCFormed(_)
                | HotShotEvent::SendPayloadCommitmentAndMetadata(..)
                | HotShotEvent::ViewSyncFinalizeCertificate2Recv(_)
                | HotShotEvent::ProposeNow(..)
                | HotShotEvent::QuorumProposalSend(..)
                | HotShotEvent::Shutdown,
        )
    }
    async fn handle_event(event: Self::Event, task: &mut Task<Self>) -> Option<()>
    where
        Self: Sized,
    {
        let receiver = task.subscribe();
        let sender = task.clone_sender();
        task.state_mut().handle(event, receiver, sender).await;
        None
    }
    fn should_shutdown(event: &Self::Event) -> bool {
        matches!(event.as_ref(), HotShotEvent::Shutdown)
    }
}
