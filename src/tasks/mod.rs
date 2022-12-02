//! Provides a number of tasks that run continuously on a [`HotShot`]

use crate::{create_or_obtain_chan_from_write, types::HotShotHandle, HotShot, HotShotConsensusApi};
use async_compatibility_layer::{
    art::{async_sleep, async_spawn, async_spawn_local, async_timeout},
    async_primitives::broadcast::channel,
    channel::{unbounded, UnboundedReceiver, UnboundedSender},
};
use async_lock::RwLock;
use hotshot_consensus::{ConsensusApi, Leader, NextLeader, Replica, ViewQueue};
use hotshot_types::{
    constants::LOOK_AHEAD,
    message::MessageKind,
    traits::{
        network::NetworkingImplementation,
        node_implementation::{NodeImplementation, NodeTypes},
    },
    ExecutionType,
};
use std::{
    collections::HashMap,
    marker::PhantomData,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tracing::{error, info, info_span, instrument, trace, Instrument};

#[cfg(feature = "async-std-executor")]
use async_std::task::{yield_now, JoinHandle};
#[cfg(feature = "tokio-executor")]
use tokio::task::{yield_now, JoinHandle};
#[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
std::compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}

/// A handle with senders to send events to the background runners.
#[derive(Default)]
pub struct TaskHandle<TYPES: NodeTypes> {
    /// Inner struct of the [`TaskHandle`]. This is `None` by default but should be initialized early on in the [`HotShot`] struct. It should be safe to `unwrap` this.
    inner: RwLock<Option<TaskHandleInner>>,
    /// Reference to the [`NodeTypes`] used in this configuration
    _types: PhantomData<TYPES>,
}
impl<TYPES: NodeTypes> TaskHandle<TYPES> {
    /// Start the round runner. This will make it run until `pause` is called
    pub async fn start(&self) {
        let handle = self.inner.read().await;
        if handle.is_some() {
            let handle = handle.as_ref().unwrap();
            handle.started.store(true, Ordering::Relaxed);
        }
    }

    /// Make the round runner run 1 round.
    /// Does/should not block.
    pub async fn start_one_round(&self) {
        let handle = self.inner.read().await;
        if handle.is_some() {
            let handle = handle.as_ref().unwrap();
            if let Some(s) = &handle.run_view_channels {
                handle.started.store(true, Ordering::Relaxed);
                let _ = s.send(()).await;
            } else {
                error!("Run one view channel not configured for this hotshot instance");
            }
        }
    }

