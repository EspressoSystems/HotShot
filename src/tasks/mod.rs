//! Provides a number of tasks that run continuously on a [`HotShot`]

// mod round_runner;

use crate::{types::HotShotHandle, HotShot, HotShotConsensusApi};
use async_std::{
    prelude::FutureExt,
    sync::RwLock,
    task::{sleep, spawn, JoinHandle},
};

use flume::{Receiver, Sender};

use hotshot_consensus::{ConsensusApi, Leader, NextLeader, Replica, ViewQueue};
use hotshot_types::{
    message::MessageKind,
    traits::{network::NetworkingImplementation, node_implementation::NodeImplementation},
};
use hotshot_utils::broadcast::channel;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tracing::{error, info, info_span, instrument, trace, Instrument};

/// A handle with senders to send events to the background runners.
#[derive(Default)]
pub struct TaskHandle {
    /// Inner struct of the [`TaskHandle`]. This is `None` by default but should be initialized early on in the [`HotShot`] struct. It should be safe to `unwrap` this.
    inner: RwLock<Option<TaskHandleInner>>,
}
impl TaskHandle {
    /// Start the round runner. This will make it run until `pause` is called
    pub async fn start(&self) {
        let handle = self.inner.read().await;
        if handle.is_some() {
            let handle = handle.as_ref().unwrap();
            handle.started.store(true, Ordering::Relaxed);
        }
    }

    /// Make the round runner run 1 round.
    /// TODO should this block?
    pub async fn run_one_round(&self) {
        let handle = self.inner.read().await;
        if handle.is_some() {
            let handle = handle.as_ref().unwrap();
            if let Some(s) = &handle.run_view_channels {
                handle.started.store(true, Ordering::Relaxed);
                let _ = s.send_async(()).await;
            } else {
                error!("Run one view channel not configured for this hotshot instance");
            }
        }
    }

    /// Wait until all underlying handles are shut down
    pub async fn wait_shutdown(&self) {
        let inner = self.inner.write().await.take().unwrap();

        // shutdown_timeout == the hotshot's view timeout
        // in case the round_runner task is running for `view_timeout`
        // (exponential timeout maxed out)
        // then this needs to be slightly longer such that it ends up being checked
        let long_timeout = inner.shutdown_timeout + Duration::new(20, 0);
        for (handle, name) in [
            (
                inner.network_broadcast_task_handle,
                "network_broadcast_task_handle",
            ),
            (
                inner.network_direct_task_handle,
                "network_direct_task_handle",
            ),
            (
                inner.network_change_task_handle,
                "network_change_task_handle",
            ),
            (inner.consensus_task_handle, "network_change_task_handle"),
        ] {
            assert!(
                handle.timeout(long_timeout).await.is_ok(),
                "{} did not shut down within a second",
                name
            );
        }
    }
}
/// Inner struct of the [`TaskHandle`]
struct TaskHandleInner {
    /// for the client to indicate "increment a view"
    /// only Some in Continuous exeuction mode
    /// otherwise None
    pub run_view_channels: Option<Sender<()>>,
    /// Join handle for `network_broadcast_task`
    pub network_broadcast_task_handle: JoinHandle<()>,

    /// Join handle for `network_direct_task`
    pub network_direct_task_handle: JoinHandle<()>,

    /// Join handle for `network_change_task`
    pub network_change_task_handle: JoinHandle<()>,

    /// Join handle for `consensus_task`
    pub consensus_task_handle: JoinHandle<()>,

    /// Global to signify the `HotShot` should be started
    pub(crate) started: Arc<AtomicBool>,

    /// same as hotshot's view_timeout such that
    /// there is not an accidental race between the two
    shutdown_timeout: Duration,
}

/// Spawn all tasks that operate on the given [`HotShot`].
///
/// For a list of which tasks are being spawned, see this module's documentation.
pub async fn spawn_all<I: NodeImplementation<N>, const N: usize>(
    hotshot: &HotShot<I, N>,
) -> HotShotHandle<I, N> {
    let shut_down = Arc::new(AtomicBool::new(false));
    let started = Arc::new(AtomicBool::new(false));

    let network_broadcast_task_handle = spawn(
        network_broadcast_task(hotshot.clone(), shut_down.clone())
            .instrument(info_span!("HotShot Broadcast Task",)),
    );
    let network_direct_task_handle = spawn(
        network_direct_task(hotshot.clone(), shut_down.clone())
            .instrument(info_span!("HotShot Direct Task",)),
    );
    let network_change_task_handle = spawn(
        network_change_task(hotshot.clone(), shut_down.clone())
            .instrument(info_span!("HotShot network change listener task",)),
    );

    let (handle_channels, task_channels) = match hotshot.inner.config.execution_type {
        crate::ExecutionType::Continuous => (None, None),
        crate::ExecutionType::Incremental => {
            let (send_consensus_start, recv_consensus_start) = flume::unbounded();
            (Some(send_consensus_start), Some(recv_consensus_start))
        }
    };

    let consensus_task_handle = spawn(
        view_runner(
            hotshot.clone(),
            started.clone(),
            shut_down.clone(),
            task_channels,
        )
        .instrument(info_span!("Consensus task handle",)),
    );

    let (broadcast_sender, broadcast_receiver) = channel();

    let handle = HotShotHandle {
        sender_handle: Arc::new(broadcast_sender.clone()),
        hotshot: hotshot.clone(),
        stream_output: broadcast_receiver,
        storage: hotshot.inner.storage.clone(),
        // TODO this should really be in the same place as started...
        shut_down,
    };
    *hotshot.inner.event_sender.write().await = Some(broadcast_sender);

    let mut background_task_handle = hotshot.inner.background_task_handle.inner.write().await;
    *background_task_handle = Some(TaskHandleInner {
        network_broadcast_task_handle,
        network_direct_task_handle,
        network_change_task_handle,
        consensus_task_handle,
        shutdown_timeout: Duration::from_millis(hotshot.inner.config.next_view_timeout),
        run_view_channels: handle_channels,
        started,
    });

    handle
}

