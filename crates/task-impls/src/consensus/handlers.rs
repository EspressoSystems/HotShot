// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::{sync::Arc, time::Duration};

use async_broadcast::Sender;
use chrono::Utc;
use hotshot_types::{
    event::{Event, EventType},
    simple_vote::{QuorumVote2, TimeoutData, TimeoutVote},
    traits::{
        election::Membership,
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
    },
    vote::HasViewNumber,
};
use tokio::{spawn, time::sleep};
use tracing::instrument;
use utils::anytrace::*;
use vbs::version::StaticVersionType;

use super::ConsensusTaskState;
use crate::{
    consensus::Versions, events::HotShotEvent, helpers::broadcast_event,
    vote_collection::handle_vote,
};

/// Handle a `QuorumVoteRecv` event.
pub(crate) async fn handle_quorum_vote_recv<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    V: Versions,
>(
    vote: &QuorumVote2<TYPES>,
    event: Arc<HotShotEvent<TYPES>>,
    sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    task_state: &mut ConsensusTaskState<TYPES, I, V>,
) -> Result<()> {
    let is_vote_leaf_extended = task_state
        .consensus
        .read()
        .await
        .is_leaf_extended(vote.data.leaf_commit);
    let we_are_leader = task_state
        .membership
        .leader(vote.view_number() + 1, task_state.cur_epoch)?
        == task_state.public_key;
    ensure!(
        is_vote_leaf_extended || we_are_leader,
        info!(
            "We are not the leader for view {:?} and this is not the last vote for eQC",
            vote.view_number() + 1
        )
    );

    handle_vote(
        &mut task_state.vote_collectors,
        vote,
        task_state.public_key.clone(),
        &task_state.membership,
        task_state.cur_epoch,
        task_state.id,
        &event,
        sender,
        &task_state.upgrade_lock,
        !is_vote_leaf_extended,
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
            .membership
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
        &task_state.membership,
        task_state.cur_epoch,
        task_state.id,
        &event,
        sender,
        &task_state.upgrade_lock,
        true,
    )
    .await?;

    Ok(())
}

/// Send an event to the next leader containing the highest QC we have
/// This is a necessary part of HotStuff 2 but not the original HotStuff
///
/// #Errors
/// Returns and error if we can't get the version or the version doesn't
/// yet support HS 2
pub async fn send_high_qc<TYPES: NodeType, V: Versions, I: NodeImplementation<TYPES>>(
    new_view_number: TYPES::View,
    sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    task_state: &mut ConsensusTaskState<TYPES, I, V>,
) -> Result<()> {
    let version = task_state.upgrade_lock.version(new_view_number).await?;
    ensure!(
        version >= V::Epochs::VERSION,
        debug!("HotStuff 2 upgrade not yet in effect")
    );
    let high_qc = task_state.consensus.read().await.high_qc().clone();
    let leader = task_state
        .membership
        .leader(new_view_number, TYPES::Epoch::new(0))?;
    broadcast_event(
        Arc::new(HotShotEvent::HighQcSend(
            high_qc,
            leader,
            task_state.public_key.clone(),
        )),
        sender,
    )
    .await;
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
    epoch_number: TYPES::Epoch,
    sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    task_state: &mut ConsensusTaskState<TYPES, I, V>,
) -> Result<()> {
    if epoch_number > task_state.cur_epoch {
        task_state.cur_epoch = epoch_number;
        tracing::info!("Progress: entered epoch {:>6}", *epoch_number);
    }

    ensure!(
        new_view_number > task_state.cur_view,
        "New view is not larger than the current view"
    );

    let old_view_number = task_state.cur_view;
    tracing::debug!("Updating view from {old_view_number:?} to {new_view_number:?}");

    if *old_view_number / 100 != *new_view_number / 100 {
        tracing::info!("Progress: entered view {:>6}", *new_view_number);
    }

    // Send our high qc to the next leader immediately upon finishing a view.
    // Part of HotStuff 2
    let _ = send_high_qc(new_view_number, sender, task_state)
        .await
        .inspect_err(|e| {
            tracing::debug!("High QC sending failed with error: {:?}", e);
        });

    // Move this node to the next view
    task_state.cur_view = new_view_number;
    task_state
        .consensus
        .write()
        .await
        .update_view(new_view_number)?;

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
    let new_timeout_task = spawn({
        let stream = sender.clone();
        let view_number = new_view_number;
        async move {
            sleep(Duration::from_millis(timeout)).await;
            broadcast_event(
                Arc::new(HotShotEvent::Timeout(TYPES::View::new(*view_number))),
                &stream,
            )
            .await;
        }
    });

    // Cancel the old timeout task
    std::mem::replace(&mut task_state.timeout_task, new_timeout_task).abort();

    let consensus_reader = task_state.consensus.read().await;
    consensus_reader
        .metrics
        .current_view
        .set(usize::try_from(task_state.cur_view.u64()).unwrap());
    let cur_view_time = Utc::now().timestamp();
    if task_state
        .membership
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
        > usize::try_from(consensus_reader.last_decided_view().u64()).unwrap()
    {
        consensus_reader
            .metrics
            .number_of_views_since_last_decide
            .set(
                usize::try_from(task_state.cur_view.u64()).unwrap()
                    - usize::try_from(consensus_reader.last_decided_view().u64()).unwrap(),
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
        task_state.cur_view <= view_number,
        "Timeout event is for an old view"
    );

    ensure!(
        task_state
            .membership
            .has_stake(&task_state.public_key, task_state.cur_epoch),
        debug!(
            "We were not chosen for the consensus committee for view {:?}",
            view_number
        )
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

    tracing::error!(
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
        .membership
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
