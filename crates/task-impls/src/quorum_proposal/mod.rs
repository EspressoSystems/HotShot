// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::{collections::BTreeMap, sync::Arc, time::Instant};

use async_broadcast::{Receiver, Sender};
use async_lock::RwLock;
use async_trait::async_trait;
use either::Either;
use hotshot_task::{
    dependency::{AndDependency, EventDependency, OrDependency},
    dependency_task::DependencyTask,
    task::TaskState,
};
use hotshot_types::{
    consensus::OuterConsensus,
    message::UpgradeLock,
    simple_certificate::{QuorumCertificate2, UpgradeCertificate},
    traits::{
        election::Membership,
        node_implementation::{ConsensusTime, NodeImplementation, NodeType, Versions},
        signature_key::SignatureKey,
        storage::Storage,
    },
    utils::EpochTransitionIndicator,
    vote::{Certificate, HasViewNumber},
};
use tokio::task::JoinHandle;
use tracing::instrument;
use utils::anytrace::*;

use self::handlers::{ProposalDependency, ProposalDependencyHandle};
use crate::events::HotShotEvent;

mod handlers;

/// The state for the quorum proposal task.
pub struct QuorumProposalTaskState<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions> {
    /// Latest view number that has been proposed for.
    pub latest_proposed_view: TYPES::View,

    /// Current epoch
    pub cur_epoch: TYPES::Epoch,

    /// Table for the in-progress proposal dependency tasks.
    pub proposal_dependencies: BTreeMap<TYPES::View, JoinHandle<()>>,

    /// Immutable instance state
    pub instance_state: Arc<TYPES::InstanceState>,

    /// Membership for Quorum Certs/votes
    pub membership: Arc<RwLock<TYPES::Membership>>,

    /// Our public key
    pub public_key: TYPES::SignatureKey,

    /// Our Private Key
    pub private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,

    /// View timeout from config.
    pub timeout: u64,

    /// This node's storage ref
    pub storage: Arc<RwLock<I::Storage>>,

    /// Shared consensus task state
    pub consensus: OuterConsensus<TYPES>,

    /// The node's id
    pub id: u64,

    /// The most recent upgrade certificate this node formed.
    /// Note: this is ONLY for certificates that have been formed internally,
    /// so that we can propose with them.
    ///
    /// Certificates received from other nodes will get reattached regardless of this fields,
    /// since they will be present in the leaf we propose off of.
    pub formed_upgrade_certificate: Option<UpgradeCertificate<TYPES>>,

    /// Lock for a decided upgrade
    pub upgrade_lock: UpgradeLock<TYPES, V>,

    /// Number of blocks in an epoch, zero means there are no epochs
    pub epoch_height: u64,