/// Executes one view of consensus
#[instrument(skip(hotshot), fields(id = hotshot.id), name = "View Runner Task", level = "error")]
pub async fn run_view<I: NodeImplementation<N>, const N: usize>(
    hotshot: HotShot<I, N>,
) -> Result<(), ()> {
    // do book keeping on channel map
    // e.g. insert the view and remove the last view
    let mut send_to_tasks = hotshot.send_to_tasks.write().await;
    let last_view = send_to_tasks.cur_view;
    send_to_tasks.cur_view += 1;
    let cur_view = send_to_tasks.cur_view;
    let (send_replica, recv_replica) = match send_to_tasks.replica_channel_map.entry(cur_view) {
        std::collections::btree_map::Entry::Vacant(v) => {
            let vq = ViewQueue::default();
            let result = (vq.sender_chan.clone(), vq.receiver_chan.clone());
            v.insert(vq);
            result
        }
        std::collections::btree_map::Entry::Occupied(o) => {
            let result = o.get();
            (result.sender_chan.clone(), result.receiver_chan.clone())
        }
    };

    // gc previous view's channel map
    send_to_tasks.replica_channel_map.remove(&last_view);
    drop(send_to_tasks);

    // increment consensus and start tasks
    error!("Running View!");
    let next_view_timeout = hotshot.inner.config.next_view_timeout;

    // OBTAIN write lock on consensus
    let mut consensus = hotshot.hotstuff.write().await;

    let cur_view = consensus.increment_view();

    // Creates the view state if it does not already exist
    // Unwrap is fine b/c getting none here shouldn't be possible

    let c_api = HotShotConsensusApi {
        inner: hotshot.inner.clone(),
    };

    let next_leader = NextLeader {
        id: hotshot.id,
        generic_qc: consensus.high_qc.clone(),
        vote_collection_chan: hotshot.recv_next_leader.clone(),
        cur_view,
        api: c_api.clone(),
    };
    let leader = Leader {
        id: hotshot.id,
        consensus: hotshot.hotstuff.clone(),
        high_qc: consensus.high_qc.clone(),
        cur_view,
        transactions: consensus.transactions.clone(),
        api: c_api.clone(),
    };
    let replica = Replica {
        id: hotshot.id,
        consensus: hotshot.hotstuff.clone(),
        proposal_collection_chan: recv_replica,
        cur_view,
        high_qc: consensus.high_qc.clone(),
        api: c_api.clone(),
    };
    // DROP write lock on consensus
    drop(consensus);

    let mut task_handles = Vec::new();

    let replica_handle = spawn(async move { replica.run_view().await });
    task_handles.push(replica_handle);

    if c_api.is_leader(cur_view).await {
        let leader_handle = spawn(async move { leader.run_view().await });
        task_handles.push(leader_handle);
    }

    if c_api.is_leader(cur_view + 1).await {
        let next_leader_handle = spawn(async move { next_leader.run_view().await });
        task_handles.push(next_leader_handle);
    }

    let children_finished = futures::future::join_all(task_handles);

    spawn({
        let next_view_timeout = next_view_timeout;
        let hotshot: HotShot<I, N> = hotshot.clone();
        async move {
            sleep(Duration::from_millis(next_view_timeout)).await;
            hotshot.timeout_view(cur_view, send_replica).await;
        }
    });

    let results = children_finished.await;
    error!("finished children tasks!");

    // unwrap is fine since results must have >= 1 item(s)
    let high_qc = results.into_iter().max_by_key(|qc| qc.view_number).unwrap();

    let mut consensus = hotshot.hotstuff.write().await;
    consensus.high_qc = high_qc;
    error!("returning!");
    Ok(())
}

