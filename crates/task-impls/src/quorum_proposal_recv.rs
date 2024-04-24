#![cfg(feature = "dependency-tasks")]

use std::{collections::BTreeMap, sync::Arc};

use crate::{
    consensus::{
        proposal_helpers::{handle_quorum_proposal_recv, validate_proposal_safety_and_liveness},
        view_change::update_view,
    },
    events::HotShotEvent,
    helpers::{broadcast_event, cancel_task, AnyhowTracing},
};
use async_broadcast::Sender;
use async_compatibility_layer::art::async_spawn;
use async_lock::{RwLock, RwLockUpgradableReadGuard};
#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
use committable::Committable;
use futures::future::join_all;
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
    vote::{Certificate, HasViewNumber, VoteDependencyData},
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

#[cfg(feature = "dependency-tasks")]
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
                match handle_quorum_proposal_recv(proposal, sender, event_stream.clone(), self)
                    .await
                {
                    Ok(Some(current_proposal)) => {
                        self.current_proposal = Some(current_proposal);

                        // Build the parent leaf since we didn't find it during the proposal check.
                        let consensus = self.consensus.read().await;

                        broadcast_event(
                            Arc::new(HotShotEvent::VoteNow(
                                proposal.data.get_view_number() + 1,
                                VoteDependencyData {
                                    quorum_proposal: current_proposal,
                                },
                            )),
                            &event_stream,
                        )
                        .await;
                        // if self.vote_if_able(&event_stream).await {
                        //     self.current_proposal = None;
                        // }
                    }
                    Ok(None) => {}
                    Err(e) => warn!(?e, "Failed to propose"),
                }
            }
            _ => {}
        }
    }
}

#[cfg(feature = "dependency-tasks")]
impl<TYPES: NodeType, I: NodeImplementation<TYPES>> TaskState
    for QuorumProposalRecvTaskState<TYPES, I>
{
    type Event = Arc<HotShotEvent<TYPES>>;
    type Output = ();
    fn filter(&self, event: &Arc<HotShotEvent<TYPES>>) -> bool {
        !matches!(event.as_ref(), HotShotEvent::QuorumProposalRecv(..))
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
