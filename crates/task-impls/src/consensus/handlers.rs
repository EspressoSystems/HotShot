// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::{sync::Arc, time::Duration};

use async_broadcast::Sender;
use async_compatibility_layer::art::{async_sleep, async_spawn};
use chrono::Utc;
use hotshot_types::{
    event::{Event, EventType},
    simple_vote::{QuorumVote, TimeoutData, TimeoutVote},
    traits::{
        election::Membership,
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
    },
    vote::HasViewNumber,
};
use tracing::instrument;
use utils::anytrace::*;

use super::ConsensusTaskState;
use crate::{
    consensus::Versions,
    events::HotShotEvent,
    helpers::{broadcast_event, cancel_task},
    vote_collection::handle_vote,
};

/// Handle a `QuorumVoteRecv` event.
pub(crate) async fn handle_quorum_vote_recv<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    V: Versions,
>(
    vote: &QuorumVote<TYPES>,
    event: Arc<HotShotEvent<TYPES>>,
    sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    task_state: &mut ConsensusTaskState<TYPES, I, V>,
) -> Result<()> {
    // Are we the leader for this view?
    ensure!(
        task_state
            .quorum_membership
            .leader(vote.view_number() + 1, task_state.cur_epoch)?
            == task_state.public_key,
        info!(
            "We are not the leader for view {:?}",
            vote.view_number() + 1
        )
    );

    handle_vote(
        &mut task_state.vote_collectors,
        vote,
        task_state.public_key.clone(),
        &task_state.quorum_membership,
        task_state.cur_epoch,
        task_state.id,
        &event,
        sender,
        &task_state.upgrade_lock,
    )
    .await?;

    Ok(())
}

/// Handle a `TimeoutVoteRecv` event.
pub(crate) async fn handle_timeout_vote_recv<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    V: Versions,
>(
    vote: &TimeoutVote<TYPES>,
    event: Arc<HotShotEvent<TYPES>>,
    sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    task_state: &mut ConsensusTaskState<TYPES, I, V>,
) -> Result<()> {
    // Are we the leader for this view?
    ensure!(
        task_state
            .timeout_membership
            .leader(vote.view_number() + 1, task_state.cur_epoch)?
            == task_state.public_key,
        info!(
            "We are not the leader for view {:?}",
            vote.view_number() + 1
        )
    );

    handle_vote(
        &mut task_state.timeout_vote_collectors,
        vote,
        task_state.public_key.clone(),
        &task_state.quorum_membership,
        task_state.cur_epoch,
        task_state.id,
        &event,
        sender,
        &task_state.upgrade_lock,
    )
    .await?;

    Ok(())
}

/// Handle a `ViewChange` event.
#[instrument(skip_all)]
pub(crate) async fn handle_view_change<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    V: Versions,
>(
    new_view_number: TYPES::View,
    sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    task_state: &mut ConsensusTaskState<TYPES, I, V>,
) -> Result<()> {
    ensure!(
        new_view_number > task_state.cur_view,
        "New view is not larger than the current view"
    );

    let old_view_number = task_state.cur_view;
    tracing::debug!("Updating view from {old_view_number:?} to {new_view_number:?}");

    // Move this node to the next view
    task_state.cur_view = new_view_number;

    // If we have a decided upgrade certificate, the protocol version may also have been upgraded.
    let decided_upgrade_certificate_read = task_state
        .upgrade_lock
        .decided_upgrade_certificate
        .read()
        .await
        .clone();
    if let Some(cert) = decided_upgrade_certificate_read {
        if new_view_number == cert.data.new_version_first_view {
            tracing::error!(
                "Version upgraded based on a decided upgrade cert: {:?}",
                cert
            );
        }
    }

    // Spawn a timeout task if we did actually update view
    let timeout = task_state.timeout;
    let new_timeout_task = async_spawn({
        let stream = sender.clone();
        // Nuance: We timeout on the view + 1 here because that means that we have
        // not seen evidence to transition to this new view
        let view_number = new_view_number + 1;
        async move {
            async_sleep(Duration::from_millis(timeout)).await;
            broadcast_event(
                Arc::new(HotShotEvent::Timeout(TYPES::View::new(*view_number))),
                &stream,
            )
            .await;
        }
    });

    // Cancel the old timeout task
    cancel_task(std::mem::replace(
        &mut task_state.timeout_task,
        new_timeout_task,
    ))
    .await;

    let consensus_reader = task_state.consensus.read().await;
    consensus_reader
        .metrics
        .current_view
        .set(usize::try_from(task_state.cur_view.u64()).unwrap());
    let cur_view_time = Utc::now().timestamp();
    if task_state
        .quorum_membership
        .leader(old_view_number, task_state.cur_epoch)?
        == task_state.public_key
    {
        #[allow(clippy::cast_precision_loss)]
        consensus_reader
            .metrics
            .view_duration_as_leader
            .add_point((cur_view_time - task_state.cur_view_time) as f64);
    }
    task_state.cur_view_time = cur_view_time;

    // Do the comparison before the subtraction to avoid potential overflow, since
    // `last_decided_view` may be greater than `cur_view` if the node is catching up.
    if usize::try_from(task_state.cur_view.u64()).unwrap()
        > usize::try_from(task_state.last_decided_view.u64()).unwrap()
    {
        consensus_reader
            .metrics
            .number_of_views_since_last_decide
            .set(
                usize::try_from(task_state.cur_view.u64()).unwrap()
                    - usize::try_from(task_state.last_decided_view.u64()).unwrap(),
            );
    }

    broadcast_event(
        Event {
            view_number: old_view_number,
            event: EventType::ViewFinished {
                view_number: old_view_number,
            },
        },
        &task_state.output_event_stream,
    )
    .await;
    Ok(())
}

/// Handle a `Timeout` event.
#[instrument(skip_all)]
pub(crate) async fn handle_timeout<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions>(
    view_number: TYPES::View,
    sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    task_state: &mut ConsensusTaskState<TYPES, I, V>,
) -> Result<()> {
    ensure!(
        task_state.cur_view < view_number,
        "Timeout event is for an old view"
    );

    ensure!(
        task_state
            .timeout_membership
            .has_stake(&task_state.public_key, task_state.cur_epoch),
        debug!("We were not chosen for the consensus committee for view {view_number:?}")
    );

    let vote = TimeoutVote::create_signed_vote(
        TimeoutData::<TYPES> { view: view_number },
        view_number,
        &task_state.public_key,
        &task_state.private_key,
        &task_state.upgrade_lock,
    )
    .await
    .wrap()
    .context(error!("Failed to sign TimeoutData"))?;

    broadcast_event(Arc::new(HotShotEvent::TimeoutVoteSend(vote)), sender).await;
    broadcast_event(
        Event {
            view_number,
            event: EventType::ViewTimeout { view_number },
        },
        &task_state.output_event_stream,
    )
    .await;

    tracing::debug!(
        "We did not receive evidence for view {} in time, sending timeout vote for that view!",
        *view_number
    );

    broadcast_event(
        Event {
            view_number,
            event: EventType::ReplicaViewTimeout { view_number },
        },
        &task_state.output_event_stream,
    )
    .await;

    task_state
        .consensus
        .read()
        .await
        .metrics
        .number_of_timeouts
        .add(1);
    if task_state
        .quorum_membership
        .leader(view_number, task_state.cur_epoch)?
        == task_state.public_key
    {
        task_state
            .consensus
            .read()
            .await
            .metrics
            .number_of_timeouts_as_leader
            .add(1);
    }

    Ok(())
}
