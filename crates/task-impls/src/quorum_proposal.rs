use std::{collections::HashMap, sync::Arc};

use async_broadcast::{Receiver, Sender};
use hotshot_task::{
    dependency::{AndDependency, EventDependency},
    dependency_task::{DependencyTask, HandleDepOutput},
};
use hotshot_types::{
    data::QuorumProposal,
    event::Event,
    message::Proposal,
    simple_certificate::{TimeoutCertificate, ViewSyncFinalizeCertificate2},
    traits::{
        network::{ConnectedNetwork, ConsensusIntentEvent},
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
    },
    vote::HasViewNumber,
};

#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
#[cfg(async_executor_impl = "tokio")]
use tokio::task::JoinHandle;
use tracing::{debug, instrument};

use crate::{
    events::HotShotEvent,
    helpers::{broadcast_event, cancel_task},
};

/// Validate a quorum proposal.
#[allow(clippy::needless_pass_by_value)]
fn validate_quorum_proposal<TYPES: NodeType>(
    _quorum_proposal: Proposal<TYPES, QuorumProposal<TYPES>>,
    _event_sender: Sender<Arc<HotShotEvent<TYPES>>>,
) -> bool {
    true
}

/// Proposal dependency types. These types represent events that precipitate a proposal.
#[derive(PartialEq)]
enum ProposalDependency {
    /// For the `SendPayloadCommitmentAndMetadata` event.
    PayloadAndMetadata,

    /// For the `ViewSyncFinalizeCertificate2Recv` event.
    ViewSync,

    /// For the `QuorumProposalRecv` event.
    QuorumProposal,

    /// For the `QCFormed` event.
    QCFormed,
}

struct ProposalDependencyHandle<TYPES: NodeType> {
    /// The view number to propose for.
    view_number: TYPES::Time,

    /// The event sender.
    sender: Sender<Arc<HotShotEvent<TYPES>>>,
}

impl<TYPES: NodeType> HandleDepOutput for ProposalDependencyHandle<TYPES> {
    type Output = Vec<Arc<HotShotEvent<TYPES>>>;

    async fn handle_dep_result(self, res: Self::Output) {
        for event in res {
            match event.as_ref() {
                _ => {}
            }
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

    /// last Timeout Certificate this node formed
    pub timeout_cert: Option<TimeoutCertificate<TYPES>>,

    // TODO: Merge view sync and timeout certs: https://github.com/EspressoSystems/HotShot/issues/2767
    /// last View Sync Certificate this node formed
    pub view_sync_cert: Option<ViewSyncFinalizeCertificate2<TYPES>>,

    /// The node's id
    pub id: u64,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> QuorumProposalTaskState<TYPES, I> {
    /// Create an event dependency
    fn create_event_dependency(
        &self,
        dependency_type: ProposalDependency,
        view_number: TYPES::Time,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
    ) -> EventDependency<Arc<HotShotEvent<TYPES>>> {
        EventDependency::new(
            event_receiver.clone(),
            Box::new(move |event| {
                let event = event.as_ref();
                let event_view = match dependency_type {
                    _ => view_number,
                };
                event_view == view_number
            }),
        )
    }

    /// Create and store an [`AndDependency`] combining [`EventDependency`]s associated with the
    /// given view number if it doesn't exist.
    fn create_dependency_task_if_new(
        &mut self,
        view_number: TYPES::Time,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
        event_sender: Sender<Arc<HotShotEvent<TYPES>>>,
    ) {
        if self.propose_dependencies.get(&view_number).is_some() {
            return;
        }
        let deps = vec![self.create_event_dependency(
            ProposalDependency::QuorumProposal,
            view_number,
            event_receiver.clone(),
        )];
        let proposal_dependency = AndDependency::from_deps(deps);
        let dependency_task = DependencyTask::new(
            proposal_dependency,
            ProposalDependencyHandle {
                view_number,
                sender: event_sender,
            },
        );
        self.propose_dependencies
            .insert(view_number, dependency_task.run());
    }

    /// Update the latest proposed view number.
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view), name = "Quorum vote update latest proposed view", level = "error")]
    async fn update_latest_proposed_view(&mut self, new_view: TYPES::Time) -> bool {
        if *self.latest_proposed_view < *new_view {
            debug!(
                "Updating next vote view from {} to {} in the quorum vote task",
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
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view), name = "Quorum vote handle", level = "error")]
    pub async fn handle(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
        event_sender: Sender<Arc<HotShotEvent<TYPES>>>,
    ) {
        match event.as_ref() {
            HotShotEvent::QCFormed(cert) => {}
            HotShotEvent::SendPayloadCommitmentAndMetadata(payload_commitment, metadata, view) => {}
            HotShotEvent::ViewSyncFinalizeCertificate2Recv(view_sync_finalize_cert) => {}
            HotShotEvent::QuorumProposalRecv(proposal, _sender) => {
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

                if !validate_quorum_proposal(proposal.clone(), event_sender.clone()) {
                    return;
                }

                broadcast_event(
                    Arc::new(HotShotEvent::QuorumProposalValidated(proposal.data.clone())),
                    &event_sender.clone(),
                )
                .await;

                self.create_dependency_task_if_new(view, event_receiver, event_sender);
            }
            _ => {}
        }
    }
}