    /// Wait until all underlying handles are shut down
    pub async fn wait_shutdown(&self, send_network_lookup: UnboundedSender<Option<TYPES::Time>>) {
        let inner = self.inner.write().await.take().unwrap();

        // this shuts down the networking task
        if send_network_lookup.send(None).await.is_err() {
            error!("network lookup task already shut down!");
        }

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
                async_timeout(long_timeout, handle).await.is_ok(),
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
    pub run_view_channels: Option<UnboundedSender<()>>,
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
pub async fn spawn_all<TYPES: NodeTypes, I: NodeImplementation<TYPES>>(
    hotshot: &HotShot<TYPES, I>,
) -> HotShotHandle<TYPES, I> {
    let shut_down = Arc::new(AtomicBool::new(false));
    let started = Arc::new(AtomicBool::new(false));

    let network_broadcast_task_handle = async_spawn(
        network_broadcast_task(hotshot.clone(), shut_down.clone())
            .instrument(info_span!("HotShot Broadcast Task",)),
    );
    let network_direct_task_handle = async_spawn(
        network_direct_task(hotshot.clone(), shut_down.clone())
            .instrument(info_span!("HotShot Direct Task",)),
    );
    let network_change_task_handle = async_spawn(
        network_change_task(hotshot.clone(), shut_down.clone())
            .instrument(info_span!("HotShot network change listener task",)),
    );

    async_spawn(
        network_lookup_task(hotshot.clone(), shut_down.clone())
            .instrument(info_span!("HotShot network lookup task",)),
    );

    let (handle_channels, task_channels) = match hotshot.inner.config.execution_type {
        ExecutionType::Continuous => (None, None),
        ExecutionType::Incremental => {
            let (send_consensus_start, recv_consensus_start) = unbounded();
            (Some(send_consensus_start), Some(recv_consensus_start))
        }
    };

    let consensus_task_handle = async_spawn(
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
pub async fn run_view<TYPES: NodeTypes, I: NodeImplementation<TYPES>>(
    hotshot: HotShot<TYPES, I>,
) -> Result<(), ()> {
    let c_api = HotShotConsensusApi {
        inner: hotshot.inner.clone(),
    };
    let start = Instant::now();
    let metrics = Arc::clone(&hotshot.hotstuff.read().await.metrics);

    // do book keeping on channel map
    // TODO probably cleaner to separate this into a function
    // e.g. insert the view and remove the last view
    let mut send_to_replica = hotshot.replica_channel_map.write().await;
    let replica_last_view: TYPES::Time = send_to_replica.cur_view;
    // gc previous view's channel map
    send_to_replica.channel_map.remove(&replica_last_view);
    send_to_replica.cur_view += 1;
    let replica_cur_view = send_to_replica.cur_view;
    let ViewQueue {
        sender_chan: send_replica,
        receiver_chan: recv_replica,
        has_received_proposal: _,
    } = create_or_obtain_chan_from_write(replica_cur_view, send_to_replica).await;

    let mut send_to_next_leader = hotshot.next_leader_channel_map.write().await;
    let next_leader_last_view = send_to_next_leader.cur_view;
    // gc previous view's channel map
    send_to_next_leader
        .channel_map
        .remove(&next_leader_last_view);
    send_to_next_leader.cur_view += 1;
    let next_leader_cur_view = send_to_next_leader.cur_view;
    let (send_next_leader, recv_next_leader) = if c_api.is_leader(next_leader_cur_view + 1).await {
        let vq = create_or_obtain_chan_from_write(next_leader_cur_view, send_to_next_leader).await;
        (Some(vq.sender_chan), Some(vq.receiver_chan))
    } else {
        (None, None)
    };

    // increment consensus and start tasks

    let (cur_view, high_qc, txns) = {
        // OBTAIN write lock on consensus
        let mut consensus = hotshot.hotstuff.write().await;
        let cur_view = consensus.increment_view();
        // make sure consistent
        assert_eq!(cur_view, next_leader_cur_view);
        assert_eq!(cur_view, replica_cur_view);
        let high_qc = consensus.high_qc.clone();
        let txns = consensus.transactions.clone();
        // DROP write lock on consensus
        drop(consensus);
        (cur_view, high_qc, txns)
    };

    // notify networking to start worrying about the (`cur_view + LOOK_AHEAD`)th leader ahead of the current view
    if hotshot
        .send_network_lookup
        .send(Some(cur_view))
        .await
        .is_err()
    {
        error!("Failed to initiate network lookup");
    };

    info!("Starting tasks for View {:?}!", cur_view);
    metrics.current_view.set(*cur_view as usize);

    let mut task_handles = Vec::new();

    // replica always runs? TODO this will change once vrf integration is added
    let replica = Replica {
        id: hotshot.id,
        consensus: hotshot.hotstuff.clone(),
        proposal_collection_chan: recv_replica,
        cur_view,
        high_qc: high_qc.clone(),
        api: c_api.clone(),
    };
    let replica_handle = async_spawn(async move { replica.run_view().await });
    task_handles.push(replica_handle);

    if c_api.is_leader(cur_view).await {
        let leader = Leader {
            id: hotshot.id,
            consensus: hotshot.hotstuff.clone(),
            high_qc: high_qc.clone(),
            cur_view,
            transactions: txns.clone(),
            api: c_api.clone(),
        };
        let leader_handle = async_spawn(async move { leader.run_view().await });
        task_handles.push(leader_handle);
    }

    if c_api.is_leader(cur_view + 1).await {
        let next_leader = NextLeader {
            id: hotshot.id,
            generic_qc: high_qc,
            // should be fine to unwrap here since the view numbers must be the same
            vote_collection_chan: recv_next_leader.unwrap(),
            cur_view,
            api: c_api.clone(),
            metrics,
        };
        let next_leader_handle = async_spawn(async move { next_leader.run_view().await });
        task_handles.push(next_leader_handle);
    }

    let children_finished = futures::future::join_all(task_handles);

    async_spawn({
        let next_view_timeout = hotshot.inner.config.next_view_timeout;
        let next_view_timeout = next_view_timeout;
        let hotshot: HotShot<TYPES, I> = hotshot.clone();
        async move {
            async_sleep(Duration::from_millis(next_view_timeout)).await;
            hotshot
                .timeout_view(cur_view, send_replica, send_next_leader)
                .await;
        }
    });

    let results = children_finished.await;

    // unwrap is fine since results must have >= 1 item(s)
    #[cfg(feature = "async-std-executor")]
    let high_qc = results.into_iter().max_by_key(|qc| qc.view_number).unwrap();
    #[cfg(feature = "tokio-executor")]
    let high_qc = results
        .into_iter()
        .filter_map(std::result::Result::ok)
        .max_by_key(|qc| qc.view_number)
        .unwrap();

    #[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
    compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}

    let mut consensus = hotshot.hotstuff.write().await;
    consensus.high_qc = high_qc;
    consensus
        .metrics
        .view_duration
        .add_point(start.elapsed().as_secs_f64());
    c_api.send_view_finished(consensus.cur_view).await;

    info!("Returning from view {:?}!", cur_view);
    Ok(())
}

/// main thread driving consensus
pub async fn view_runner<TYPES: NodeTypes, I: NodeImplementation<TYPES>>(
    hotshot: HotShot<TYPES, I>,
    started: Arc<AtomicBool>,
    shut_down: Arc<AtomicBool>,
    run_once: Option<UnboundedReceiver<()>>,
) {
    while !shut_down.load(Ordering::Relaxed) && !started.load(Ordering::Relaxed) {
        yield_now().await;
    }

    while !shut_down.load(Ordering::Relaxed) && started.load(Ordering::Relaxed) {
        if let Some(ref recv) = run_once {
            let _ = recv.recv().await;
        }
        let _ = run_view(hotshot.clone()).await;
    }
}

/// Task to look up a node in the future as needed
pub async fn network_lookup_task<TYPES: NodeTypes, I: NodeImplementation<TYPES>>(
    hotshot: HotShot<TYPES, I>,
    shut_down: Arc<AtomicBool>,
) {
    info!("Launching network lookup task");
    let networking = hotshot.inner.networking.clone();
    let c_api = HotShotConsensusApi {
        inner: hotshot.inner.clone(),
    };

    let mut completion_map: HashMap<TYPES::Time, Arc<AtomicBool>> = HashMap::default();

    while !shut_down.load(Ordering::Relaxed) {
        let lock = hotshot.recv_network_lookup.lock().await;
        if let Ok(Some(cur_view)) = lock.recv().await {
            let view_to_lookup = cur_view + LOOK_AHEAD;

            // perform pruning
            // TODO in the future btreemap would be better
            completion_map = completion_map
                .drain()
                .filter(|(view, is_done)| {
                    if !is_done.load(Ordering::Relaxed) {
                        // we are past the view where this is useful
                        if cur_view >= *view {
                            is_done.store(true, Ordering::Relaxed);
                            return true;
                        }
                        // we aren't done
                        return false;
                    }
                    true
                })
                .collect();

            // logic to look ahead
            if !c_api.is_leader(view_to_lookup).await {
                let is_done = Arc::new(AtomicBool::new(false));
                completion_map.insert(view_to_lookup, is_done.clone());
                let c_api = c_api.clone();
                let networking = networking.clone();
                async_spawn_local(async move {
                    info!("starting lookup for {:?}", view_to_lookup);
                    networking
                        .notify_of_subsequent_leader(
                            c_api.get_leader(view_to_lookup).await,
                            is_done,
                        )
                        .await;
                    info!("finished lookup for {:?}", view_to_lookup);
                });
            }
        }
    }

    // shut down all child tasks
    for (_, is_done) in completion_map {
        is_done.store(true, Ordering::Relaxed);
    }
}

/// Continually processes the incoming broadcast messages received on `hotshot.inner.networking`, redirecting them to `hotshot.handle_broadcast_*_message`.
pub async fn network_broadcast_task<TYPES: NodeTypes, I: NodeImplementation<TYPES>>(
    hotshot: HotShot<TYPES, I>,
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
            async_sleep(Duration::from_millis(incremental_backoff_ms)).await;
            incremental_backoff_ms = (incremental_backoff_ms * 2).min(1000);
            continue;
        }
        // Make sure to reset the backoff time
        incremental_backoff_ms = 10;
        for item in queue {
            trace!(?item, "Processing item");
            hotshot
                .hotstuff
                .read()
                .await
                .metrics
                .broadcast_messages_received
                .add(1);
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
pub async fn network_direct_task<TYPES: NodeTypes, I: NodeImplementation<TYPES>>(
    hotshot: HotShot<TYPES, I>,
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
            async_sleep(Duration::from_millis(incremental_backoff_ms)).await;
            incremental_backoff_ms = (incremental_backoff_ms * 2).min(1000);
            continue;
        }
        // Make sure to reset the backoff time
        incremental_backoff_ms = 10;
        for item in queue {
            let metrics = Arc::clone(&hotshot.hotstuff.read().await.metrics);
            metrics.direct_messages_received.add(1);
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
pub async fn network_change_task<TYPES: NodeTypes, I: NodeImplementation<TYPES>>(
    hotshot: HotShot<TYPES, I>,
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
            async_sleep(Duration::from_millis(incremental_backoff_ms)).await;
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
