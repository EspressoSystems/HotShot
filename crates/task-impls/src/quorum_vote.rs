use crate::{
    events::HotShotEvent,
    helpers::{broadcast_event, cancel_task},
};
use async_broadcast::{Receiver, Sender};
use async_lock::RwLock;
#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
use commit::Committable;
use hotshot_task::{
    dependency::{AndDependency, EventDependency},
    dependency_task::{DependencyTask, HandleDepOutput},
    task::{Task, TaskState},
};
use hotshot_types::{
    consensus::Consensus,
    data::{Leaf, QuorumProposal},
    event::Event,
    message::{GeneralConsensusMessage, Proposal},
    simple_vote::{QuorumData, QuorumVote},
    traits::{
        block_contents::BlockHeader,
        election::Membership,
        network::{ConnectedNetwork, ConsensusIntentEvent},
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
        signature_key::SignatureKey,
        storage::Storage,
    },
    vote::{Certificate, HasViewNumber},
};
use std::collections::HashMap;
use std::sync::Arc;
#[cfg(async_executor_impl = "tokio")]
use tokio::task::JoinHandle;
use tracing::{debug, error, instrument, warn};

/// Vote dependency types.
#[derive(Debug, PartialEq)]
enum VoteDependency {
    /// For the `QuorumProposalRecv` event.
    QuorumProposal,
    /// For the `DACertificateRecv` event.
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
    _event_sender: Sender<Arc<HotShotEvent<TYPES>>>,
) -> bool {
    true
}

/// Handler for the vote dependency.
struct VoteDependencyHandle<TYPES: NodeType> {
    /// Public key.
    pub public_key: TYPES::SignatureKey,
    /// Private Key.
    pub private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
    /// View number to vote on.
    view_number: TYPES::Time,
    /// Event sender.
    sender: Sender<Arc<HotShotEvent<TYPES>>>,
}
impl<TYPES: NodeType> HandleDepOutput for VoteDependencyHandle<TYPES> {
    type Output = Vec<Arc<HotShotEvent<TYPES>>>;
    async fn handle_dep_result(self, res: Self::Output) {
        let mut payload_commitment = None;
        let mut leaf = None;
        for event in res {
            match event.as_ref() {
                HotShotEvent::QuorumProposalValidated(proposal, parent_leaf) => {
                    let proposal_payload_comm = proposal.block_header.payload_commitment();
                    if let Some(comm) = payload_commitment {
                        if proposal_payload_comm != comm {
                            error!("Quorum proposal has inconsistent payload commitment with DAC or VID.");
                            return;
                        }
                    } else {
                        payload_commitment = Some(proposal_payload_comm);
                    }
                    let parent_commitment = parent_leaf.commit();
                    let mut proposed_leaf = Leaf::from_quorum_proposal(proposal);
                    proposed_leaf.set_parent_commitment(parent_commitment);
                    leaf = Some(proposed_leaf);
                }
                HotShotEvent::DACertificateValidated(cert) => {
                    let cert_payload_comm = cert.get_data().payload_commit;
                    if let Some(comm) = payload_commitment {
                        if !cert.is_genesis && cert_payload_comm != comm {
                            error!("DAC has inconsistent payload commitment with quorum proposal or VID.");
                            return;
                        }
                    } else {
                        payload_commitment = Some(cert_payload_comm);
                    }
                }
                HotShotEvent::VIDShareValidated(share) => {
                    let vid_payload_commitment = share.payload_commitment;
                    if let Some(comm) = payload_commitment {
                        if vid_payload_commitment != comm {
                            error!("VID has inconsistent payload commitment with quorum proposal or DAC.");
                            return;
                        }
                    } else {
                        payload_commitment = Some(vid_payload_commitment);
                    }
                }
                _ => {}
            }
        }
        broadcast_event(
            Arc::new(HotShotEvent::QuorumVoteDependenciesValidated(
                self.view_number,
            )),
            &self.sender,
        )
        .await;

        // Create and send the vote.
        let Some(leaf) = leaf else {
            error!("Quorum proposal isn't validated, but it should be.");
            return;
        };
        let message = if let Ok(vote) = QuorumVote::<TYPES>::create_signed_vote(
            QuorumData {
                leaf_commit: leaf.commit(),
            },
            self.view_number,
            &self.public_key,
            &self.private_key,
        ) {
            GeneralConsensusMessage::<TYPES>::Vote(vote)
        } else {
            error!("Unable to sign quorum vote!");
            return;
        };
        if let GeneralConsensusMessage::Vote(vote) = message {
            debug!(
                "Sending vote to next quorum leader {:?}",
                vote.get_view_number() + 1
            );
            broadcast_event(Arc::new(HotShotEvent::QuorumVoteSend(vote)), &self.sender).await;
        }
    }
}

