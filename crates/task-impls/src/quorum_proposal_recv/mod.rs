// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

#![allow(unused_imports)]

use std::{collections::BTreeMap, sync::Arc};

use anyhow::{bail, Result};
use async_broadcast::{broadcast, Receiver, Sender};
use async_compatibility_layer::art::async_spawn;
use async_lock::RwLock;
#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
use async_trait::async_trait;
use futures::future::join_all;
use hotshot_task::task::{Task, TaskState};
use hotshot_types::{
    consensus::{Consensus, OuterConsensus},
    data::{Leaf, ViewChangeEvidence},
    event::Event,
    message::UpgradeLock,
    simple_certificate::UpgradeCertificate,
    traits::{
        node_implementation::{NodeImplementation, NodeType, Versions},
        signature_key::SignatureKey,
    },
    vote::HasViewNumber,
};
#[cfg(async_executor_impl = "tokio")]
use tokio::task::JoinHandle;
use tracing::{debug, error, info, instrument, warn};
use vbs::version::Version;

use self::handlers::handle_quorum_proposal_recv;
use crate::{
    events::{HotShotEvent, ProposalMissing},
    helpers::{broadcast_event, cancel_task, parent_leaf_and_state},
};
use hotshot_types::traits::node_implementation::ConsensusTime;
/// Event handlers for this task.
mod handlers;

/// The state for the quorum proposal task. Contains all of the information for
/// handling [`HotShotEvent::QuorumProposalRecv`] events.
pub struct QuorumProposalRecvTaskState<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions> {
    /// Our public key
    pub public_key: TYPES::SignatureKey,

    /// Our Private Key
    pub private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,

    /// Reference to consensus. The replica will require a write lock on this.
    pub consensus: OuterConsensus<TYPES>,

    /// View number this view is executing in.
    pub cur_view: TYPES::View,

    /// Epoch number this node is executing in.
    pub cur_epoch: TYPES::Epoch,

    /// Membership for Quorum Certs/votes
    pub quorum_membership: Arc<TYPES::Membership>,

    /// View timeout from config.
    pub timeout: u64,

    /// Output events to application
    pub output_event_stream: async_broadcast::Sender<Event<TYPES>>,

    /// This node's storage ref
    pub storage: Arc<RwLock<I::Storage>>,

    /// Spawned tasks related to a specific view, so we can cancel them when
    /// they are stale
    pub spawned_tasks: BTreeMap<TYPES::View, Vec<JoinHandle<()>>>,

    /// Immutable instance state
    pub instance_state: Arc<TYPES::InstanceState>,

    /// The node's id
    pub id: u64,

    /// Lock for a decided upgrade
    pub upgrade_lock: UpgradeLock<TYPES, V>,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions>
    QuorumProposalRecvTaskState<TYPES, I, V>
{
    /// Cancel all tasks the consensus tasks has spawned before the given view
    pub fn cancel_tasks(&mut self, view: TYPES::View) {
        let keep = self.spawned_tasks.split_off(&view);
        let mut cancel = Vec::new();
        while let Some((_, tasks)) = self.spawned_tasks.pop_first() {
            let mut to_cancel = tasks.into_iter().map(cancel_task).collect();
            cancel.append(&mut to_cancel);
        }
        self.spawned_tasks = keep;
        async_spawn(async move { join_all(cancel).await });
    }

    /// Handles all consensus events relating to propose and vote-enabling events.
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "Consensus replica task", level = "error")]
    #[allow(unused_variables)]
    pub async fn handle(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        event_sender: Sender<Arc<HotShotEvent<TYPES>>>,
        event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
    ) {
        match event.as_ref() {
            HotShotEvent::QuorumProposalRecv(proposal, sender) => {
                if self.consensus.read().await.cur_view() > proposal.data.view_number()
                    || self.cur_view > proposal.data.view_number()
                {
                    return;
                }
                match handle_quorum_proposal_recv(
                    proposal,
                    sender,
                    &event_sender,
                    &event_receiver,
                    self,
                )
                .await
                {
                    Ok(()) => {}
                    Err(e) => debug!(?e, "Failed to validate the proposal"),
                }
            }
            HotShotEvent::ViewChange(view) => {
                if self.cur_view >= *view {
                    return;
                }
                self.cur_view = *view;
                // cancel task for any view 2 views prior or more.  The view here is the oldest
                // view we want to KEEP tasks for.  We keep the view prior to this because
                // we might still be processing the proposal from view V which caused us
                // to enter view V + 1.
                let oldest_view_to_keep = TYPES::View::new(view.saturating_sub(1));
                self.cancel_tasks(oldest_view_to_keep);
            }
            _ => {}
        }
    }
}

#[async_trait]
impl<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions> TaskState
    for QuorumProposalRecvTaskState<TYPES, I, V>
{
    type Event = HotShotEvent<TYPES>;

    async fn handle_event(
        &mut self,
        event: Arc<Self::Event>,
        sender: &Sender<Arc<Self::Event>>,
        receiver: &Receiver<Arc<Self::Event>>,
    ) -> Result<()> {
        self.handle(event, sender.clone(), receiver.clone()).await;

        Ok(())
    }

    async fn cancel_subtasks(&mut self) {
        while !self.spawned_tasks.is_empty() {
            let Some((_, handles)) = self.spawned_tasks.pop_first() else {
                break;
            };

            for handle in handles {
                #[cfg(async_executor_impl = "async-std")]
                handle.cancel().await;
                #[cfg(async_executor_impl = "tokio")]
                handle.abort();
            }
        }
    }
}
