// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::sync::Arc;

use async_broadcast::{Receiver, Sender};
use async_lock::RwLock;
use async_trait::async_trait;
use either::Either;
use hotshot_task::task::TaskState;
use hotshot_types::{
    consensus::OuterConsensus,
    event::Event,
    message::UpgradeLock,
    simple_certificate::{NextEpochQuorumCertificate2, QuorumCertificate2, TimeoutCertificate2},
    simple_vote::{NextEpochQuorumVote2, QuorumVote2, TimeoutVote2},
    traits::{
        node_implementation::{ConsensusTime, NodeImplementation, NodeType, Versions},
        signature_key::SignatureKey,
    },
    utils::epoch_from_block_number,
    vote::HasViewNumber,
};
use tokio::task::JoinHandle;
use tracing::instrument;
use utils::anytrace::*;

use self::handlers::{
    handle_quorum_vote_recv, handle_timeout, handle_timeout_vote_recv, handle_view_change,
};
use crate::{events::HotShotEvent, helpers::broadcast_event, vote_collection::VoteCollectorsMap};

/// Event handlers for use in the `handle` method.
mod handlers;

/// Task state for the Consensus task.
pub struct ConsensusTaskState<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions> {
    /// Our public key
    pub public_key: TYPES::SignatureKey,

    /// Our Private Key
    pub private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,

    /// Immutable instance state
    pub instance_state: Arc<TYPES::InstanceState>,

    /// The underlying network
    pub network: Arc<I::Network>,

    /// Membership for Quorum Certs/votes
    pub membership: Arc<RwLock<TYPES::Membership>>,

    /// A map of `QuorumVote` collector tasks.
    pub vote_collectors: VoteCollectorsMap<TYPES, QuorumVote2<TYPES>, QuorumCertificate2<TYPES>, V>,

    /// A map of `QuorumVote` collector tasks. They collect votes from the nodes in the next epoch.
    pub next_epoch_vote_collectors: VoteCollectorsMap<
        TYPES,
        NextEpochQuorumVote2<TYPES>,
        NextEpochQuorumCertificate2<TYPES>,
        V,
    >,

    /// A map of `TimeoutVote` collector tasks.
    pub timeout_vote_collectors:
        VoteCollectorsMap<TYPES, TimeoutVote2<TYPES>, TimeoutCertificate2<TYPES>, V>,

    /// The view number that this node is currently executing in.
    pub cur_view: TYPES::View,

    /// Timestamp this view starts at.
    pub cur_view_time: i64,

    /// The epoch number that this node is currently executing in.
    pub cur_epoch: TYPES::Epoch,

    /// Output events to application
    pub output_event_stream: async_broadcast::Sender<Event<TYPES>>,

    /// Timeout task handle
    pub timeout_task: JoinHandle<()>,

    /// View timeout from config.
    pub timeout: u64,

    /// A reference to the metrics trait.
    pub consensus: OuterConsensus<TYPES>,

    /// The node's id
    pub id: u64,

    /// Lock for a decided upgrade
    pub upgrade_lock: UpgradeLock<TYPES, V>,

    /// Number of blocks in an epoch, zero means there are no epochs
    pub epoch_height: u64,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions> ConsensusTaskState<TYPES, I, V> {
    /// Handles a consensus event received on the event stream
    #[instrument(skip_all, fields(id = self.id, cur_view = *self.cur_view, cur_epoch = *self.cur_epoch), name = "Consensus replica task", level = "error", target = "ConsensusTaskState")]
    pub async fn handle(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        sender: Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> Result<()> {
        match event.as_ref() {
            HotShotEvent::QuorumVoteRecv(ref vote) => {
                if let Err(e) =
                    handle_quorum_vote_recv(vote, Arc::clone(&event), &sender, self).await
                {
                    tracing::debug!("Failed to handle QuorumVoteRecv event; error = {e}");
                }
            }
            HotShotEvent::TimeoutVoteRecv(ref vote) => {
                if let Err(e) =
                    handle_timeout_vote_recv(vote, Arc::clone(&event), &sender, self).await
                {
                    tracing::debug!("Failed to handle TimeoutVoteRecv event; error = {e}");
                }
            }
            HotShotEvent::ViewChange(new_view_number, epoch_number) => {
                if let Err(e) =
                    handle_view_change(*new_view_number, *epoch_number, &sender, self).await
                {
                    tracing::trace!("Failed to handle ViewChange event; error = {e}");
                }
            }
            HotShotEvent::Timeout(view_number, epoch) => {
                if let Err(e) = handle_timeout(*view_number, *epoch, &sender, self).await {
                    tracing::debug!("Failed to handle Timeout event; error = {e}");
                }
            }
            HotShotEvent::Qc2Formed(Either::Left(quorum_cert)) => {
                if !self
                    .consensus
                    .read()
                    .await
                    .is_leaf_extended(quorum_cert.data.leaf_commit)
                {
                    tracing::debug!("We formed QC but not eQC. Do nothing");
                    return Ok(());
                }
                let cert_view = quorum_cert.view_number();
                let cert_block_number = self
                    .consensus
                    .read()
                    .await
                    .saved_leaves()
                    .get(&quorum_cert.data.leaf_commit)
                    .context(error!(
                        "Could not find the leaf for the eQC. It shouldn't happen."
                    ))?
                    .height();
                let cert_epoch = TYPES::Epoch::new(epoch_from_block_number(
                    cert_block_number,
                    self.epoch_height,
                ));
                // Transition to the new epoch by sending ViewChange
                tracing::info!("Entering new epoch: {:?}", cert_epoch + 1);
                broadcast_event(
                    Arc::new(HotShotEvent::ViewChange(cert_view + 1, cert_epoch + 1)),
                    &sender,
                )
                .await;
            }
            _ => {}
        }

        Ok(())
    }
}

#[async_trait]
impl<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions> TaskState
    for ConsensusTaskState<TYPES, I, V>
{
    type Event = HotShotEvent<TYPES>;

    async fn handle_event(
        &mut self,
        event: Arc<Self::Event>,
        sender: &Sender<Arc<Self::Event>>,
        _receiver: &Receiver<Arc<Self::Event>>,
    ) -> Result<()> {
        self.handle(event, sender.clone()).await
    }

    /// Joins all subtasks.
    fn cancel_subtasks(&mut self) {
        // Cancel the old timeout task
        std::mem::replace(&mut self.timeout_task, tokio::spawn(async {})).abort();
    }
}