/// The state for the quorum vote task.
///
/// Contains all of the information for the quorum vote.
pub struct QuorumVoteTaskState<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Public key.
    pub public_key: TYPES::SignatureKey,

    /// Private Key.
    pub private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,

    /// Reference to consensus. The replica will require a write lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES>>>,

    /// Latest view number that has been voted for.
    pub latest_voted_view: TYPES::Time,

    /// Table for the in-progress dependency tasks.
    pub vote_dependencies: HashMap<TYPES::Time, JoinHandle<()>>,

    /// Network for all nodes
    pub quorum_network: Arc<I::QuorumNetwork>,

    /// Network for DA committee
    pub committee_network: Arc<I::CommitteeNetwork>,

    /// Membership for Quorum certs/votes.
    pub quorum_membership: Arc<TYPES::Membership>,

    /// Membership for DA committee certs/votes.
    pub da_membership: Arc<TYPES::Membership>,

    /// Output events to application
    pub output_event_stream: async_broadcast::Sender<Event<TYPES>>,

    /// The node's id
    pub id: u64,

    /// Reference to the storage.
    pub storage: Arc<RwLock<I::Storage>>,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> QuorumVoteTaskState<TYPES, I> {
    /// Create an event dependency.
    #[instrument(skip_all, fields(id = self.id, latest_voted_view = *self.latest_voted_view), name = "Quorum vote create event dependency", level = "error")]
    fn create_event_dependency(
        &self,
        dependency_type: VoteDependency,
        view_number: TYPES::Time,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
    ) -> EventDependency<Arc<HotShotEvent<TYPES>>> {
        EventDependency::new(
            event_receiver.clone(),
            Box::new(move |event| {
                let event = event.as_ref();
                let event_view = match dependency_type {
                    VoteDependency::QuorumProposal => {
                        if let HotShotEvent::QuorumProposalValidated(proposal, _) = event {
                            proposal.view_number
                        } else {
                            return false;
                        }
                    }
                    VoteDependency::Dac => {
                        if let HotShotEvent::DACertificateValidated(cert) = event {
                            cert.view_number
                        } else {
                            return false;
                        }
                    }
                    VoteDependency::Vid => {
                        if let HotShotEvent::VIDShareValidated(disperse) = event {
                            disperse.view_number
                        } else {
                            return false;
                        }
                    }
                };
                if event_view == view_number {
                    debug!("Vote dependency {:?} completed", dependency_type);
                    return true;
                }
                false
            }),
        )
    }

    /// Create and store an [`AndDependency`] combining [`EventDependency`]s associated with the
    /// given view number if it doesn't exist.
    #[instrument(skip_all, fields(id = self.id, latest_voted_view = *self.latest_voted_view), name = "Quorum vote crete dependency task if new", level = "error")]
    fn create_dependency_task_if_new(
        &mut self,
        view_number: TYPES::Time,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
        event_sender: &Sender<Arc<HotShotEvent<TYPES>>>,
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
                public_key: self.public_key.clone(),
                private_key: self.private_key.clone(),
                view_number,
                sender: event_sender.clone(),
            },
        );
        self.vote_dependencies
            .insert(view_number, dependency_task.run());
    }

    /// Update the latest voted view number.
    #[instrument(skip_all, fields(id = self.id, latest_voted_view = *self.latest_voted_view), name = "Quorum vote update latest voted view", level = "error")]
    async fn update_latest_voted_view(&mut self, new_view: TYPES::Time) -> bool {
        if *self.latest_voted_view < *new_view {
            debug!(
                "Updating next vote view from {} to {} in the quorum vote task",
                *self.latest_voted_view, *new_view
            );

            // Cancel the old dependency tasks.
            for view in (*self.latest_voted_view + 1)..=(*new_view) {
                if let Some(dependency) = self.vote_dependencies.remove(&TYPES::Time::new(view)) {
                    cancel_task(dependency).await;
                    debug!("Vote dependency removed for view {:?}", view);
                }
            }

            self.latest_voted_view = new_view;

            return true;
        }
        false
    }

    /// Handle a vote dependent event received on the event stream
    #[instrument(skip_all, fields(id = self.id, latest_voted_view = *self.latest_voted_view), name = "Quorum vote handle", level = "error")]
    pub async fn handle(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
        event_sender: Sender<Arc<HotShotEvent<TYPES>>>,
    ) {
        match event.as_ref() {
            HotShotEvent::QuorumProposalRecv(proposal, _sender) => {
                let view = proposal.data.view_number;
                debug!("Received Quorum Proposal for view {}", *view);
                if view <= self.latest_voted_view {
                    return;
                }

                // stop polling for the received proposal
                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForProposal(*view))
                    .await;
                broadcast_event(Arc::new(HotShotEvent::ViewChange(view + 1)), &event_sender).await;

                // Validate the quorum proposal.
                if !validate_quorum_proposal(proposal.clone(), event_sender.clone()) {
                    return;
                }

                // Vaildate the justify QC.
                let justify_qc = proposal.data.justify_qc.clone();
                if !justify_qc.is_valid_cert(self.quorum_membership.as_ref()) {
                    error!("Invalid justify_qc in proposal for view {}", *view);
                    let consensus = self.consensus.write().await;
                    consensus.metrics.invalid_qc.update(1);
                    return;
                }
                let consensus = self.consensus.read().await;
                let parent = if justify_qc.is_genesis {
                    Some(Leaf::genesis(&consensus.instance_state))
                } else {
                    consensus
                        .saved_leaves
                        .get(&justify_qc.get_data().leaf_commit)
                        .cloned()
                };
                drop(consensus);
                // Justify qc's leaf commitment is not the same as the parent's leaf commitment, but it should be (in this case)
                let Some(parent) = parent else {
                    warn!(
                                "Proposal's parent missing from storage with commitment: {:?}, proposal view {:?}",
                                justify_qc.get_data().leaf_commit,
                                *view,
                            );
                    return;
                };

                broadcast_event(
                    Arc::new(HotShotEvent::QuorumProposalValidated(
                        proposal.data.clone(),
                        parent.clone(),
                    )),
                    &event_sender.clone(),
                )
                .await;
                self.create_dependency_task_if_new(view, event_receiver, &event_sender);
            }
            HotShotEvent::DACertificateRecv(cert) => {
                let view = cert.view_number;
                debug!("Received DAC for view {}", *view);
                if view <= self.latest_voted_view {
                    return;
                }

                // Validate the DAC.
                if !cert.is_valid_cert(self.da_membership.as_ref()) {
                    return;
                }

                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForDAC(*view))
                    .await;

                self.committee_network
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForVotes(*view))
                    .await;

                // Add to the storage.
                self.consensus
                    .write()
                    .await
                    .saved_da_certs
                    .insert(view, cert.clone());

                broadcast_event(
                    Arc::new(HotShotEvent::DACertificateValidated(cert.clone())),
                    &event_sender.clone(),
                )
                .await;
                self.create_dependency_task_if_new(view, event_receiver, &event_sender);
            }
            HotShotEvent::VidDisperseRecv(disperse) => {
                let view = disperse.data.get_view_number();
                debug!("Received VID share for view {}", *view);
                if view <= self.latest_voted_view {
                    return;
                }

                // Validate the VID share.
                let payload_commitment = disperse.data.payload_commitment;
                // Check whether the data satisfies one of the following.
                // * From the right leader for this view.
                // * Calculated and signed by the current node.
                // * Signed by one of the staked DA committee members.
                if !self
                    .quorum_membership
                    .get_leader(view)
                    .validate(&disperse.signature, payload_commitment.as_ref())
                    && !self
                        .public_key
                        .validate(&disperse.signature, payload_commitment.as_ref())
                {
                    let mut validated = false;
                    for da_member in self.da_membership.get_staked_committee(view) {
                        if da_member.validate(&disperse.signature, payload_commitment.as_ref()) {
                            validated = true;
                            break;
                        }
                    }
                    if !validated {
                        return;
                    }
                }

                // stop polling for the received disperse after verifying it's valid
                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForVIDDisperse(
                        *disperse.data.view_number,
                    ))
                    .await;

                // Add to the storage.
                if let Err(e) = self.storage.write().await.append_vid(disperse).await {
                    error!("Failed to store VID share with error {:?}", e);
                    return;
                }
                self.consensus
                    .write()
                    .await
                    .vid_shares
                    .entry(view)
                    .or_default()
                    .insert(disperse.data.recipient_key.clone(), disperse.clone());

                broadcast_event(
                    Arc::new(HotShotEvent::VIDShareValidated(disperse.data.clone())),
                    &event_sender.clone(),
                )
                .await;
                self.create_dependency_task_if_new(view, event_receiver, &event_sender);
            }
            HotShotEvent::QuorumVoteDependenciesValidated(view) => {
                // TODO(Keyao): Update view after voting?
                debug!("All vote dependencies verified for view {:?}", view);
                if !self.update_latest_voted_view(*view).await {
                    debug!("view not updated");
                    return;
                }
            }
            HotShotEvent::ViewChange(new_view) => {
                let new_view = *new_view;
                debug!(
                    "View Change event for view {} in quorum vote task",
                    *new_view
                );

                let old_voted_view = self.latest_voted_view;

                // Start polling for VID disperse for the new view
                self.quorum_network
                    .inject_consensus_info(ConsensusIntentEvent::PollForVIDDisperse(
                        *old_voted_view + 1,
                    ))
                    .await;
            }
            _ => {}
        }
    }
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> TaskState for QuorumVoteTaskState<TYPES, I> {
    type Event = Arc<HotShotEvent<TYPES>>;
    type Output = ();
    fn filter(&self, event: &Arc<HotShotEvent<TYPES>>) -> bool {
        !matches!(
            event.as_ref(),
            HotShotEvent::QuorumProposalRecv(_, _)
                | HotShotEvent::DACertificateRecv(_)
                | HotShotEvent::ViewChange(_)
                | HotShotEvent::VidDisperseRecv(..)
                | HotShotEvent::QuorumVoteDependenciesValidated(_)
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
