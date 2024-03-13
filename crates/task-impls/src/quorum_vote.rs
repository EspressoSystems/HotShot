use crate::{
    events::HotShotEvent,
    helpers::{broadcast_event, cancel_task},
};
use async_broadcast::{Receiver, Sender};
use async_compatibility_layer::art::async_spawn;
use hotshot_task::{
    dependency::{AndDependency, Dependency, EventDependency},
    task::{Task, TaskState},
};
use hotshot_types::{
    event::{Event, EventType},
    traits::{
        block_contents::BlockHeader,
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
use tracing::{debug, error, instrument};

enum VoteDependency {
    QuorumProposal,
    Dac,
    Vid,
}

/// The state for the quorum vote task.
///
/// Contains all of the information for the quorum vote.
pub struct QuorumVoteTaskState<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    pub next_vote_view: TYPES::Time,
    pub vote_dependencies: HashMap<TYPES::Time, AndDependency<HotShotEvent<TYPES>>>,
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
    /// Validate the quorum proposal.
    // TODO: Complete the dependency implementation.
    // <https://github.com/EspressoSystems/HotShot/issues/2710>
    #[instrument(skip_all, fields(id = self.id, next_vote_view = *self.next_vote_view), name = "Quorum vote validate proposal", level = "error")]
    fn validate_proposal(&mut self, _event_sender: &Sender<HotShotEvent<TYPES>>) -> bool {
        true
    }

    /// Validate the DAC.
    // TODO: Complete the dependency implementation.
    // <https://github.com/EspressoSystems/HotShot/issues/2710>
    #[instrument(skip_all, fields(id = self.id, next_vote_view = *self.next_vote_view), name = "Quorum vote validate DAC", level = "error")]
    fn validate_dac(&mut self, _event_sender: &Sender<HotShotEvent<TYPES>>) -> bool {
        true
    }

    /// Validate the VID share.
    // TODO: Complete the dependency implementation.
    // <https://github.com/EspressoSystems/HotShot/issues/2710>
    #[instrument(skip_all, fields(id = self.id, next_vote_view = *self.next_vote_view), name = "Quorum vote validate VID", level = "error")]
    fn validate_vid(&mut self, _event_sender: &Sender<HotShotEvent<TYPES>>) -> bool {
        true
    }

    /// Create an event dependency.
    #[instrument(skip_all, fields(id = self.id, next_vote_view = *self.next_vote_view), name = "Quorum vote validate VID", level = "error")]
    fn create_event_dependency(
        &self,
        dependency_type: VoteDependency,
        view_number: TYPES::Time,
        event_sender: &Sender<HotShotEvent<TYPES>>,
    ) -> EventDependency<HotShotEvent<TYPES>> {
        EventDependency {
            event_rx: self.event_receiver,
            match_fn: Box::new(move |e| match dependency_type {
                VoteDependency::QuorumProposal => {
                    if let HotShotEvent::QuorumProposalRecv(proposal, _) = e {
                        if proposal.data.view_number != view_number {
                            return false;
                        }
                        return self.validate_proposal(event_sender);
                    }
                    false
                }
                VoteDependency::Dac => {
                    if let HotShotEvent::DACRecv(proposal, _) = e {
                        if proposal.data.view_number != view_number {
                            return false;
                        }
                        return self.validate_dac(event_sender);
                    }
                    false
                }
                VoteDependency::Vid => {
                    if let HotShotEvent::VidRecv(proposal, _) = e {
                        if proposal.data.view_number != view_number {
                            return false;
                        }
                        return self.validate_vid(event_sender);
                    }
                    false
                }
            }),
        }
    }

    fn create_vote_dependency(
        validated_dependency: VoteDependency,
        view: TYPES::Time,
        sender: &Sender<HotShotEvent<TYPES>>,
    ) -> AndDependency<HotShotEvent<TYPES>> {
        let mut deps = Vec::new();
        if validated_dependency != VoteDependency::Proposal {
            deps.add(Self::create_event_dependency(
                VoteDependency::QuorumProposal,
                sender,
            ));
        }
        if validated_dependency != VoteDependency::Dac {
            deps.add(Self::create_event_dependency(VoteDependency::Dac, sender));
        }
        if validated_dependency != VoteDependency::Vid {
            deps.add(Self::create_event_dependency(VoteDependency::Vid, sender));
        }
        AndDependency::from(deps)
    }

    /// Update the view number for the next view to be voted for.
    #[instrument(skip_all, fields(id = self.id, next_vote_view = *self.next_vote_view), name = "Quorum vote update next vote view", level = "error")]
    async fn update_next_vote_view(
        &mut self,
        new_view: TYPES::Time,
        event_stream: &Sender<HotShotEvent<TYPES>>,
    ) -> bool {
        if *self.next_vote_view < *new_view {
            debug!(
                "Updating next vote view from {} to {} in the quorum vote task",
                *self.next_vote_view, *new_view
            );

            if *self.next_vote_view / 100 != *new_view / 100 {
                // TODO (https://github.com/EspressoSystems/HotShot/issues/2296):
                // switch to info! when INFO logs become less cluttered
                error!("Progress: entered view {:>6}", *new_view);
            }

            // Cancel the old dependency tasks.
            for (view, dependency) in self.vote_dependencies.iter_mut() {
                if view < &new_view {
                    cancel_task(dependency).await;
                    self.vote_dependencies.remove(view);
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
                    if !self.validate_proposal(&event_sender) {
                        return;
                    }
                    let dependency = Self::create_vote_dependency(
                        VoteDependency::QuorumProposal,
                        view,
                        &event_sender,
                    );
                    self.vote_dependencies.insert(view, dependency);
                    async_spawn(async move {
                        dependency.completed().await;
                        for v in [self.next_vote_view..view] {
                            self.vote_dependencies.remove(&v);
                        }
                        self.next_vote_view = view + 1;
                    });
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
                    if !self.validate_dac(&event_sender) {
                        return;
                    }
                    let dependency =
                        Self::create_vote_dependency(VoteDependency::Dac, view, &event_sender);
                    self.vote_dependencies.insert(view, dependency);
                    async_spawn(async move {
                        dependency.completed().await;
                        self.next_vote_view = view + 1;
                        for (view, _) in self.vote_dependencies.iter() {
                            if view < &self.next_vote_view {
                                self.vote_dependencies.remove(view);
                            }
                        }
                    });
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
                    if !self.validate_proposal(&event_sender) {
                        return;
                    }
                    let dependency =
                        Self::create_vote_dependency(VoteDependency::Vid, view, &event_sender);
                    self.vote_dependencies.insert(view, dependency);
                    async_spawn(async move {
                        dependency.completed().await;
                        self.next_vote_view = view + 1;
                        for (view, _) in self.vote_dependencies.iter() {
                            if *view < self.next_vote_view {
                                self.vote_dependencies.remove(view);
                            }
                        }
                    });
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
                if !self.update_next_vote_view(new_view, &event_sender).await {
                    debug!("view not updated");
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
