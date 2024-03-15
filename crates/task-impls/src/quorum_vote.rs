use crate::{
    events::HotShotEvent,
    helpers::{broadcast_event, cancel_task},
};
use async_broadcast::{Receiver, Sender};
use async_compatibility_layer::art::async_spawn;
#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
use hotshot_task::{
    dependency::{AndDependency, EventDependency},
    dependency_task::{DependencyTask, HandleDepOutput},
    task::{Task, TaskState},
};
use hotshot_types::{
    data::{QuorumProposal, VidDisperse},
    event::{Event, EventType},
    message::Proposal,
    simple_certificate::DACertificate,
    traits::{
        block_contents::BlockHeader,
        network::{ConnectedNetwork, ConsensusIntentEvent},
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
    },
    vote::{Certificate, HasViewNumber},
};
use std::collections::HashMap;
use std::sync::Arc;
#[cfg(async_executor_impl = "tokio")]
use tokio::task::JoinHandle;
use tracing::warn;
use tracing::{debug, instrument};

/// Vote dependency types.
#[derive(PartialEq)]
enum VoteDependency {
    /// For the `QuorumProposalRecv` event.
    QuorumProposal,
    /// For the `DACRecv` event.
    Dac,
    /// For the `VidDisperseRecv` event.
    Vid,
}

/// Validate the quorum proposal.
// TODO: Complete the dependency implementation.
// <https://github.com/EspressoSystems/HotShot/issues/2710>
#[allow(clippy::needless_pass_by_value)]
fn validate_quorum_proposal<TYPES: NodeType>(
    _quorum_proposal: Proposal<TYPES, QuorumProposal<TYPES>>,
    _event_sender: Sender<HotShotEvent<TYPES>>,
) -> bool {
    true
}

/// Validate the DAC.
// TODO: Complete the dependency implementation.
// <https://github.com/EspressoSystems/HotShot/issues/2710>
#[allow(clippy::needless_pass_by_value)]
fn validate_dac<TYPES: NodeType>(
    _dac: DACertificate<TYPES>,
    _event_sender: Sender<HotShotEvent<TYPES>>,
) -> bool {
    true
}

/// Validate the VID share.
// TODO: Complete the dependency implementation.
// <https://github.com/EspressoSystems/HotShot/issues/2710>
#[allow(clippy::needless_pass_by_value)]
fn validate_vid<TYPES: NodeType>(
    _disperse: Proposal<TYPES, VidDisperse<TYPES>>,
    _event_sender: Sender<HotShotEvent<TYPES>>,
) -> bool {
    true
}

/// Handler for the vote dependency.
struct VoteDependencyHandle<TYPES: NodeType> {
    /// View number to vote on.
    view_number: TYPES::Time,
    /// Event sender.
    sender: Sender<HotShotEvent<TYPES>>,
}
impl<TYPES: NodeType> HandleDepOutput for VoteDependencyHandle<TYPES> {
    type Output = Vec<HotShotEvent<TYPES>>;
    async fn handle_dep_result(self, res: Self::Output) {
        // Add this commitment check to test if the handler works, but this isn't the only thing
        // that we'll need to check.
        // TODO: Complete the dependency implementation.
        // <https://github.com/EspressoSystems/HotShot/issues/2710>
        let mut payload_commitment = None;
        for event in res {
            match event {
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
                HotShotEvent::DACValidated(cert) => {
                    let cert_payload_comm = cert.get_data().payload_commit;
                    if let Some(comm) = payload_commitment {
                        if cert_payload_comm != comm {
                            return;
                        }
                    } else {
                        payload_commitment = Some(cert_payload_comm);
                    }
                }
                _ => {}
            }
        }
        broadcast_event(HotShotEvent::ViewChange(self.view_number + 1), &self.sender).await;
        broadcast_event(
            HotShotEvent::DummyQuorumVoteSend(self.view_number),
            &self.sender,
        )
        .await;
    }
}

