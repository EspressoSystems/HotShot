use commit::Committable;
use std::{collections::HashMap, marker::PhantomData, sync::Arc};

use anyhow::{bail, Result};
use async_broadcast::{Receiver, Sender};
use async_lock::RwLock;
use either::Either;
use hotshot_task::{
    dependency::{AndDependency, EventDependency, OrDependency},
    dependency_task::{DependencyTask, HandleDepOutput},
    task::{Task, TaskState},
};
use hotshot_types::{
    consensus::Consensus,
    data::{Leaf, QuorumProposal, ViewChangeEvidence},
    event::Event,
    message::Proposal,
    simple_certificate::{UpgradeCertificate, ViewSyncFinalizeCertificate2},
    traits::{
        block_contents::BlockHeader,
        election::Membership,
        network::{ConnectedNetwork, ConsensusIntentEvent},
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
        signature_key::SignatureKey,
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

    /// Membership for Quorum Certs/votes
    quorum_membership: Arc<TYPES::Membership>,

    /// Our public key
    public_key: TYPES::SignatureKey,

    /// Our Private Key
    private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,

    /// last Upgrade Certificate this node formed
    upgrade_cert: Option<UpgradeCertificate<TYPES>>,
}

impl<TYPES: NodeType> ProposalDependencyHandle<TYPES> {
    /// Checks if we have somehow initiated a proposal while not being the leader of the next
    /// `view`.
    fn check_leader(&self, view: TYPES::Time) -> Result<()> {
        if self.quorum_membership.get_leader(view) != self.public_key {
            // This is expected for view 1, so skipping the logging.
            if view != TYPES::Time::new(1) {
                bail!(
                    "Somehow we formed a QC but are not the leader for the next view {:?}",
                    view
                );
            }
        }
        Ok(())
    }

    /// Find the leaf and state of the parent view to the view that we're currently proposing on.
    fn find_parent_leaf_and_state(
        consensus: &Consensus<TYPES>,
    ) -> Result<(Leaf<TYPES>, Arc<TYPES::ValidatedState>)> {
        let parent_view_number = &consensus.high_qc.get_view_number();

        let Some(parent_view) = consensus.validated_state_map.get(parent_view_number) else {
            // This should have been added by the replica?
            bail!("Couldn't find parent view in state map, waiting for replica to see proposal\n parent view number: {}", **parent_view_number);
        };

        // Leaf hash in view inner does not match high qc hash - Why?
        let Some((leaf_commitment, state)) = parent_view.get_leaf_and_state() else {
            bail!(
                "Parent of high QC points to a view without a proposal; parent_view_number: {}, parent_view: {:?}",
                **parent_view_number,
                parent_view,
            );
        };

        if leaf_commitment != consensus.high_qc.get_data().leaf_commit {
            // NOTE: This happens on the genesis block
            debug!(
                "Parent leaf commitment does not equal high qc leaf commitment: {:?} != {:?}",
                leaf_commitment,
                consensus.high_qc.get_data().leaf_commit
            );
        }
        let Some(leaf) = consensus.saved_leaves.get(&leaf_commitment) else {
            bail!("Failed to find high QC of parent.");
        };

        Ok((leaf.clone(), state.clone()))
    }

    /// Publishes a proposal from a completed payload commitment and metadata and one of several
    /// optional additional pieces of data which may be attached to the proposal.
    async fn publish_proposal(
        &self,
        view: TYPES::Time,
        event_stream: &Sender<Arc<HotShotEvent<TYPES>>>,
        commit_and_metadata: CommitmentAndMetadata<TYPES::BlockPayload>,
        proposal_cert: Option<ViewChangeEvidence<TYPES>>,
    ) -> Result<()> {
        self.check_leader(view)?;
        let consensus = self.consensus.read().await;
        let (leaf, state) = Self::find_parent_leaf_and_state(&consensus)?;
        let reached_decided = leaf.get_view_number() == consensus.last_decided_view;

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

        let block_header = TYPES::BlockHeader::new(
            &state,
            &consensus.instance_state,
            &parent_leaf,
            commit_and_metadata.commitment,
            commit_and_metadata.metadata.clone(),
        )
        .await;

        let upgrade_cert = if self
            .upgrade_cert
            .as_ref()
            .is_some_and(|cert| cert.view_number == view)
        {
            // If the cert view number matches, set upgrade_cert to self.upgrade_cert
            // and set self.upgrade_cert to None.
            //
            // Note: the certificate is discarded, regardless of whether the vote on the proposal succeeds or not.
            self.upgrade_cert.clone()
        } else {
            // Otherwise, set upgrade_cert to None.
            None
        };

        // We only want the proposal to be attached if any of them are valid.
        let proposal_certificate = proposal_cert
            .as_ref()
            .filter(|cert| cert.is_valid_for_view(&view))
            .cloned();

        let proposal = QuorumProposal {
            block_header,
            view_number: view,
            justify_qc: consensus.high_qc.clone(),
            proposal_certificate,
            upgrade_certificate: upgrade_cert,
        };

        let mut new_leaf = Leaf::from_quorum_proposal(&proposal);
        new_leaf.set_parent_commitment(parent_leaf.commit());

        let Ok(signature) =
            TYPES::SignatureKey::sign(&self.private_key, new_leaf.commit().as_ref())
        else {
            bail!("Failed to sign leaf.commit()!");
        };

        let message = Proposal {
            data: proposal,
            signature,
            _pd: PhantomData,
        };
        debug!("Sending proposal for view {:?}", view);

        broadcast_event(
            Arc::new(HotShotEvent::DummyQuorumProposalSend(
                message.clone(),
                self.public_key.clone(),
            )),
            event_stream,
        )
        .await;

        Ok(())
    }
}

impl<TYPES: NodeType> HandleDepOutput for ProposalDependencyHandle<TYPES> {
    type Output = Vec<Vec<Arc<HotShotEvent<TYPES>>>>;

    #[allow(clippy::no_effect_underscore_binding)]
    async fn handle_dep_result(self, res: Self::Output) {
        let mut commit_and_metadata: Option<CommitmentAndMetadata<TYPES::BlockPayload>> = None;
        let mut timeout_certificate = None;
        let mut view_sync_finalize_cert = None;
        for event in res.iter().flatten() {
            match event.as_ref() {
                /* TODO Add QuorumProposalValidated */
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
                        timeout_certificate = Some(timeout.clone());
                    }
                    either::Left(_) => { /* unhandled */ }
                },
                HotShotEvent::ViewSyncFinalizeCertificate2Recv(cert) => {
                    view_sync_finalize_cert = Some(cert.clone());
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

        let view_change_evidence = {
            if let Some(timeout_certificate) = timeout_certificate {
                Some(ViewChangeEvidence::Timeout(timeout_certificate))
            } else {
                view_sync_finalize_cert.map(ViewChangeEvidence::ViewSync)
            }
        };

        broadcast_event(
            Arc::new(HotShotEvent::QuorumProposalDependenciesValidated(
                self.view_number,
            )),
            &self.sender,
        )
        .await;

        if let Err(e) = self
            .publish_proposal(
                self.view_number,
                &self.sender,
                commit_and_metadata.unwrap(),
                view_change_evidence,
            )
            .await
        {
            error!("Failed to propse; error = {:?}", e);
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

    /// last Upgrade Certificate this node formed
    pub upgrade_cert: Option<UpgradeCertificate<TYPES>>,

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

                    ProposalDependency::Proposal => {
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

                // NOTE: This will *not* emit when a dependency was short circuited on creation.
                // This is only for dependencies that were awaiting a result.
                if valid {
                    debug!("Depencency {:?} is complete!", dependency_type);
                }
                valid
            }),
        )
    }

    /// Create and store an [`AndDependency`] of [`OrDependency`]s combining
    /// [`EventDependency`]s associated with the given view number if it doesn't exist.
    /// Also takes in the received `event` to seed a
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
            HotShotEvent::SendPayloadCommitmentAndMetadata(_, _, _) => {
                payload_commitment_dependency.mark_as_completed(event.clone());
            }
            HotShotEvent::QuorumProposalValidated(_) => {
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
        let combined = AndDependency::from_deps(vec![
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
        ]);

        // We only want to pass down the upgrade cert if we already have one *and* it is for this
        // view, otherwise we can just ignore it.
        let upgrade_cert = match &self.upgrade_cert {
            Some(cert) => {
                // Is this cert for the correct view?
                if cert.view_number == view_number {
                    Some(cert.clone())
                } else {
                    None
                }
            }
            None => None,
        };

        let dependency_task = DependencyTask::new(
            combined,
            ProposalDependencyHandle {
                view_number,
                sender: event_sender,
                consensus: self.consensus.clone(),
                quorum_membership: self.quorum_membership.clone(),
                public_key: self.public_key.clone(),
                private_key: self.private_key.clone(),
                upgrade_cert,
            },
        );
        self.propose_dependencies
            .insert(view_number, dependency_task.run());
    }

    /// Update the latest proposed view number. This function destructively removes all old
    /// dependency tasks (tasks whose view is < the latest view).
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
            self.upgrade_cert = None;

            return true;
        }
        false
    }

    /// Validate a view sync cert or a timeout cert.
    fn validate_view_sync_finalize_certificate(
        &self,
        certificate: &ViewSyncFinalizeCertificate2<TYPES>,
    ) -> bool {
        certificate.is_valid_cert(self.quorum_membership.as_ref())
    }

    /// Handles a consensus event received on the event stream. A notable deviation between the
    /// original implementation of this code in the `ConsensusTask` is the handling of inbound
    /// events which *initiate* a proposal. In this case, when an event arrives for a previously
    /// unseen view (> than the latest proposed view), we create a background dependency task for
    /// that view, which allows for the collection of events which, when satisfied, initiates a
    /// proposal. The code works according to the following flow:
    /// 1. Is the received event for a newer view than we've ever seen? Create a task.
    ///
    /// 2. Is the event received for an event we're currently collecting events for? Let the
    ///    dependency task handle it, as it will make sure that the data is valid and corresponds
    ///    to the correct view.
    /// Lastly, in option 1, we don't want to lose access to the available data, so we clone the
    /// `event` and initialize the dependency task with the data from it to ensure that the
    /// information is not lost when we start the dependency, this allows us to instantly mark one
    /// of the requirements as complete.
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view), name = "Quorum proposal handle", level = "error")]
    pub async fn handle(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
        event_sender: Sender<Arc<HotShotEvent<TYPES>>>,
    ) {
        match event.as_ref() {
            HotShotEvent::UpgradeCertificateFormed(cert) => {
                debug!(
                    "Upgrade certificate received for view {}!",
                    *cert.view_number
                );

                // Update our current upgrade_cert as long as it's still relevant.
                if cert.view_number >= self.latest_proposed_view {
                    self.upgrade_cert = Some(cert.clone());
                }
            }
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