    /// The highest_qc we've seen at the start of this task
    pub highest_qc: QuorumCertificate2<TYPES>,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions>
    QuorumProposalTaskState<TYPES, I, V>
{
    /// Create an event dependency
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view), name = "Create event dependency", level = "info")]
    fn create_event_dependency(
        &self,
        dependency_type: ProposalDependency,
        view_number: TYPES::View,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
    ) -> EventDependency<Arc<HotShotEvent<TYPES>>> {
        let id = self.id;
        EventDependency::new(
            event_receiver,
            Box::new(move |event| {
                let event = event.as_ref();
                let event_view = match dependency_type {
                    ProposalDependency::Qc => {
                        if let HotShotEvent::Qc2Formed(either::Left(qc)) = event {
                            qc.view_number() + 1
                        } else {
                            return false;
                        }
                    }
                    ProposalDependency::TimeoutCert => {
                        if let HotShotEvent::Qc2Formed(either::Right(timeout)) = event {
                            timeout.view_number() + 1
                        } else {
                            return false;
                        }
                    }
                    ProposalDependency::ViewSyncCert => {
                        if let HotShotEvent::ViewSyncFinalizeCertificateRecv(view_sync_cert) = event
                        {
                            view_sync_cert.view_number()
                        } else {
                            return false;
                        }
                    }
                    ProposalDependency::Proposal => {
                        if let HotShotEvent::QuorumProposalPreliminarilyValidated(proposal) = event
                        {
                            proposal.data.view_number() + 1
                        } else {
                            return false;
                        }
                    }
                    ProposalDependency::PayloadAndMetadata => {
                        if let HotShotEvent::SendPayloadCommitmentAndMetadata(
                            _payload_commitment,
                            _builder_commitment,
                            _metadata,
                            view_number,
                            _fee,
                            _auction_result,
                        ) = event
                        {
                            *view_number
                        } else {
                            return false;
                        }
                    }
                    ProposalDependency::VidShare => {
                        if let HotShotEvent::VidDisperseSend(vid_share, _) = event {
                            vid_share.data.view_number()
                        } else {
                            return false;
                        }
                    }
                };
                let valid = event_view == view_number;
                if valid {
                    tracing::debug!(
                        "Dependency {dependency_type:?} is complete for view {event_view:?}, my id is {id:?}!",
                    );
                }
                valid
            }),
        )
    }

    /// Creates the requisite dependencies for the Quorum Proposal task. It also handles any event forwarding.
    fn create_and_complete_dependencies(
        &self,
        view_number: TYPES::View,
        event_receiver: &Receiver<Arc<HotShotEvent<TYPES>>>,
        event: Arc<HotShotEvent<TYPES>>,
    ) -> AndDependency<Vec<Vec<Arc<HotShotEvent<TYPES>>>>> {
        let mut proposal_dependency = self.create_event_dependency(
            ProposalDependency::Proposal,
            view_number,
            event_receiver.clone(),
        );

        let mut qc_dependency = self.create_event_dependency(
            ProposalDependency::Qc,
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

        let mut vid_share_dependency = self.create_event_dependency(
            ProposalDependency::VidShare,
            view_number,
            event_receiver.clone(),
        );

        match event.as_ref() {
            HotShotEvent::SendPayloadCommitmentAndMetadata(..) => {
                payload_commitment_dependency.mark_as_completed(Arc::clone(&event));
            }
            HotShotEvent::QuorumProposalPreliminarilyValidated(..) => {
                proposal_dependency.mark_as_completed(event);
            }
            HotShotEvent::Qc2Formed(quorum_certificate) => match quorum_certificate {
                Either::Right(_) => {
                    timeout_dependency.mark_as_completed(event);
                }
                Either::Left(_) => {
                    qc_dependency.mark_as_completed(event);
                }
            },
            HotShotEvent::ViewSyncFinalizeCertificateRecv(_) => {
                view_sync_dependency.mark_as_completed(event);
            }
            HotShotEvent::VidDisperseSend(_, _) => {
                vid_share_dependency.mark_as_completed(event);
            }
            _ => {}
        };

        // We have three cases to consider:
        let mut secondary_deps = vec![
            // 1. A timeout cert was received
            AndDependency::from_deps(vec![timeout_dependency]),
            // 2. A view sync cert was received.
            AndDependency::from_deps(vec![view_sync_dependency]),
        ];
        // 3. A `Qc2Formed`` event (and `QuorumProposalRecv` event)
        if *view_number > 1 {
            secondary_deps.push(AndDependency::from_deps(vec![
                qc_dependency,
                proposal_dependency,
            ]));
        } else {
            secondary_deps.push(AndDependency::from_deps(vec![qc_dependency]));
        }

        let primary_deps = vec![payload_commitment_dependency, vid_share_dependency];

        AndDependency::from_deps(vec![OrDependency::from_deps(vec![
            AndDependency::from_deps(vec![
                OrDependency::from_deps(vec![AndDependency::from_deps(primary_deps)]),
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
    async fn create_dependency_task_if_new(
        &mut self,
        view_number: TYPES::View,
        epoch_number: TYPES::Epoch,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
        event_sender: Sender<Arc<HotShotEvent<TYPES>>>,
        event: Arc<HotShotEvent<TYPES>>,
        epoch_transition_indicator: EpochTransitionIndicator,
    ) -> Result<()> {
        let membership_reader = self.membership.read().await;
        let leader_in_current_epoch =
            membership_reader.leader(view_number, epoch_number)? == self.public_key;
        // If we are in the epoch transition and we are the leader in the next epoch,
        // we might want to start collecting dependencies for our next epoch proposal.
        let leader_in_next_epoch = matches!(
            epoch_transition_indicator,
            EpochTransitionIndicator::InTransition
        ) && membership_reader.leader(view_number, epoch_number + 1)?
            == self.public_key;
        drop(membership_reader);

        // Don't even bother making the task if we are not entitled to propose anyway.
        ensure!(
            leader_in_current_epoch || leader_in_next_epoch,
            debug!("We are not the leader of the next view")
        );

        // Don't try to propose twice for the same view.
        ensure!(
            view_number > self.latest_proposed_view,
            "We have already proposed for this view"
        );

        tracing::debug!(
            "Attempting to make dependency task for view {view_number:?} and event {event:?}"
        );

        ensure!(
            !self.proposal_dependencies.contains_key(&view_number),
            "Task already exists"
        );

        let dependency_chain =
            self.create_and_complete_dependencies(view_number, &event_receiver, event);

        let dependency_task = DependencyTask::new(
            dependency_chain,
            ProposalDependencyHandle {
                latest_proposed_view: self.latest_proposed_view,
                view_number,
                sender: event_sender,
                receiver: event_receiver,
                membership: Arc::clone(&self.membership),
                public_key: self.public_key.clone(),
                private_key: self.private_key.clone(),
                instance_state: Arc::clone(&self.instance_state),
                consensus: OuterConsensus::new(Arc::clone(&self.consensus.inner_consensus)),
                timeout: self.timeout,
                formed_upgrade_certificate: self.formed_upgrade_certificate.clone(),
                upgrade_lock: self.upgrade_lock.clone(),
                id: self.id,
                view_start_time: Instant::now(),
                highest_qc: self.highest_qc.clone(),
                epoch_height: self.epoch_height,
            },
        );
        self.proposal_dependencies
            .insert(view_number, dependency_task.run());

        Ok(())
    }

    /// Update the latest proposed view number.
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view), name = "Update latest proposed view", level = "error")]
    async fn update_latest_proposed_view(&mut self, new_view: TYPES::View) -> bool {
        if *self.latest_proposed_view < *new_view {
            tracing::debug!(
                "Updating latest proposed view from {} to {}",
                *self.latest_proposed_view,
                *new_view
            );

            // Cancel the old dependency tasks.
            for view in (*self.latest_proposed_view + 1)..=(*new_view) {
                if let Some(dependency) = self.proposal_dependencies.remove(&TYPES::View::new(view))
                {
                    dependency.abort();
                }
            }

            self.latest_proposed_view = new_view;

            return true;
        }
        false
    }

    /// Handles a consensus event received on the event stream
    #[instrument(skip_all, fields(id = self.id, latest_proposed_view = *self.latest_proposed_view, epoch = *self.cur_epoch), name = "handle method", level = "error", target = "QuorumProposalTaskState")]
    pub async fn handle(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
        event_sender: Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> Result<()> {
        let epoch_number = self.cur_epoch;
        let epoch_transition_indicator = if self.consensus.read().await.is_high_qc_for_last_block()
        {
            EpochTransitionIndicator::InTransition
        } else {
            EpochTransitionIndicator::NotInTransition
        };
        match event.as_ref() {
            HotShotEvent::UpgradeCertificateFormed(cert) => {
                tracing::debug!(
                    "Upgrade certificate received for view {}!",
                    *cert.view_number
                );
                // Update our current upgrade_cert as long as we still have a chance of reaching a decide on it in time.
                if cert.data.decide_by >= self.latest_proposed_view + 3 {
                    tracing::debug!("Updating current formed_upgrade_certificate");

                    self.formed_upgrade_certificate = Some(cert.clone());
                }
            }
            HotShotEvent::Qc2Formed(cert) => match cert.clone() {
                either::Right(timeout_cert) => {
                    let view_number = timeout_cert.view_number + 1;
                    self.create_dependency_task_if_new(
                        view_number,
                        epoch_number,
                        event_receiver,
                        event_sender,
                        Arc::clone(&event),
                        epoch_transition_indicator,
                    )
                    .await?;
                }
                either::Left(qc) => {
                    // Only update if the qc is from a newer view
                    if qc.view_number <= self.consensus.read().await.high_qc().view_number {
                        tracing::trace!(
                            "Received a QC for a view that was not > than our current high QC"
                        );
                    }
                    self.consensus
                        .write()
                        .await
                        .update_high_qc(qc.clone())
                        .wrap()
                        .context(error!(
                            "Failed to update high QC in internal consensus state!"
                        ))?;

                    // Then update the high QC in storage
                    self.storage
                        .write()
                        .await
                        .update_high_qc2(qc.clone())
                        .await
                        .wrap()
                        .context(error!("Failed to update high QC in storage!"))?;
                    let view_number = qc.view_number() + 1;
                    self.create_dependency_task_if_new(
                        view_number,
                        epoch_number,
                        event_receiver,
                        event_sender,
                        Arc::clone(&event),
                        epoch_transition_indicator,
                    )
                    .await?;
                }
            },
            HotShotEvent::SendPayloadCommitmentAndMetadata(
                _payload_commitment,
                _builder_commitment,
                _metadata,
                view_number,
                _fee,
                _auction_result,
            ) => {
                let view_number = *view_number;

                self.create_dependency_task_if_new(
                    view_number,
                    epoch_number,
                    event_receiver,
                    event_sender,
                    Arc::clone(&event),
                    EpochTransitionIndicator::NotInTransition,
                )
                .await?;
            }
            HotShotEvent::ViewSyncFinalizeCertificateRecv(certificate) => {
                let epoch_number = certificate.data.epoch;

                let membership_reader = self.membership.read().await;
                let membership_stake_table = membership_reader.stake_table(epoch_number);
                let membership_success_threshold =
                    membership_reader.success_threshold(epoch_number);
                drop(membership_reader);

                ensure!(
                    certificate
                        .is_valid_cert(
                            membership_stake_table,
                            membership_success_threshold,
                            &self.upgrade_lock
                        )
                        .await,
                    warn!(
                        "View Sync Finalize certificate {:?} was invalid",
                        certificate.data()
                    )
                );

                let view_number = certificate.view_number;

                self.create_dependency_task_if_new(
                    view_number,
                    epoch_number,
                    event_receiver,
                    event_sender,
                    event,
                    EpochTransitionIndicator::NotInTransition,
                )
                .await?;
            }
            HotShotEvent::QuorumProposalPreliminarilyValidated(proposal) => {
                let view_number = proposal.data.view_number();
                // All nodes get the latest proposed view as a proxy of `cur_view` of old.
                if !self.update_latest_proposed_view(view_number).await {
                    tracing::trace!("Failed to update latest proposed view");
                }

                self.create_dependency_task_if_new(
                    view_number + 1,
                    epoch_number,
                    event_receiver,
                    event_sender,
                    Arc::clone(&event),
                    epoch_transition_indicator,
                )
                .await?;
            }
            HotShotEvent::QuorumProposalSend(proposal, _) => {
                let view = proposal.data.view_number();

                ensure!(
                    self.update_latest_proposed_view(view).await,
                    "Failed to update latest proposed view"
                );
            }
            HotShotEvent::VidDisperseSend(vid_share, _) => {
                let view_number = vid_share.data.view_number();
                self.create_dependency_task_if_new(
                    view_number,
                    epoch_number,
                    event_receiver,
                    event_sender,
                    Arc::clone(&event),
                    EpochTransitionIndicator::NotInTransition,
                )
                .await?;
            }
            HotShotEvent::ViewChange(view, epoch) => {
                if epoch > &self.cur_epoch {
                    self.cur_epoch = *epoch;
                }
                let keep_view = TYPES::View::new(view.saturating_sub(1));
                self.cancel_tasks(keep_view);
            }
            HotShotEvent::Timeout(view, ..) => {
                let keep_view = TYPES::View::new(view.saturating_sub(1));
                self.cancel_tasks(keep_view);
            }
            HotShotEvent::HighQcSend(qc, ..) => {
                ensure!(qc.view_number() > self.highest_qc.view_number());
                let cert_epoch_number = qc.data.epoch;

                let membership_reader = self.membership.read().await;
                let membership_stake_table = membership_reader.stake_table(cert_epoch_number);
                let membership_success_threshold =
                    membership_reader.success_threshold(cert_epoch_number);
                drop(membership_reader);

                ensure!(
                    qc.is_valid_cert(
                        membership_stake_table,
                        membership_success_threshold,
                        &self.upgrade_lock
                    )
                    .await,
                    warn!("Quorum certificate {:?} was invalid", qc.data())
                );
                self.highest_qc = qc.clone();
            }
            HotShotEvent::NextEpochQc2Formed(Either::Left(next_epoch_qc)) => {
                // Only update if the qc is from a newer view
                let current_next_epoch_qc =
                    self.consensus.read().await.next_epoch_high_qc().cloned();
                ensure!(current_next_epoch_qc.is_none() ||
                    next_epoch_qc.view_number > current_next_epoch_qc.unwrap().view_number,
                    debug!("Received a next epoch QC for a view that was not > than our current next epoch high QC")
                );
                self.consensus
                    .write()
                    .await
                    .update_next_epoch_high_qc(next_epoch_qc.clone())
                    .wrap()
                    .context(error!(
                        "Failed to update next epoch high QC in internal consensus state!"
                    ))?;

                // Then update the next epoch high QC in storage
                self.storage
                    .write()
                    .await
                    .update_next_epoch_high_qc2(next_epoch_qc.clone())
                    .await
                    .wrap()
                    .context(error!("Failed to update next epoch high QC in storage!"))?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Cancel all tasks the consensus tasks has spawned before the given view
    pub fn cancel_tasks(&mut self, view: TYPES::View) {
        let keep = self.proposal_dependencies.split_off(&view);
        while let Some((_, task)) = self.proposal_dependencies.pop_first() {
            task.abort();
        }
        self.proposal_dependencies = keep;
    }
}

#[async_trait]
impl<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions> TaskState
    for QuorumProposalTaskState<TYPES, I, V>
{
    type Event = HotShotEvent<TYPES>;

    async fn handle_event(
        &mut self,
        event: Arc<Self::Event>,
        sender: &Sender<Arc<Self::Event>>,
        receiver: &Receiver<Arc<Self::Event>>,
    ) -> Result<()> {
        self.handle(event, receiver.clone(), sender.clone()).await
    }

    fn cancel_subtasks(&mut self) {
        while let Some((_, handle)) = self.proposal_dependencies.pop_first() {
            handle.abort();
        }
    }
}