/// The state for the quorum vote task.
///
/// Contains all of the information for the quorum vote.
pub struct QuorumVoteTaskState<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Smallest potential view number that will be voted for.
    pub next_vote_view: TYPES::Time,

    /// Table for the in-progress dependency tasks.
    pub vote_dependencies: HashMap<TYPES::Time, JoinHandle<()>>,

    /// Network for all nodes
    pub quorum_network: Arc<I::QuorumNetwork>,

    /// Network for DA committee
    pub committee_network: Arc<I::CommitteeNetwork>,

    /// Output events to application
    pub output_event_stream: async_broadcast::Sender<Event<TYPES>>,

    /// The node's id
    pub id: u64,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> QuorumVoteTaskState<TYPES, I> {
    /// Create an event dependency.
    #[instrument(skip_all, fields(id = self.id, next_vote_view = *self.next_vote_view), name = "Quorum vote create event dependency", level = "error")]
    fn create_event_dependency(
        &self,
        dependency_type: VoteDependency,
        view_number: TYPES::Time,
        event_receiver: Receiver<HotShotEvent<TYPES>>,
    ) -> EventDependency<HotShotEvent<TYPES>> {
        EventDependency::new(
            event_receiver.clone(),
            Box::new(move |event| {
                let event_view = match dependency_type {
                    VoteDependency::QuorumProposal => {
                        if let HotShotEvent::QuorumProposalValidated(proposal) = event {
                            proposal.view_number
                        } else {
                            return false;
                        }
                    }
                    VoteDependency::Dac => {
                        if let HotShotEvent::DACValidated(cert) = event {
                            cert.view_number
                        } else {
                            return false;
                        }
                    }
                    VoteDependency::Vid => {
                        if let HotShotEvent::VidDisperseValidated(disperse) = event {
                            disperse.view_number
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
    /// given view number if it doesn't exist.
    fn create_dependency_task_if_new(
        &mut self,
        view_number: TYPES::Time,
        event_receiver: Receiver<HotShotEvent<TYPES>>,
        event_sender: Sender<HotShotEvent<TYPES>>,
    ) {
        if self.vote_dependencies.get(&view_number).is_some() {
            return;
        }
        let deps = vec![
            self.create_event_dependency(
                VoteDependency::QuorumProposal,
                view_number,
                event_receiver.clone(),
            ),
            self.create_event_dependency(VoteDependency::Dac, view_number, event_receiver.clone()),
            self.create_event_dependency(VoteDependency::Vid, view_number, event_receiver),
        ];
        let vote_dependency = AndDependency::from_deps(deps);
        let dependency_task = DependencyTask::new(
            vote_dependency,
            VoteDependencyHandle {
                view_number,
                sender: event_sender,
            },
        );
        self.vote_dependencies.insert(
            view_number,
            async_spawn(async move {
                dependency_task.run().await;
            }),
        );
    }

    /// Update the view number for the next view to be voted for.
    #[instrument(skip_all, fields(id = self.id, next_vote_view = *self.next_vote_view), name = "Quorum vote update next vote view", level = "error")]
    async fn update_next_vote_view(&mut self, new_view: TYPES::Time) -> bool {
        if *self.next_vote_view < *new_view {
            debug!(
                "Updating next vote view from {} to {} in the quorum vote task",
                *self.next_vote_view, *new_view
            );

            // Cancel the old dependency tasks.
            for view in *self.next_vote_view..*new_view {
                if let Some(dependency) = self.vote_dependencies.remove(&TYPES::Time::new(view)) {
                    cancel_task(dependency).await;

                    broadcast_event(
                        Event {
                            view_number: TYPES::Time::new(view),
                            event: EventType::ViewFinished {
                                view_number: TYPES::Time::new(view),
                            },
                        },
                        &self.output_event_stream,
                    )
                    .await;
                }
            }

            self.next_vote_view = new_view;

            return true;
        }
        false
    }

    /// Handles a consensus event received on the event stream
    #[instrument(skip_all, fields(id = self.id, next_vote_view = *self.next_vote_view), name = "Quorum vote handle", level = "error")]
    pub async fn handle(
        &mut self,
        event: HotShotEvent<TYPES>,
        event_receiver: Receiver<HotShotEvent<TYPES>>,
        event_sender: Sender<HotShotEvent<TYPES>>,
    ) {
        match event {
            HotShotEvent::QuorumProposalRecv(proposal, _sender) => {
                let view = proposal.data.view_number;
                if view < self.next_vote_view {
                    return;
                }
                debug!("Received Quorum Proposal for view {}", *view);

                // stop polling for the received proposal
                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForProposal(*view))
                    .await;
                if !validate_quorum_proposal(proposal.clone(), event_sender.clone()) {
                    return;
                }
                broadcast_event(
                    HotShotEvent::QuorumProposalValidated(proposal.data),
                    &event_sender.clone(),
                )
                .await;
                self.create_dependency_task_if_new(view, event_receiver, event_sender);
            }
            HotShotEvent::DACRecv(cert) => {
                debug!("DAC Received for view {}!", *cert.view_number);
                let view = cert.view_number;

                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForDAC(*view))
                    .await;

                self.committee_network
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForVotes(*view))
                    .await;

                if !validate_dac(cert.clone(), event_sender.clone()) {
                    return;
                }
                broadcast_event(HotShotEvent::DACValidated(cert), &event_sender.clone()).await;
                self.create_dependency_task_if_new(view, event_receiver, event_sender);
            }
            HotShotEvent::VidDisperseRecv(disperse) => {
                let view = disperse.data.get_view_number();

                // stop polling for the received disperse after verifying it's valid
                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForVIDDisperse(
                        *disperse.data.view_number,
                    ))
                    .await;

                if !validate_vid(disperse.clone(), event_sender.clone()) {
                    return;
                }
                broadcast_event(
                    HotShotEvent::VidDisperseValidated(disperse.data),
                    &event_sender.clone(),
                )
                .await;
                self.create_dependency_task_if_new(view, event_receiver, event_sender);
            }
            HotShotEvent::ViewChange(new_view) => {
                debug!("View Change event for view {} in consensus task", *new_view);

                let old_view_number = self.next_vote_view;

                // Start polling for VID disperse for the new view
                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::PollForVIDDisperse(
                        *old_view_number + 1,
                    ))
                    .await;

                // update the view in state to the one in the message
                // Publish a view change event to the application
                if !self.update_next_vote_view(new_view).await {
                    debug!("view not updated");
                    return;
                }
            }
            _ => {}
        }
    }
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> TaskState for QuorumVoteTaskState<TYPES, I> {
    type Event = HotShotEvent<TYPES>;
    type Output = ();
    fn filter(&self, event: &HotShotEvent<TYPES>) -> bool {
        !matches!(
            event,
            HotShotEvent::QuorumProposalRecv(_, _)
                | HotShotEvent::DACRecv(_)
                | HotShotEvent::ViewChange(_)
                | HotShotEvent::VidDisperseRecv(..)
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
        matches!(event.clone(), HotShotEvent::Shutdown)
    }
}
