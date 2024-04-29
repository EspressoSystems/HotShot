use std::{collections::HashMap, marker::PhantomData, sync::Arc, time::Duration};

use async_broadcast::{Receiver, Sender};
use async_compatibility_layer::art::async_sleep;
use async_lock::RwLock;
#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
use committable::Committable;
use either::Either;
use hotshot_task::{
    dependency::{AndDependency, EventDependency, OrDependency},
    dependency_task::{DependencyTask, HandleDepOutput},
    task::{Task, TaskState},
};
use hotshot_types::{
    consensus::{CommitmentAndMetadata, Consensus},
    data::{Leaf, QuorumProposal},
    event::Event,
    message::Proposal,
    traits::{
        block_contents::BlockHeader,
        election::Membership,
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
        signature_key::SignatureKey,
        storage::Storage,
    },
    vote::{Certificate, HasViewNumber},
};
#[cfg(async_executor_impl = "tokio")]
use tokio::task::JoinHandle;
use tracing::{debug, error, info, instrument, warn};

#[cfg(feature = "dependency-tasks")]
use crate::consensus::proposal_helpers::handle_quorum_proposal_validated;
use crate::{
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

    /// For the `ProposeNow` event.
    ProposeNow,
}

/// Handler for the proposal dependency
struct ProposalDependencyHandle<TYPES: NodeType> {
    /// The view number to propose for.
    view_number: TYPES::Time,

    /// The event sender.
    sender: Sender<Arc<HotShotEvent<TYPES>>>,

    /// Reference to consensus. The replica will require a write lock on this.
    consensus: Arc<RwLock<Consensus<TYPES>>>,

    /// Immutable instance state
    instance_state: Arc<TYPES::InstanceState>,

    /// Output events to application
    #[allow(dead_code)]
    output_event_stream: async_broadcast::Sender<Event<TYPES>>,

    /// Membership for Timeout votes/certs
    #[allow(dead_code)]
    timeout_membership: Arc<TYPES::Membership>,

    /// Membership for Quorum Certs/votes
    quorum_membership: Arc<TYPES::Membership>,

    /// Our public key
    public_key: TYPES::SignatureKey,

    /// Our Private Key
    private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,

    /// View timeout from config.
    #[allow(dead_code)]
    timeout: u64,

    /// Round start delay from config, in milliseconds.
    round_start_delay: u64,

    /// The node's id
    #[allow(dead_code)]
    id: u64,
}

impl<TYPES: NodeType> ProposalDependencyHandle<TYPES> {
    /// Sends a proposal if possible from the high qc we have
    #[allow(clippy::too_many_lines)]
    pub async fn publish_proposal_if_able(
        &self,
        view: TYPES::Time,
        event_stream: &Sender<Arc<HotShotEvent<TYPES>>>,
        commit_and_metadata: CommitmentAndMetadata<TYPES>,
        instance_state: Arc<TYPES::InstanceState>,
    ) -> bool {
        if self.quorum_membership.get_leader(view) != self.public_key {
            // This is expected for view 1, so skipping the logging.
            if view != TYPES::Time::new(1) {
                error!(
                    "Somehow we formed a QC but are not the leader for the next view {:?}",
                    view
                );
            }
            return false;
        }

        let consensus = self.consensus.read().await;
        let parent_view_number = &consensus.high_qc.get_view_number();
        let mut reached_decided = false;

        let Some(parent_view) = consensus.validated_state_map.get(parent_view_number) else {
            // This should have been added by the replica?
            error!("Couldn't find parent view in state map, waiting for replica to see proposal\n parent view number: {}", **parent_view_number);
            return false;
        };
        // Leaf hash in view inner does not match high qc hash - Why?
        let Some((leaf_commitment, state)) = parent_view.get_leaf_and_state() else {
            error!(
                ?parent_view_number,
                ?parent_view,
                "Parent of high QC points to a view without a proposal"
            );
            return false;
        };
        if leaf_commitment != consensus.high_qc.get_data().leaf_commit {
            // NOTE: This happens on the genesis block
            debug!(
                "They don't equal: {:?}   {:?}",
                leaf_commitment,
                consensus.high_qc.get_data().leaf_commit
            );
        }
        let Some(leaf) = consensus.saved_leaves.get(&leaf_commitment) else {
            error!("Failed to find high QC of parent.");
            return false;
        };
        if leaf.get_view_number() == consensus.last_decided_view {
            reached_decided = true;
        }

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

        // Special case: if we have a decided upgrade certificate AND it does not apply a version to the current view, we MUST propose with a null block.
        let block_header = TYPES::BlockHeader::new(
            state,
            instance_state.as_ref(),
            &parent_leaf,
            commit_and_metadata.commitment,
            commit_and_metadata.builder_commitment,
            commit_and_metadata.metadata,
            commit_and_metadata.fee,
        )
        .await;

        // TODO: DA cert is sent as part of the proposal here, we should split this out so we don't have to wait for it.
        let proposal = QuorumProposal {
            block_header: block_header.clone(),
            view_number: view,
            justify_qc: consensus.high_qc.clone(),
            proposal_certificate: None,
            upgrade_certificate: None,
        };

        let proposed_leaf = Leaf::from_quorum_proposal(&proposal);

        let Ok(signature) =
            TYPES::SignatureKey::sign(&self.private_key, proposed_leaf.commit().as_ref())
        else {
            error!("Failed to sign new_leaf.commit()!");
            return false;
        };

        let message = Proposal {
            data: proposal,
            signature,
            _pd: PhantomData,
        };
        debug!(
            "Sending null proposal for view {:?} \n {:?}",
            proposed_leaf.get_view_number(),
            ""
        );

        async_sleep(Duration::from_millis(self.round_start_delay)).await;
        broadcast_event(
            Arc::new(HotShotEvent::QuorumProposalSend(
                message.clone(),
                self.public_key.clone(),
            )),
            event_stream,
        )
        .await;
        true
    }
}

