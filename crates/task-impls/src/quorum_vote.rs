use crate::{
    events::{HotShotEvent, HotShotTaskCompleted},
    helpers::{broadcast_event, cancel_task},
    vote_collection::{create_vote_accumulator, AccumulatorInfo, VoteCollectionTaskState},
};
use async_compatibility_layer::art::{async_sleep, async_spawn};
use async_lock::{RwLock, RwLockUpgradableReadGuard};
#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
use commit::Committable;
use core::time::Duration;
use hotshot_constants::Version;
use hotshot_constants::LOOK_AHEAD;
use hotshot_task::{
    dependency::{AndDependency, EventDependency},
    task::{Task, TaskState},
};

use async_broadcast::Sender;

use hotshot_types::{
    consensus::{Consensus, View},
    data::{Leaf, QuorumProposal, VidDisperse},
    event::{Event, EventType},
    message::{GeneralConsensusMessage, Proposal},
    simple_certificate::{QuorumCertificate, TimeoutCertificate, UpgradeCertificate},
    simple_vote::{QuorumData, QuorumVote, TimeoutData, TimeoutVote},
    traits::{
        block_contents::BlockHeader,
        consensus_api::ConsensusApi,
        election::Membership,
        network::{ConnectedNetwork, ConsensusIntentEvent},
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
        signature_key::SignatureKey,
        states::ValidatedState,
        BlockPayload,
    },
    utils::{Terminator, ViewInner},
    vid::VidCommitment,
    vote::{Certificate, HasViewNumber},
};
use tracing::warn;

use crate::vote_collection::HandleVoteEvent;
use chrono::Utc;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    marker::PhantomData,
    sync::Arc,
};
#[cfg(async_executor_impl = "tokio")]
use tokio::task::JoinHandle;
use tracing::{debug, error, info, instrument};

enum VoteDepedency {
    QuorumProposal,
    Dac,
    Vid,
}

