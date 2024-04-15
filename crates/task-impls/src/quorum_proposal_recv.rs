use std::{collections::BTreeMap, sync::Arc};

use crate::{
    consensus::{update_view, validate_proposal},
    events::HotShotEvent,
    helpers::{broadcast_event, cancel_task, AnyhowTracing},
};
use async_broadcast::Sender;
use async_compatibility_layer::art::{async_sleep, async_spawn};
use async_lock::{RwLock, RwLockUpgradableReadGuard};
#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
use committable::Committable;
use futures::future::{join_all, FutureExt};
use hotshot_task::task::{Task, TaskState};
use hotshot_types::{
    consensus::Consensus,
    data::{Leaf, QuorumProposal, ViewChangeEvidence},
    event::{Event, EventType, LeafInfo},
    simple_certificate::UpgradeCertificate,
    traits::{
        election::Membership,
        network::{ConnectedNetwork, ConsensusIntentEvent},
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
        signature_key::SignatureKey,
        storage::Storage,
        ValidatedState,
    },
    utils::{View, ViewInner},
    vote::{Certificate, HasViewNumber},
};
#[cfg(async_executor_impl = "tokio")]
use tokio::task::JoinHandle;
use tracing::{debug, error, instrument, warn};

/// The state for the quorum proposal task. Contains all of the information for
/// handling [`HotShotEvent::QuorumProposalRecv`] events.
pub struct QuorumProposalRecvTaskState<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Our public key
    pub public_key: TYPES::SignatureKey,

    /// Our Private Key
    pub private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,

    /// Reference to consensus. The replica will require a write lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES>>>,

    /// View number this view is executing in.
    pub cur_view: TYPES::Time,

    /// Network for all nodes
    pub quorum_network: Arc<I::QuorumNetwork>,

    /// Membership for Quorum Certs/votes
    pub quorum_membership: Arc<TYPES::Membership>,

    /// Membership for Timeout votes/certs
    pub timeout_membership: Arc<TYPES::Membership>,

    /// timeout task handle
    pub timeout_task: Option<JoinHandle<()>>,

    /// View timeout from config.
    pub timeout: u64,

    /// Output events to application
    pub output_event_stream: async_broadcast::Sender<Event<TYPES>>,

    /// This node's storage ref
    pub storage: Arc<RwLock<I::Storage>>,

    /// The most recent proposal we have, will correspond to the current view if Some()
    /// Will be none if the view advanced through timeout/view_sync
    pub current_proposal: Option<QuorumProposal<TYPES>>,

    /// Spawned tasks related to a specific view, so we can cancel them when
    /// they are stale
    pub spawned_tasks: BTreeMap<TYPES::Time, Vec<JoinHandle<()>>>,

    /// The node's id
    pub id: u64,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> QuorumProposalRecvTaskState<TYPES, I> {
    /// Cancel all tasks that have been spawned before the provided view.
    async fn cancel_tasks(&mut self, view: TYPES::Time) {
        let keep = self.spawned_tasks.split_off(&view);
        let mut cancel = Vec::new();
        while let Some((_, tasks)) = self.spawned_tasks.pop_first() {
            let mut to_cancel = tasks.into_iter().map(cancel_task).collect();
            cancel.append(&mut to_cancel);
        }
        self.spawned_tasks = keep;
        join_all(cancel).await;
    }

    /// Handles all consensus events relating to propose and vote-enabling events.
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "Consensus replica task", level = "error")]
    pub async fn handle(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        event_stream: Sender<Arc<HotShotEvent<TYPES>>>,
    ) {
        match event.as_ref() {
            HotShotEvent::QuorumProposalRecv(proposal, sender) => {
                let sender = sender.clone();
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

                let view = proposal.data.get_view_number();
                if view < self.cur_view {
                    debug!("Proposal is from an older view {:?}", proposal.data.clone());
                    return;
                }

                let view_leader_key = self.quorum_membership.get_leader(view);
                if view_leader_key != sender {
                    warn!("Leader key does not match key in proposal");
                    return;
                }

                // Verify a timeout certificate OR a view sync certificate exists and is valid.
                if proposal.data.justify_qc.get_view_number() != view - 1 {
                    if let Some(received_proposal_cert) = proposal.data.proposal_certificate.clone()
                    {
                        match received_proposal_cert {
                            ViewChangeEvidence::Timeout(timeout_cert) => {
                                if timeout_cert.get_data().view != view - 1 {
                                    warn!("Timeout certificate for view {} was not for the immediately preceding view", *view);
                                    return;
                                }

                                if !timeout_cert.is_valid_cert(self.timeout_membership.as_ref()) {
                                    warn!("Timeout certificate for view {} was invalid", *view);
                                    return;
                                }
                            }
                            ViewChangeEvidence::ViewSync(view_sync_cert) => {
                                if view_sync_cert.view_number != view {
                                    debug!(
                                        "Cert view number {:?} does not match proposal view number {:?}",
                                        view_sync_cert.view_number, view
                                    );
                                    return;
                                }

                                // View sync certs must also be valid.
                                if !view_sync_cert.is_valid_cert(self.quorum_membership.as_ref()) {
                                    debug!("Invalid ViewSyncFinalize cert provided");
                                    return;
                                }
                            }
                        }
                    } else {
                        warn!(
                            "Quorum proposal for view {} needed a timeout or view sync certificate, but did not have one",
                            *view);
                        return;
                    };
                }

                let justify_qc = proposal.data.justify_qc.clone();

                if !justify_qc.is_valid_cert(self.quorum_membership.as_ref()) {
                    error!("Invalid justify_qc in proposal for view {}", *view);
                    let consensus = self.consensus.write().await;
                    consensus.metrics.invalid_qc.update(1);
                    return;
                }

                // Validate the upgrade certificate -- this is just a signature validation.
                // Note that we don't do anything with the certificate directly if this passes; it eventually gets stored as part of the leaf if nothing goes wrong.
                if let Err(e) = UpgradeCertificate::validate(
                    &proposal.data.upgrade_certificate,
                    &self.quorum_membership,
                ) {
                    warn!("{:?}", e);

                    return;
                }

                // NOTE: We could update our view with a valid TC but invalid QC, but that is not what we do here
                if let Err(e) = update_view::<TYPES, I>(
                    self.public_key.clone(),
                    view,
                    &event_stream,
                    self.quorum_membership.clone(),
                    self.quorum_network.clone(),
                    self.timeout,
                    self.consensus.clone(),
                    &mut self.cur_view,
                    &mut self.timeout_task,
                )
                .await
                {
                    // This isn't typically a serious end-it-all failure.
                    warn!("Failed to update view; error = {:?}", e);
                }

                let consensus = self.consensus.upgradable_read().await;

                // Get the parent leaf and state.
                let parent = if justify_qc.is_genesis {
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
                    Some((leaf, state))
                } else {
                    match consensus
                        .saved_leaves
                        .get(&justify_qc.get_data().leaf_commit)
                        .cloned()
                    {
                        Some(leaf) => {
                            if let (Some(state), _) =
                                consensus.get_state_and_delta(leaf.get_view_number())
                            {
                                Some((leaf, state.clone()))
                            } else {
                                error!("Parent state not found! Consensus internally inconsistent");
                                return;
                            }
                        }
                        None => None,
                    }
                };

                if justify_qc.get_view_number() > consensus.high_qc.view_number {
                    if let Err(e) = self
                        .storage
                        .write()
                        .await
                        .update_high_qc(justify_qc.clone())
                        .await
                    {
                        warn!("Failed to store High QC not voting. Error: {:?}", e);
                        return;
                    }
                }

                let mut consensus = RwLockUpgradableReadGuard::upgrade(consensus).await;

                if justify_qc.get_view_number() > consensus.high_qc.view_number {
                    debug!("Updating high QC");
                    consensus.high_qc = justify_qc.clone();
                }

                // Justify qc's leaf commitment is not the same as the parent's leaf commitment, but it should be (in this case)
                let Some((parent_leaf, parent_state)) = parent else {
                    warn!(
                        "Proposal's parent missing from storage with commitment: {:?}",
                        justify_qc.get_data().leaf_commit
                    );
                    let leaf = Leaf::from_quorum_proposal(&proposal.data);

                    let state = Arc::new(
                        <TYPES::ValidatedState as ValidatedState<TYPES>>::from_header(
                            &proposal.data.block_header,
                        ),
                    );

                    consensus.validated_state_map.insert(
                        view,
                        View {
                            view_inner: ViewInner::Leaf {
                                leaf: leaf.commit(),
                                state,
                                delta: None,
                            },
                        },
                    );
                    consensus.saved_leaves.insert(leaf.commit(), leaf.clone());

                    if let Err(e) = self
                        .storage
                        .write()
                        .await
                        .update_undecided_state(
                            consensus.saved_leaves.clone(),
                            consensus.validated_state_map.clone(),
                        )
                        .await
                    {
                        warn!("Couldn't store undecided state.  Error: {:?}", e);
                    }

                    // If we are missing the parent from storage, the safety check will fail.  But we can
                    // still vote if the liveness check succeeds.
                    let liveness_check = justify_qc.get_view_number() > consensus.locked_view;

                    let high_qc = consensus.high_qc.clone();
                    let locked_view = consensus.locked_view;

                    drop(consensus);

                    if liveness_check {
                        self.current_proposal = Some(proposal.data.clone());
                        let new_view = proposal.data.view_number + 1;

                        // This is for the case where we form a QC but have not yet seen the previous proposal ourselves
                        let should_propose = self.quorum_membership.get_leader(new_view)
                            == self.public_key
                            && high_qc.view_number
                                == self.current_proposal.clone().unwrap().view_number;
                        let qc = high_qc.clone();
                        if should_propose {
                            debug!(
                                "Attempting to publish proposal after voting; now in view: {}",
                                *new_view
                            );
                            self.publish_proposal_if_able(qc.view_number + 1, &event_stream)
                                .await;
                        }
                        if self.vote_if_able(&event_stream).await {
                            self.current_proposal = None;
                        }
                    }
                    warn!("Failed liveneess check; cannot find parent either\n High QC is {:?}  Proposal QC is {:?}  Locked view is {:?}", high_qc, proposal.data.clone(), locked_view);

                    return;
                };

                self.spawned_tasks
                    .entry(proposal.data.get_view_number())
                    .or_default()
                    .push(async_spawn(
                        validate_proposal(
                            proposal.clone(),
                            parent_leaf,
                            self.consensus.clone(),
                            self.decided_upgrade_cert.clone(),
                            self.quorum_membership.clone(),
                            parent_state.clone(),
                            view_leader_key,
                            event_stream.clone(),
                            sender,
                            self.output_event_stream.clone(),
                            self.storage.clone(),
                        )
                        .map(AnyhowTracing::err_as_debug),
                    ));
            }
            _ => {}
        }
    }
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> TaskState
    for QuorumProposalRecvTaskState<TYPES, I>
{
    type Event = Arc<HotShotEvent<TYPES>>;
    type Output = ();
    fn filter(&self, event: &Arc<HotShotEvent<TYPES>>) -> bool {
        !matches!(
            event.as_ref(),
            HotShotEvent::QuorumProposalRecv(..) | HotShotEvent::QuorumProposalValidated(..),
        )
    }

    async fn handle_event(event: Self::Event, task: &mut Task<Self>) -> Option<()>
    where
        Self: Sized,
    {
        let sender = task.clone_sender();
        task.state_mut().handle(event, sender).await;
        None
    }

    fn should_shutdown(event: &Self::Event) -> bool {
        matches!(event.as_ref(), HotShotEvent::Shutdown)
    }
}
