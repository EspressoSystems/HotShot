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
                HotShotEvent::QuorumProposalRecv(proposal, _) => {
                    let proposal_payload_comm = proposal.data.block_header.payload_commitment();
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
pub struct QuorumProposalTaskState<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    A: ConsensusApi<TYPES, I> + 'static,
> {
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

    /// Consensus api
    pub api: A,

    /// The node's id
    pub id: u64,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>, A: ConsensusApi<TYPES, I> + 'static>
    QuorumProposalTaskState<TYPES, I, A>
{
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
                        if let HotShotEvent::QuorumProposalRecv(proposal, _) = event {
                            proposal.data.view_number
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
            HotShotEvent::QuorumProposalRecv(_, _) => {
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
                // 1. A QCFormed event and QuorumProposalRecv event
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

    /// Must only update the view and GC if the view actually changes
    #[instrument(skip_all, fields(id = self.id, view = *self.latest_proposed_view), name = "Consensus update view", level = "error")]
    async fn update_view(
        &mut self,
        new_view: TYPES::Time,
        event_stream: &Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> Result<()> {
        if *self.latest_proposed_view > *new_view {
            bail!(
                "Latest view {} is newer than incoming view {}",
                *self.latest_proposed_view,
                *new_view
            );
        }

        debug!(
            "Updating view from {} to {} in consensus task",
            *self.latest_proposed_view, *new_view
        );

        if *self.latest_proposed_view / 100 != *new_view / 100 {
            // TODO (https://github.com/EspressoSystems/HotShot/issues/2296):
            // switch to info! when INFO logs become less cluttered
            error!("Progress: entered view {:>6}", *new_view);
        }

        // cancel the old timeout task
        if let Some(timeout_task) = self.timeout_task.take() {
            cancel_task(timeout_task).await;
        }
        self.latest_proposed_view = new_view;

        // Poll the future leader for lookahead
        let lookahead_view = new_view + LOOK_AHEAD;
        if self.quorum_membership.get_leader(lookahead_view) != self.public_key {
            self.quorum_network
                .inject_consensus_info(ConsensusIntentEvent::PollFutureLeader(
                    *lookahead_view,
                    self.quorum_membership.get_leader(lookahead_view),
                ))
                .await;
        }

        // Start polling for proposals for the new view
        self.quorum_network
            .inject_consensus_info(ConsensusIntentEvent::PollForProposal(
                *self.latest_proposed_view + 1,
            ))
            .await;

        self.quorum_network
            .inject_consensus_info(ConsensusIntentEvent::PollForDAC(
                *self.latest_proposed_view + 1,
            ))
            .await;

        if self
            .quorum_membership
            .get_leader(self.latest_proposed_view + 1)
            == self.public_key
        {
            debug!(
                "Polling for quorum votes for view {}",
                *self.latest_proposed_view
            );
            self.quorum_network
                .inject_consensus_info(ConsensusIntentEvent::PollForVotes(
                    *self.latest_proposed_view,
                ))
                .await;
        }

        broadcast_event(Arc::new(HotShotEvent::ViewChange(new_view)), event_stream).await;

        // Spawn a timeout task if we did actually update view
        let timeout = self.timeout;
        self.timeout_task = Some(async_spawn({
            let stream = event_stream.clone();
            // Nuance: We timeout on the view + 1 here because that means that we have
            // not seen evidence to transition to this new view
            let view_number = self.latest_proposed_view + 1;
            async move {
                async_sleep(Duration::from_millis(timeout)).await;
                broadcast_event(
                    Arc::new(HotShotEvent::Timeout(TYPES::Time::new(*view_number))),
                    &stream,
                )
                .await;
            }
        }));
        let consensus = self.consensus.upgradable_read().await;
        consensus
            .metrics
            .current_view
            .set(usize::try_from(self.latest_proposed_view.get_u64()).unwrap());
        // Do the comparison before the substraction to avoid potential overflow, since
        // `last_decided_view` may be greater than `latest_proposed_view` if the node is catching up.
        if usize::try_from(self.latest_proposed_view.get_u64()).unwrap()
            > usize::try_from(consensus.last_decided_view.get_u64()).unwrap()
        {
            consensus.metrics.number_of_views_since_last_decide.set(
                usize::try_from(self.latest_proposed_view.get_u64()).unwrap()
                    - usize::try_from(consensus.last_decided_view.get_u64()).unwrap(),
            );
        }
        let mut consensus = RwLockUpgradableReadGuard::upgrade(consensus).await;
        consensus.update_view(new_view);
        drop(consensus);
        Ok(())
    }

    /// Get the parent leaf and state for the incoming `quorum_proposal`.
    async fn get_parent_leaf_and_state(
        &self,
        consensus: &RwLockUpgradableReadGuard<'_, Consensus<TYPES>>,
        quorum_proposal: &Proposal<TYPES, QuorumProposal<TYPES>>,
    ) -> Result<Option<(Leaf<TYPES>, Arc<TYPES::ValidatedState>)>> {
        let justify_qc = quorum_proposal.data.justify_qc.clone();
        if justify_qc.is_genesis {
            // Send the `Decide` event for the genesis block if the justify QC is genesis.
            let leaf = Leaf::genesis(&consensus.instance_state);
            let (validated_state, state_delta) =
                TYPES::ValidatedState::genesis(&consensus.instance_state);
            let state = Arc::new(validated_state);
            broadcast_event(
                Event {
                    view_number: TYPES::Time::genesis(),
                    event: EventType::Decide {
                        leaf_chain: Arc::new(vec![LeafInfo::new(
                            leaf.clone(),
                            state.clone(),
                            Some(Arc::new(state_delta)),
                            None,
                        )]),
                        qc: Arc::new(justify_qc.clone()),
                        block_size: None,
                    },
                },
                &self.output_event_stream,
            )
            .await;
            Ok(Some((leaf, state)))
        } else {
            match consensus
                .saved_leaves
                .get(&justify_qc.get_data().leaf_commit)
                .cloned()
            {
                Some(leaf) => {
                    if let (Some(state), _) = consensus.get_state_and_delta(leaf.view_number) {
                        Ok(Some((leaf, state.clone())))
                    } else {
                        bail!("Parent state not found! Consensus internally inconsistent");
                    }
                }
                None => Ok(None),
            }
        }
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

    /// Validate a quorum proposal.
    #[allow(clippy::too_many_lines)]
    async fn validate_quorum_proposal(
        &mut self,
        view: TYPES::Time,
        quorum_proposal: Proposal<TYPES, QuorumProposal<TYPES>>,
        sender: TYPES::SignatureKey,
        event_stream: Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> Result<()> {
        let view_leader_key = self.quorum_membership.get_leader(view);
        if view_leader_key != sender {
            bail!("Leader key does not match key in proposal");
        }

        if quorum_proposal.data.justify_qc.get_view_number() != view - 1 {
            if let Some(received_quorum_proposal_cert) =
                quorum_proposal.data.proposal_certificate.clone()
            {
                if !self.validate_view_change_evidence(view, received_quorum_proposal_cert) {
                    bail!("Failed to validate view change evidence");
                }
            } else {
                bail!(
                    "Quorum proposal for view {} needed a timeout or view sync certificate, but did not have one",
                    *view);
            };
        }

        let justify_qc = quorum_proposal.data.justify_qc.clone();
        if !justify_qc.is_valid_cert(self.quorum_membership.as_ref()) {
            let consensus = self.consensus.write().await;
            consensus.metrics.invalid_qc.update(1);
            bail!("Invalid justify_qc in proposal for view {}", *view);
        }

        // Validate the upgrade certificate, if one is attached.
        // Continue unless the certificate is invalid.
        //
        // Note: we are *not* directly voting on the upgrade certificate here.
        // Once a certificate has been (allegedly) formed, it has already been voted on.
        // The certificate is either valid or invalid, and we are simply validating it.
        //
        // SS: It is possible that we may wish to vote against any quorum proposal
        // if it attaches an upgrade certificate that we cannot support.
        // But I don't think there's much point in this -- if the UpgradeCertificate
        // threshhold (90%) has been reached, voting against the QuorumProposal on that basis
        // will probably be completely symbolic anyway.
        //
        // We should just make sure we don't *sign* an UpgradeCertificate for an upgrade
        // that we do not support.
        if let Some(ref upgrade_cert) = quorum_proposal.data.upgrade_certificate {
            if upgrade_cert.is_valid_cert(self.quorum_membership.as_ref()) {
                self.consensus
                    .write()
                    .await
                    .saved_upgrade_certs
                    .insert(view, upgrade_cert.clone());
            } else {
                bail!("Invalid upgrade_cert in proposal for view {}", *view);
            }
        }

        if let Err(e) = self.update_view(view, &event_stream).await {
            tracing::debug!("{}", e);
        }
        let consensus = self.consensus.upgradable_read().await;
        let parent = self
            .get_parent_leaf_and_state(&consensus, &quorum_proposal)
            .await?;
        let mut consensus = RwLockUpgradableReadGuard::upgrade(consensus).await;
        if justify_qc.get_view_number() > consensus.high_qc.view_number {
            debug!("Updating high QC");
            consensus.high_qc = justify_qc.clone();
        }

        let Some((parent_leaf, parent_state)) = parent else {
            bail!("Tmp");
        };

        let Ok((validated_state, state_delta)) = parent_state
            .validate_and_apply_header(
                &consensus.instance_state,
                &parent_leaf,
                &quorum_proposal.data.block_header.clone(),
            )
            .await
        else {
            bail!("Block header doesn't extend the proposal");
        };
        let state = Arc::new(validated_state);
        let delta = Arc::new(state_delta);
        let parent_commitment = parent_leaf.commit();
        let leaf: Leaf<_> = Leaf {
            view_number: view,
            justify_qc: justify_qc.clone(),
            parent_commitment,
            block_header: quorum_proposal.data.block_header.clone(),
            block_payload: None,
        };
        let leaf_commitment = leaf.commit();

        // Validate the signature. This should also catch if the leaf_commitment does not equal our calculated parent commitment
        if !view_leader_key.validate(&quorum_proposal.signature, leaf_commitment.as_ref()) {
            bail!("Could not verify proposal {:?}", quorum_proposal.signature);
        }
        // Create a positive vote if either liveness or safety check
        // passes.

        // Liveness check.
        let liveness_check = justify_qc.get_view_number() > consensus.locked_view;

        // Safety check.
        // Check if proposal extends from the locked leaf.
        let outcome = consensus.visit_leaf_ancestors(
            justify_qc.get_view_number(),
            Terminator::Inclusive(consensus.locked_view),
            false,
            |leaf, _, _| {
                // if leaf view no == locked view no then we're done, report success by
                // returning true
                leaf.view_number != consensus.locked_view
            },
        );
        let safety_check = outcome.is_ok();
        if let Err(e) = outcome {
            self.api
                .send_event(Event {
                    view_number: view,
                    event: EventType::Error { error: Arc::new(e) },
                })
                .await;
        }

        // Skip if both saftey and liveness checks fail.
        if !safety_check && !liveness_check {
            error!("Failed safety and liveness check \n High QC is {:?}  Proposal QC is {:?}  Locked view is {:?}", consensus.high_qc, quorum_proposal.data.clone(), consensus.locked_view);
        }

        let current_proposal = Some(quorum_proposal.data.clone());

        // We accept the proposal, notify the application layer
        self.api
            .send_event(Event {
                view_number: self.latest_proposed_view,
                event: EventType::QuorumProposal {
                    proposal: quorum_proposal.clone(),
                    sender,
                },
            })
            .await;
        // Notify other tasks
        broadcast_event(
            Arc::new(HotShotEvent::QuorumProposalValidated(
                quorum_proposal.data.clone(),
            )),
            &event_stream,
        )
        .await;

        let mut new_anchor_view = consensus.last_decided_view;
        let mut new_locked_view = consensus.locked_view;
        let mut last_view_number_visited = view;
        let mut new_commit_reached: bool = false;
        let mut new_decide_reached = false;
        let mut new_decide_qc = None;
        let mut leaf_views = Vec::new();
        let mut leafs_decided = Vec::new();
        let mut included_txns = HashSet::new();
        let old_anchor_view = consensus.last_decided_view;
        let parent_view = leaf.justify_qc.get_view_number();
        let mut current_chain_length = 0usize;
        if parent_view + 1 == view {
            current_chain_length += 1;
            if let Err(e) = consensus.visit_leaf_ancestors(
                parent_view,
                Terminator::Exclusive(old_anchor_view),
                true,
                |leaf, state, delta| {
                    if !new_decide_reached {
                        if last_view_number_visited == leaf.view_number + 1 {
                            last_view_number_visited = leaf.view_number;
                            current_chain_length += 1;
                            if current_chain_length == 2 {
                                new_locked_view = leaf.view_number;
                                new_commit_reached = true;
                                // The next leaf in the chain, if there is one, is decided, so this
                                // leaf's justify_qc would become the QC for the decided chain.
                                new_decide_qc = Some(leaf.justify_qc.clone());
                            } else if current_chain_length == 3 {
                                new_anchor_view = leaf.view_number;
                                new_decide_reached = true;
                            }
                        } else {
                            // nothing more to do here... we don't have a new chain extension
                            return false;
                        }
                    }
                    // starting from the first iteration with a three chain, e.g. right after the else if case nested in the if case above
                    if new_decide_reached {
                        let mut leaf = leaf.clone();
                        if leaf.view_number == new_anchor_view {
                            consensus
                                .metrics
                                .last_synced_block_height
                                .set(usize::try_from(leaf.get_height()).unwrap_or(0));
                        }
                        if let Some(upgrade_cert) =
                            consensus.saved_upgrade_certs.get(&leaf.get_view_number())
                        {
                            info!(
                                "Updating consensus state with decided upgrade certificate: {:?}",
                                upgrade_cert
                            );
                            // consesnsu.decided_upgrade_cert = Some(upgrade_cert.clone());
                        }
                        // If the block payload is available for this leaf, include it in
                        // the leaf chain that we send to the client.
                        if let Some(encoded_txns) =
                            consensus.saved_payloads.get(&leaf.get_view_number())
                        {
                            let payload = BlockPayload::from_bytes(
                                encoded_txns.clone().into_iter(),
                                leaf.get_block_header().metadata(),
                            );

                            leaf.fill_block_payload_unchecked(payload);
                        }

                        let vid = VidDisperseShare::to_vid_disperse(
                            consensus
                                .vid_shares
                                .get(&leaf.get_view_number())
                                .unwrap_or(&HashMap::new())
                                .iter()
                                .map(|(_key, proposal)| &proposal.data),
                        );

                        leaf_views.push(LeafInfo::new(
                            leaf.clone(),
                            state.clone(),
                            delta.clone(),
                            vid,
                        ));
                        leafs_decided.push(leaf.clone());
                        if let Some(ref payload) = leaf.block_payload {
                            for txn in
                                payload.transaction_commitments(leaf.get_block_header().metadata())
                            {
                                included_txns.insert(txn);
                            }
                        }
                    }
                    true
                },
            ) {
                error!("view publish error {e}");
                broadcast_event(
                    Event {
                        view_number: view,
                        event: EventType::Error { error: e.into() },
                    },
                    &self.output_event_stream,
                )
                .await;
            }
        }

        let included_txns_set: HashSet<_> = if new_decide_reached {
            included_txns
        } else {
            HashSet::new()
        };

        consensus.validated_state_map.insert(
            view,
            View {
                view_inner: ViewInner::Leaf {
                    leaf: leaf.commit(),
                    state: state.clone(),
                    delta: Some(delta.clone()),
                },
            },
        );
        consensus.saved_leaves.insert(leaf.commit(), leaf.clone());
        if new_commit_reached {
            consensus.locked_view = new_locked_view;
        }
        #[allow(clippy::cast_precision_loss)]
        if new_decide_reached {
            broadcast_event(
                Arc::new(HotShotEvent::LeafDecided(leafs_decided)),
                &event_stream,
            )
            .await;
            let decide_sent = broadcast_event(
                Event {
                    view_number: consensus.last_decided_view,
                    event: EventType::Decide {
                        leaf_chain: Arc::new(leaf_views),
                        qc: Arc::new(new_decide_qc.unwrap()),
                        block_size: Some(included_txns_set.len().try_into().unwrap()),
                    },
                },
                &self.output_event_stream,
            );
            let old_anchor_view = consensus.last_decided_view;
            consensus.collect_garbage(old_anchor_view, new_anchor_view);
            consensus.last_decided_view = new_anchor_view;
            consensus
                .metrics
                .last_decided_time
                .set(Utc::now().timestamp().try_into().unwrap());
            consensus.metrics.invalid_qc.set(0);
            consensus
                .metrics
                .last_decided_view
                .set(usize::try_from(consensus.last_decided_view.get_u64()).unwrap());
            let cur_number_of_views_per_decide_event =
                *self.latest_proposed_view - consensus.last_decided_view.get_u64();
            consensus
                .metrics
                .number_of_views_per_decide_event
                .add_point(cur_number_of_views_per_decide_event as f64);

            debug!("Sending Decide for view {:?}", consensus.last_decided_view);
            debug!("Decided txns len {:?}", included_txns_set.len());
            decide_sent.await;
            debug!("decide send succeeded");
        }

        let new_view = current_proposal.clone().unwrap().view_number + 1;
        // In future we can use the mempool model where we fetch the proposal if we don't have it, instead of having to wait for it here
        // This is for the case where we form a QC but have not yet seen the previous proposal ourselves
        let should_propose = self.quorum_membership.get_leader(new_view) == self.public_key
            && consensus.high_qc.view_number == current_proposal.clone().unwrap().view_number;
        // todo get rid of this clone
        let qc = consensus.high_qc.clone();

        drop(consensus);
        if !should_propose {
            bail!("Cannot propose");
        }

        Ok(())
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
            HotShotEvent::QuorumProposalRecv(proposal, sender) => {
                let view = proposal.data.get_view_number() + 1;
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

                if let Err(e) = self
                    .validate_quorum_proposal(
                        view,
                        proposal.clone(),
                        sender.clone(),
                        event_sender.clone(),
                    )
                    .await
                {
                    tracing::warn!("Failed to validate quorum proposal; error = {:?}", e);
                    return;
                }
                broadcast_event(
                    Arc::new(HotShotEvent::QuorumProposalValidated(proposal.data.clone())),
                    &event_sender.clone(),
                )
                .await;

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

impl<TYPES: NodeType, I: NodeImplementation<TYPES>, A: ConsensusApi<TYPES, I> + 'static> TaskState
    for QuorumProposalTaskState<TYPES, I, A>
{
    type Event = Arc<HotShotEvent<TYPES>>;
    type Output = ();
    fn filter(&self, event: &Arc<HotShotEvent<TYPES>>) -> bool {
        !matches!(
            event.as_ref(),
            HotShotEvent::QuorumProposalRecv(_, _)
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