/// The state for the quorum vote task.
///
/// Contains all of the information for the quorum vote.
pub struct QuorumVoteTaskState<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    A: ConsensusApi<TYPES, I> + 'static,
> {
    pub next_vote_view: TYPES::Time,
    pub vote_dependencies: HashMap<TYPES::Time, AddDependency>,
    pub event_receiver: Receiver<HotShotEvent<TYPES>>,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>, A: ConsensusApi<TYPES, I> + 'static>
    ConsensusTaskState<TYPES, I, A>
{
    /// Validate the quorum proposal.
    // TODO: Complete the dependency implementation.
    // <https://github.com/EspressoSystems/HotShot/issues/2710>
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "Quorum vote validate proposal", level = "error")]
    fn validate_proposal(&mut self, _event_stream: &Sender<HotShotEvent<TYPES>>) -> bool {
        true
    }

    /// Validate the DAC.
    // TODO: Complete the dependency implementation.
    // <https://github.com/EspressoSystems/HotShot/issues/2710>
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "Quorum vote validate DAC", level = "error")]
    fn validate_dac(&mut self, _event_stream: &Sender<HotShotEvent<TYPES>>) -> bool {
        true
    }

    /// Validate the VID share.
    // TODO: Complete the dependency implementation.
    // <https://github.com/EspressoSystems/HotShot/issues/2710>
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "Quorum vote validate VID", level = "error")]
    fn validate_vid(&mut self, _event_stream: &Sender<HotShotEvent<TYPES>>) -> bool {
        true
    }

    /// Create an event dependency.
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "Quorum vote validate VID", level = "error")]
    fn create_event_dependency(
        &self,
        event: HotShotEvent,
        sender: &Sender<HotShotEvent<TYPES>>,
    ) -> EventDependency {
        EventDependency {
            event_rx: self.event_receiver,
            match_fn: Box::new(move |e| {
                if *e != event {
                    return false;
                }
                match event {
                    VoteDepedency::QuorumProposal => self.validate_proposal(event_stream, sender),
                    VoteDepedency::Dac => self.validate_dac(event_stream, sender),
                    VoteDepedency::Vid => self.validate_vid(event_stream, sender),
                }
            }),
        }
    }

    fn create_vote_dependency(
        validated_dependency: VoteDepedency,
        sender: &Sender<HotShotEvent<TYPES>>,
    ) -> AndDependency {
        let mut deps = Vec::new();
        if validated_dependency != VoteDepedency::Proposal {
            deps.add(self.create_event_dependency(VoteDepedency::QuorumProposal, sender));
        }
        if validated_dependency != VoteDepedency::Dac {
            deps.add(self.create_event_dependency(VoteDepedency::Dac, sender));
        }
        if validated_dependency != Vote::Dependency::Vid {
            deps.add(self.create_event_dependency(VoteDepedency::Vid, sender));
        }
        AndDependency::from(deps)
    }

    /// Update the view number for the next view to be voted for.
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "Quorum vote update next vote view", level = "error")]
    async fn update_next_vote_view(
        &mut self,
        new_view: TYPES::Time,
        event_stream: &Sender<HotShotEvent<TYPES>>,
    ) -> bool {
        if *self.cur_view < *new_view {
            debug!(
                "Updating view from {} to {} in the quorum vote task",
                *self.cur_view, *new_view
            );

            if *self.cur_view / 100 != *new_view / 100 {
                // TODO (https://github.com/EspressoSystems/HotShot/issues/2296):
                // switch to info! when INFO logs become less cluttered
                error!("Progress: entered view {:>6}", *new_view);
            }

            // Cancel the old dependency tasks.
            for (view, dependency) in self.vote_dependencies.iter_mut() {
                if view < new_view {
                    cancel_task(dependency.1).await;
                    vote_dependencies.remove(view);
                }
            }

            self.cur_view = new_view;

            return true;
        }
        false
    }

    /// Handles a consensus event received on the event stream
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "Consensus replica task", level = "error")]
    pub async fn handle(
        &mut self,
        event: HotShotEvent<TYPES>,
        event_sender: Sender<HotShotEvent<TYPES>>,
        event_receiver: Receiver<HotShotEvent<TYPES>>,
    ) {
        match event {
            HotShotEvent::QuorumProposalRecv(proposal, sender) => {
                let view = proposal.data.view_number;
                if view < next_vote_view {
                    return;
                }
                debug!("Received Quorum Proposal for view {}", *view);

                // stop polling for the received proposal
                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForProposal(*view))
                    .await;
                if self.vote_dependencies.get(view).is_none() {
                    if !self.validate_proposal(event_sender) {
                        return;
                    }
                    let dependency = create_vote_dependency(VoteDepedency::QuorumProposal, sender);
                    self.vote_dependencies = insert(view, dependency);
                    async_spawn(async move {
                        dependency.completed().await;
                        self.next_vote_view = view + 1;
                        for (view, _) in self.vote_dependencies.iter() {
                            if view < self.next_vote_view {
                                self.vote_dependencies.remove(view)
                            }
                        }
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

                if self.vote_dependencies.get(view).is_none() {
                    if !self.validate_dac(event_sender) {
                        return;
                    }
                    let dependency = create_vote_dependency(VoteDepedency::Dac, sender);
                    self.vote_dependencies = insert(view, dependency);
                    async_spawn(async move {
                        dependency.completed().await;
                        self.next_vote_view = view + 1;
                        for (view, _) in self.vote_dependencies.iter() {
                            if view < self.next_vote_view {
                                self.vote_dependencies.remove(view)
                            }
                        }
                    });
                }
            }
            HotShotEvent::VidDisperseRecv(disperse, sender) => {
                let view = disperse.data.get_view_number();

                // stop polling for the received disperse after verifying it's valid
                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForVIDDisperse(
                        *disperse.data.view_number,
                    ))
                    .await;

                if self.vote_dependencies.get(view).is_none() {
                    if !self.validate_proposal(event_sender) {
                        return;
                    }
                    let dependency = create_vote_dependency(VoteDepedency::Vid, sender);
                    self.vote_dependencies = insert(view, dependency);
                    async_spawn(async move {
                        dependency.completed().await;
                        self.next_vote_view = view + 1;
                        for (view, _) in self.vote_dependencies.iter() {
                            if view < self.next_vote_view {
                                self.vote_dependencies.remove(view)
                            }
                        }
                    });
                }
            }
            HotShotEvent::ViewChange(new_view) => {
                debug!("View Change event for view {} in consensus task", *new_view);

                let old_view_number = self.cur_view;

                // Start polling for VID disperse for the new view
                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::PollForVIDDisperse(
                        *old_view_number + 1,
                    ))
                    .await;

                // update the view in state to the one in the message
                // Publish a view change event to the application
                if !self.update_next_vote_view(new_view, &event_stream).await {
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

impl<TYPES: NodeType, I: NodeImplementation<TYPES>, A: ConsensusApi<TYPES, I> + 'static> TaskState
    for QuorumVoteTaskState<TYPES, I, A>
{
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
        let receiver = task.subscribe();
        tracing::trace!("sender queue len {}", sender.len());
        task.state_mut().handle(event, sender, receiver).await;
        None
    }
    fn should_shutdown(event: &Self::Event) -> bool {
        matches!(event, HotShotEvent::Shutdown)
    }
}
