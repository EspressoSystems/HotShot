use crate::{
    events::HotShotEvent,
    helpers::{broadcast_event, cancel_task},
};
use async_broadcast::{Receiver, Sender};
use async_compatibility_layer::art::async_spawn;
#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
use hotshot_task::{
    dependency::{AndDependency, Dependency, EventDependency},
    task::{Task, TaskState},
};
use hotshot_types::{
    data::{QuorumProposal, VidDisperse},
    event::{Event, EventType},
    message::Proposal,
    simple_certificate::DACertificate,
    traits::{
        network::{ConnectedNetwork, ConsensusIntentEvent},
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
    },
    vote::HasViewNumber,
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

/// The state for the quorum vote task.
///
/// Contains all of the information for the quorum vote.
pub struct QuorumVoteTaskState<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Smallest potential view number that will be voted for.
    pub next_vote_view: TYPES::Time,

    /// Table for the in-progress vote dependencies.
    pub vote_dependencies: HashMap<TYPES::Time, JoinHandle<()>>,

    /// Event receiver.
    pub event_receiver: Receiver<HotShotEvent<TYPES>>,

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
    #[instrument(skip_all, fields(id = self.id, next_vote_view = *self.next_vote_view), name = "Quorum vote validate VID", level = "error")]
    fn create_event_dependency(
        &self,
        dependency_type: VoteDependency,
        view_number: TYPES::Time,
        event_sender: Sender<HotShotEvent<TYPES>>,
    ) -> EventDependency<HotShotEvent<TYPES>> {
        EventDependency::new(
            self.event_receiver.clone(),
            Box::new(move |event| match dependency_type {
                VoteDependency::QuorumProposal => {
                    if let HotShotEvent::QuorumProposalRecv(proposal, _) = event {
                        if proposal.data.view_number != view_number {
                            return false;
                        }
                        return validate_quorum_proposal(proposal.clone(), event_sender.clone());
                    }
                    false
                }
                VoteDependency::Dac => {
                    if let HotShotEvent::DACRecv(cert) = event {
                        if cert.view_number != view_number {
                            return false;
                        }
                        return validate_dac(cert.clone(), event_sender.clone());
                    }
                    false
                }
                VoteDependency::Vid => {
                    if let HotShotEvent::VidDisperseRecv(disperse, _) = event {
                        if disperse.data.view_number != view_number {
                            return false;
                        }
                        return validate_vid(disperse.clone(), event_sender.clone());
                    }
                    false
                }
            }),
        )
    }

    /// Create an [`AndDependency`] combining three [`EventDependency`]s.
    fn create_vote_dependency(
        &self,
        validated_dependency: &VoteDependency,
        view_number: TYPES::Time,
        event_sender: Sender<HotShotEvent<TYPES>>,
    ) -> AndDependency<HotShotEvent<TYPES>> {
        let mut deps = Vec::new();
        if validated_dependency != &VoteDependency::QuorumProposal {
            deps.push(self.create_event_dependency(
                VoteDependency::QuorumProposal,
                view_number,
                event_sender.clone(),
            ));
        }
        if validated_dependency != &VoteDependency::Dac {
            deps.push(self.create_event_dependency(
                VoteDependency::Dac,
                view_number,
                event_sender.clone(),
            ));
        }
        if validated_dependency != &VoteDependency::Vid {
            deps.push(self.create_event_dependency(VoteDependency::Vid, view_number, event_sender));
        }
        AndDependency::from_deps(deps)
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
    #[instrument(skip_all, fields(id = self.id, next_vote_view = *self.next_vote_view), name = "Consensus replica task", level = "error")]
    pub async fn handle(
        &mut self,
        event: HotShotEvent<TYPES>,
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
                if self.vote_dependencies.get(&view).is_none() {
                    if !validate_quorum_proposal(proposal, event_sender.clone()) {
                        return;
                    }
                    let dependency = self.create_vote_dependency(
                        &VoteDependency::QuorumProposal,
                        view,
                        event_sender.clone(),
                    );
                    self.vote_dependencies.insert(
                        view,
                        async_spawn(async move {
                            dependency.completed().await;
                            broadcast_event(HotShotEvent::ViewChange(view + 1), &event_sender)
                                .await;
                            broadcast_event(HotShotEvent::DummyQuorumVoteSend(view), &event_sender)
                                .await;
                        }),
                    );
                }
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

                if self.vote_dependencies.get(&view).is_none() {
                    if !validate_dac(cert, event_sender.clone()) {
                        return;
                    }
                    let dependency = self.create_vote_dependency(
                        &VoteDependency::Dac,
                        view,
                        event_sender.clone(),
                    );
                    self.vote_dependencies.insert(
                        view,
                        async_spawn(async move {
                            dependency.completed().await;
                            broadcast_event(HotShotEvent::ViewChange(view + 1), &event_sender)
                                .await;
                            broadcast_event(HotShotEvent::DummyQuorumVoteSend(view), &event_sender)
                                .await;
                        }),
                    );
                }
            }
            HotShotEvent::VidDisperseRecv(disperse, _sender) => {
                let view = disperse.data.get_view_number();

                // stop polling for the received disperse after verifying it's valid
                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForVIDDisperse(
                        *disperse.data.view_number,
                    ))
                    .await;

                if self.vote_dependencies.get(&view).is_none() {
                    if !validate_vid(disperse, event_sender.clone()) {
                        return;
                    }
                    let dependency = self.create_vote_dependency(
                        &VoteDependency::Vid,
                        view,
                        event_sender.clone(),
                    );
                    self.vote_dependencies.insert(
                        view,
                        async_spawn(async move {
                            dependency.completed().await;
                            broadcast_event(HotShotEvent::ViewChange(view + 1), &event_sender)
                                .await;
                            broadcast_event(HotShotEvent::DummyQuorumVoteSend(view), &event_sender)
                                .await;
                        }),
                    );
                }
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
        let sender = task.clone_sender();
        tracing::trace!("sender queue len {}", sender.len());
        task.state_mut().handle(event, sender).await;
        None
    }
    fn should_shutdown(event: &Self::Event) -> bool {
        matches!(event, HotShotEvent::Shutdown)
    }
}