impl<TYPES: NodeType> HandleDepOutput for ProposalDependencyHandle<TYPES> {
    type Output = Vec<Vec<Vec<Arc<HotShotEvent<TYPES>>>>>;

    #[allow(clippy::no_effect_underscore_binding)]
    async fn handle_dep_result(self, res: Self::Output) {
        let mut payload_commitment = None;
        let mut commit_and_metadata: Option<CommitmentAndMetadata<TYPES>> = None;
        let mut _quorum_certificate = None;
        let mut _timeout_certificate = None;
        let mut _view_sync_finalize_cert = None;
        for event in res.iter().flatten().flatten() {
            match event.as_ref() {
                HotShotEvent::QuorumProposalValidated(proposal, _) => {
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
                    builder_commitment,
                    metadata,
                    view,
                    fee,
                ) => {
                    debug!("Got commit and meta {:?}", payload_commitment);
                    commit_and_metadata = Some(CommitmentAndMetadata {
                        commitment: *payload_commitment,
                        builder_commitment: builder_commitment.clone(),
                        metadata: metadata.clone(),
                        fee: fee.clone(),
                        block_view: *view,
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
                HotShotEvent::ProposeNow(_, pdd) => {
                    commit_and_metadata = Some(pdd.commitment_and_metadata.clone());
                    match &pdd.secondary_proposal_information {
                        hotshot_types::consensus::SecondaryProposalInformation::QuorumProposalAndCertificate(quorum_proposal, quorum_certificate) => {
                            _quorum_certificate = Some(quorum_certificate.clone());
                            payload_commitment = Some(quorum_proposal.block_header.payload_commitment());
                        },
                        hotshot_types::consensus::SecondaryProposalInformation::Timeout(tc) => {
                            _timeout_certificate = Some(tc.clone());
                        }
                        hotshot_types::consensus::SecondaryProposalInformation::ViewSync(vsc) => {
                            _view_sync_finalize_cert = Some(vsc.clone());
                        },
                    }
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

        self.publish_proposal_if_able(
            self.view_number,
            &self.sender,
            commit_and_metadata.unwrap(),
            Arc::clone(&self.instance_state),
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

    /// The view number that this node is executing in.
    pub cur_view: TYPES::Time,

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
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view, view = *self.cur_view), name = "Quorum proposal create event dependency", level = "error")]
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
                            warn!(
                                "QC View number {} View number {}",
                                *qc.view_number, *view_number
                            );
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
                    info!("Dependency {:?} is complete!", dependency_type);
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

        info!(
            "Node {} Dependency {:?} is complete for view {}!",
            self.id, event, *view_number
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
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view, view = *self.cur_view), name = "Quorum proposal create dependency task", level = "error")]
    fn create_dependency_task_if_new(
        &mut self,
        view_number: TYPES::Time,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
        event_sender: Sender<Arc<HotShotEvent<TYPES>>>,
        event: Arc<HotShotEvent<TYPES>>,
    ) {
        info!("Attempting to make dependency task for event {:?}", event);
        if self.propose_dependencies.get(&view_number).is_some() {
            debug!("Task already exists");
            return;
        }

        let dependency_chain =
            self.create_and_complete_dependencies(view_number, event_receiver, event);

        let dependency_task = DependencyTask::new(
            dependency_chain,
            ProposalDependencyHandle {
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
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view), name = "Quorum proposal update latest proposed view", level = "error")]
    async fn update_latest_proposed_view(&mut self, new_view: TYPES::Time) -> bool {
        if *self.latest_proposed_view < *new_view {
            info!(
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

    /// Handles a consensus event received on the event stream
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view), name = "Quorum proposal handle", level = "error")]
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
                        info!("Making QC Timeout dependency for view {view:?}");

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
                            warn!("Failed to store High QC of QC we formed. Error: {:?}", e);
                        }

                        let mut consensus = self.consensus.write().await;
                        consensus.high_qc = qc.clone();
                        // We need to drop our handle here to make the borrow checker happy.
                        drop(consensus);

                        let view = qc.view_number + 1;
                        info!("Making QC Dependency for view {view:?}");

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
                payload_commitment,
                _builder_commitment,
                _metadata,
                view,
                _fee,
            ) => {
                let view = *view;
                debug!(
                    "Got payload commitment {:?} for view {view:?}",
                    payload_commitment
                );

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

                if self.quorum_membership.get_leader(view) == self.public_key {
                    debug!(
                        "Attempting to publish proposal after forming a View Sync Finalized Cert for view {}",
                        *certificate.view_number
                    );
                    self.create_dependency_task_if_new(view, event_receiver, event_sender, event);
                }
            }
            #[cfg(feature = "dependency-tasks")]
            HotShotEvent::QuorumProposalValidated(proposal, _) => {
                let new_view = proposal.view_number + 1;

                if let Err(e) =
                    handle_quorum_proposal_validated(proposal, event_sender.clone(), self).await
                {
                    debug!("Failed to handle QuorumProposalValidated event; error = {e:#}");
                }

                info!(
                    "Node {} creating dependency task for view {:?} from QuorumProposalRecv",
                    self.id, new_view
                );

                self.create_dependency_task_if_new(
                    new_view,
                    event_receiver,
                    event_sender,
                    Arc::clone(&event),
                );
            }
            HotShotEvent::QuorumProposalSend(proposal, _) => {
                let view = proposal.data.view_number;
                if !self.update_latest_proposed_view(view).await {
                    warn!("Failed to update latest proposed view");
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