/// main thread driving consensus
pub async fn view_runner<I: NodeImplementation<N>, const N: usize>(
    hotshot: HotShot<I, N>,
    started: Arc<AtomicBool>,
    shut_down: Arc<AtomicBool>,
    run_once: Option<Receiver<()>>,
) {
    while !shut_down.load(Ordering::Relaxed) && !started.load(Ordering::Relaxed) {}

    while !shut_down.load(Ordering::Relaxed) && started.load(Ordering::Relaxed) {
        if let Some(ref recv) = run_once {
            error!("WAITING FOR RUN");
            let _ = recv.recv_async().await;
        }
        error!("STARTING VIEW");
        let _ = run_view(hotshot.clone()).await;
    }
}

/// Continually processes the incoming broadcast messages received on `hotshot.inner.networking`, redirecting them to `hotshot.handle_broadcast_*_message`.
pub async fn network_broadcast_task<I: NodeImplementation<N>, const N: usize>(
    hotshot: HotShot<I, N>,
    shut_down: Arc<AtomicBool>,
) {
    info!("Launching broadcast processing task");
    let networking = &hotshot.inner.networking;
    let mut incremental_backoff_ms = 10;
    while !shut_down.load(Ordering::Relaxed) {
        let queue = match networking.broadcast_queue().await {
            Ok(queue) => queue,
            Err(e) => {
                if !shut_down.load(Ordering::Relaxed) {
                    error!(?e, "did not shut down gracefully.");
                }
                return;
            }
        };
        if queue.is_empty() {
            trace!("No message, sleeping for {} ms", incremental_backoff_ms);
            async_std::task::sleep(Duration::from_millis(incremental_backoff_ms)).await;
            incremental_backoff_ms = (incremental_backoff_ms * 2).min(1000);
            continue;
        }
        // Make sure to reset the backoff time
        incremental_backoff_ms = 10;
        for item in queue {
            trace!(?item, "Processing item");
            match item.kind {
                MessageKind::Consensus(msg) => {
                    hotshot
                        .handle_broadcast_consensus_message(msg, item.sender)
                        .await;
                }
                MessageKind::Data(msg) => {
                    hotshot
                        .handle_broadcast_data_message(msg, item.sender)
                        .await;
                }
            }
        }
        trace!("Items processed, querying for more");
    }
}

/// Continually processes the incoming direct messages received on `hotshot.inner.networking`, redirecting them to `hotshot.handle_direct_*_message`.
pub async fn network_direct_task<I: NodeImplementation<N>, const N: usize>(
    hotshot: HotShot<I, N>,
    shut_down: Arc<AtomicBool>,
) {
    info!("Launching direct processing task");
    let networking = &hotshot.inner.networking;
    let mut incremental_backoff_ms = 10;
    while !shut_down.load(Ordering::Relaxed) {
        let queue = match networking.direct_queue().await {
            Ok(queue) => queue,
            Err(e) => {
                if !shut_down.load(Ordering::Relaxed) {
                    error!(?e, "did not shut down gracefully.");
                }
                return;
            }
        };
        if queue.is_empty() {
            trace!("No message, sleeping for {} ms", incremental_backoff_ms);
            async_std::task::sleep(Duration::from_millis(incremental_backoff_ms)).await;
            incremental_backoff_ms = (incremental_backoff_ms * 2).min(1000);
            continue;
        }
        // Make sure to reset the backoff time
        incremental_backoff_ms = 10;
        for item in queue {
            trace!(?item, "Processing item");
            match item.kind {
                MessageKind::Consensus(msg) => {
                    hotshot
                        .handle_direct_consensus_message(msg, item.sender)
                        .await;
                }
                MessageKind::Data(msg) => {
                    hotshot.handle_direct_data_message(msg, item.sender).await;
                }
            }
        }
        trace!("Items processed, querying for more");
    }
}

/// Runs a task that will call `hotshot.handle_network_change` whenever a change in the network is detected.
pub async fn network_change_task<I: NodeImplementation<N>, const N: usize>(
    hotshot: HotShot<I, N>,
    shut_down: Arc<AtomicBool>,
) {
    info!("Launching network change handler task");
    let networking = &hotshot.inner.networking;
    let mut incremental_backoff_ms = 10;
    while !shut_down.load(Ordering::Relaxed) {
        let queue = match networking.network_changes().await {
            Ok(queue) => queue,
            Err(e) => {
                if !shut_down.load(Ordering::Relaxed) {
                    error!(?e, "did not shut down gracefully.");
                }
                return;
            }
        };
        if queue.is_empty() {
            trace!("No message, sleeping for {} ms", incremental_backoff_ms);
            async_std::task::sleep(Duration::from_millis(incremental_backoff_ms)).await;
            incremental_backoff_ms = (incremental_backoff_ms * 2).min(1000);
            continue;
        }
        // Make sure to reset the backoff time
        incremental_backoff_ms = 10;

        for node in queue {
            hotshot.handle_network_change(node).await;
        }
    }
}
