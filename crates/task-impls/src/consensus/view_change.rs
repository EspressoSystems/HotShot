use std::sync::Arc;

use crate::{
    events::HotShotEvent,
    helpers::{broadcast_event, cancel_task},
};
use anyhow::{ensure, Result};
use async_broadcast::Sender;
use async_compatibility_layer::art::{async_sleep, async_spawn};
use async_lock::{RwLock, RwLockUpgradableReadGuard};
use core::time::Duration;
use hotshot_types::{
    consensus::Consensus,
    constants::LOOK_AHEAD,
    traits::{
        election::Membership,
        network::{ConnectedNetwork, ConsensusIntentEvent},
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
    },
};
use tracing::{debug, error};

#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
#[cfg(async_executor_impl = "tokio")]
use tokio::task::JoinHandle;

/// Update the view if it actually changed. Returns the spawned timeout task join handle.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn update_view<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    public_key: TYPES::SignatureKey,
    new_view: TYPES::Time,
    event_stream: &Sender<Arc<HotShotEvent<TYPES>>>,
    quorum_membership: Arc<TYPES::Membership>,
    quorum_network: Arc<I::QuorumNetwork>,
    timeout: u64,
    consensus: Arc<RwLock<Consensus<TYPES>>>,
    cur_view: &mut TYPES::Time,
    timeout_task: &mut Option<JoinHandle<()>>,
) -> Result<()> {
    ensure!(
        new_view > *cur_view,
        "New view is not greater than our current view"
    );

    debug!("Updating view from {} to {}", **cur_view, *new_view);

    if **cur_view / 100 != *new_view / 100 {
        // TODO (https://github.com/EspressoSystems/HotShot/issues/2296):
        // switch to info! when INFO logs become less cluttered
        error!("Progress: entered view {:>6}", *new_view);
    }

    // cancel the old timeout task
    if let Some(timeout_task) = timeout_task.take() {
        cancel_task(timeout_task).await;
    }

    *cur_view = new_view;

    // Poll the future leader for lookahead
    let lookahead_view = new_view + LOOK_AHEAD;
    if quorum_membership.get_leader(lookahead_view) != public_key {
        quorum_network
            .inject_consensus_info(ConsensusIntentEvent::PollFutureLeader(
                *lookahead_view,
                quorum_membership.get_leader(lookahead_view),
            ))
            .await;
    }

    // The next view is just the current view + 1
    let next_view = *cur_view + 1;

    // Start polling for proposals for the new view
    quorum_network
        .inject_consensus_info(ConsensusIntentEvent::PollForProposal(*next_view))
        .await;

    quorum_network
        .inject_consensus_info(ConsensusIntentEvent::PollForDAC(*next_view))
        .await;

    if quorum_membership.get_leader(next_view) == public_key {
        debug!("Polling for quorum votes for view {}", **cur_view);
        quorum_network
            .inject_consensus_info(ConsensusIntentEvent::PollForVotes(**cur_view))
            .await;
    }

    broadcast_event(Arc::new(HotShotEvent::ViewChange(new_view)), event_stream).await;

    // Spawn a timeout task if we did actually update view
    *timeout_task = Some(async_spawn({
        let stream = event_stream.clone();
        // Nuance: We timeout on the view + 1 here because that means that we have
        // not seen evidence to transition to this new view
        let view_number = next_view;
        async move {
            async_sleep(Duration::from_millis(timeout)).await;
            broadcast_event(
                Arc::new(HotShotEvent::Timeout(TYPES::Time::new(*view_number))),
                &stream,
            )
            .await;
        }
    }));
    let consensus = consensus.upgradable_read().await;
    consensus
        .metrics
        .current_view
        .set(usize::try_from(cur_view.get_u64()).unwrap());

    // Do the comparison before the subtraction to avoid potential overflow, since
    // `last_decided_view` may be greater than `cur_view` if the node is catching up.
    if usize::try_from(cur_view.get_u64()).unwrap()
        > usize::try_from(consensus.last_decided_view.get_u64()).unwrap()
    {
        consensus.metrics.number_of_views_since_last_decide.set(
            usize::try_from(cur_view.get_u64()).unwrap()
                - usize::try_from(consensus.last_decided_view.get_u64()).unwrap(),
        );
    }
    let mut consensus = RwLockUpgradableReadGuard::upgrade(consensus).await;
    consensus.update_view(new_view);

    Ok(())
}
