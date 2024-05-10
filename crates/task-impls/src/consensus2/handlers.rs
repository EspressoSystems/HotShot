use std::{sync::Arc, time::Duration};

use anyhow::{ensure, Context, Result};
use async_broadcast::Sender;
use async_compatibility_layer::art::{async_sleep, async_spawn};
use async_std::task;
use hotshot_task::{broadcast_event, cancel_task};
use hotshot_types::{
    event::{Event, EventType},
    hotshot_event::{HotShotEvent, HotShotTaskCompleted},
    simple_certificate::{QuorumCertificate, TimeoutCertificate},
    simple_vote::{QuorumVote, TimeoutData, TimeoutVote},
    traits::{
        election::Membership,
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
    },
    vote::HasViewNumber,
};
use tracing::debug;

use crate::vote_collection::{create_vote_accumulator, AccumulatorInfo, HandleVoteEvent};

use super::Consensus2TaskState;

pub(crate) async fn handle_quorum_vote_recv<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    vote: &QuorumVote<TYPES>,
    event: Arc<HotShotEvent<TYPES>>,
    sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    task_state: &mut Consensus2TaskState<TYPES, I>,
) -> Result<()> {
    // Are we the leader for this view?
    ensure!(
        task_state
            .quorum_membership
            .get_leader(vote.get_view_number() + 1)
            == task_state.public_key,
        format!(
            "We are not the leader for view {:?}",
            vote.get_view_number()
        )
    );

    let mut collector = task_state.vote_collector.write().await;

    if collector.is_none() || vote.get_view_number() > collector.as_ref().unwrap().view {
        let info = AccumulatorInfo {
            public_key: task_state.public_key.clone(),
            membership: Arc::clone(&task_state.quorum_membership),
            view: vote.get_view_number(),
            id: task_state.id,
        };
        *collector = create_vote_accumulator::<TYPES, QuorumVote<TYPES>, QuorumCertificate<TYPES>>(
            &info,
            vote.clone(),
            event,
            sender,
        )
        .await;
    } else {
        let result = collector
            .as_mut()
            .unwrap()
            .handle_event(Arc::clone(&event), sender)
            .await;

        if result == Some(HotShotTaskCompleted) {
            *collector = None;
        }
    }
    Ok(())
}

pub(crate) async fn handle_timeout_vote_recv<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    vote: &TimeoutVote<TYPES>,
    event: Arc<HotShotEvent<TYPES>>,
    sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    task_state: &mut Consensus2TaskState<TYPES, I>,
) -> Result<()> {
    // Are we the leader for this view?
    ensure!(
        task_state
            .timeout_membership
            .get_leader(vote.get_view_number() + 1)
            == task_state.public_key,
        format!(
            "We are not the leader for view {:?}",
            vote.get_view_number()
        )
    );

    let mut collector = task_state.timeout_vote_collector.write().await;

    if collector.is_none() || vote.get_view_number() > collector.as_ref().unwrap().view {
        let info = AccumulatorInfo {
            public_key: task_state.public_key.clone(),
            membership: Arc::clone(&task_state.quorum_membership),
            view: vote.get_view_number(),
            id: task_state.id,
        };
        *collector =
            create_vote_accumulator::<TYPES, TimeoutVote<TYPES>, TimeoutCertificate<TYPES>>(
                &info,
                vote.clone(),
                event,
                sender,
            )
            .await;
    } else {
        let result = collector
            .as_mut()
            .unwrap()
            .handle_event(Arc::clone(&event), sender)
            .await;

        if result == Some(HotShotTaskCompleted) {
            *collector = None;
        }
    }
    Ok(())
}

pub(crate) async fn handle_view_change<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    new_view_number: TYPES::Time,
    sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    task_state: &mut Consensus2TaskState<TYPES, I>,
) -> Result<()> {
    ensure!(
        new_view_number > task_state.cur_view,
        "New view is not larger than the current view"
    );

    let old_view_number = task_state.cur_view;
    debug!("Updating view from {old_view_number:?} to {new_view_number:?}");

    // Cancel the old timeout task
    if let Some(timeout_task) = task_state.timeout_task.take() {
        cancel_task(timeout_task).await;
    }

    // Move this node to the next view
    task_state.cur_view = new_view_number;

    // Spawn a timeout task if we did actually update view
    let timeout = task_state.timeout;
    task_state.timeout_task = Some(async_spawn({
        let stream = sender.clone();
        // Nuance: We timeout on the view + 1 here because that means that we have
        // not seen evidence to transition to this new view
        let view_number = new_view_number + 1;
        async move {
            async_sleep(Duration::from_millis(timeout)).await;
            broadcast_event(
                Arc::new(HotShotEvent::Timeout(TYPES::Time::new(*view_number))),
                &stream,
            )
            .await;
        }
    }));

    let metrics = task_state.metrics.read().await;
    metrics
        .current_view
        .set(usize::try_from(task_state.cur_view.get_u64()).unwrap());

    // Do the comparison before the subtraction to avoid potential overflow, since
    // `last_decided_view` may be greater than `cur_view` if the node is catching up.
    if usize::try_from(task_state.cur_view.get_u64()).unwrap()
        > usize::try_from(task_state.last_decided_view.get_u64()).unwrap()
    {
        metrics.number_of_views_since_last_decide.set(
            usize::try_from(task_state.cur_view.get_u64()).unwrap()
                - usize::try_from(task_state.last_decided_view.get_u64()).unwrap(),
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

pub(crate) async fn handle_timeout<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    view_number: TYPES::Time,
    sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    task_state: &mut Consensus2TaskState<TYPES, I>,
) -> Result<()> {
    ensure!(
        task_state.cur_view < view_number,
        "Timeout event is for an old view"
    );

    ensure!(
        task_state
            .timeout_membership
            .has_stake(&task_state.public_key),
        format!("We were not chosen for the consensus committee for view {view_number:?}")
    );

    let vote = TimeoutVote::create_signed_vote(
        TimeoutData::<TYPES> { view: view_number },
        view_number,
        &task_state.public_key,
        &task_state.private_key,
    )
    .context("Failed to sign TimeoutData")?;

    broadcast_event(Arc::new(HotShotEvent::TimeoutVoteSend(vote)), sender).await;
    broadcast_event(
        Event {
            view_number,
            event: EventType::ViewTimeout { view_number },
        },
        &task_state.output_event_stream,
    )
    .await;

    debug!(
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

    task_state.metrics.read().await.number_of_timeouts.add(1);

    Ok(())
}
