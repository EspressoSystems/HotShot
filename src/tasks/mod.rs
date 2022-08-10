//! Provides a number of tasks that run continuously on a [`HotShot`]

// mod round_runner;

use crate::{types::HotShotHandle, HotShot, HotShotConsensusApi};
use async_std::{
    prelude::FutureExt,
    sync::RwLock,
    task::{sleep, spawn, JoinHandle},
};

use futures::{channel::oneshot::channel as oneshot_channel};
use hotshot_consensus::{Leader, NextLeader, Replica, ConsensusApi};
use hotshot_types::{
    message::{MessageKind},
    traits::{network::NetworkingImplementation, node_implementation::NodeImplementation},
};
use hotshot_utils::{broadcast::channel, hack::nll_todo};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tracing::{error, info, info_span, trace, Instrument};

/// A handle with senders to send events to the background runners.
#[derive(Default)]
pub struct TaskHandle {
    /// Inner struct of the [`TaskHandle`]. This is `None` by default but should be initialized early on in the [`HotShot`] struct. It should be safe to `unwrap` this.
    inner: RwLock<Option<TaskHandleInner>>,
}
impl TaskHandle {

    /// Get the internal state of the [`round_runner_task`].
    ///
    /// This will time out after two seconds.
    pub async fn get_round_runner_state(&self) -> Result<(), Box<dyn std::error::Error>> {
        let (_sender, receiver) = oneshot_channel();
        // self.send_to_round_runner(ToRoundRunner::GetState(sender))
        //     .await?;
        let _state = receiver.timeout(Duration::from_millis(30000)).await??;
        nll_todo()
        // Ok(state)
    }

    /// Start the round runner. This will make it run until `pause` is called
    pub async fn start(&self) {
        // self.send_to_round_runner(ToRoundRunner::Run)
        //     .await
        //     .expect("Could not start the round runner");
    }

    /// Make the round runner run 1 round.
    pub async fn run_one_round(&self) {
        // self.send_to_round_runner(ToRoundRunner::RunOnce)
        //     .await
        //     .expect("Could not start the round runner");
    }

    /// Pause the round runner.
    pub async fn pause(&self) {
        // self.send_to_round_runner(ToRoundRunner::Pause)
        //     .await
        //     .expect("Could not pause the round runner");
    }

    /// Wait until all underlying handles are shut down
    pub async fn wait_shutdown(&self) {
        let inner = self.inner.write().await.take().unwrap();

        // shutdown_timeout == the hotshot's view timeout
        // in case the round_runner task is running for `view_timeout`
        // (exponential timeout maxed out)
        // then this needs to be slightly longer such that it ends up being checked
        let long_timeout = inner.shutdown_timeout + Duration::new(1, 0);
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
    /// Join handle for `network_broadcast_task`
    pub network_broadcast_task_handle: JoinHandle<()>,

    /// Join handle for `network_direct_task`
    pub network_direct_task_handle: JoinHandle<()>,

    /// Join handle for `network_change_task`
    pub network_change_task_handle: JoinHandle<()>,

    /// Join handle for `consensus_task`
    pub consensus_task_handle: JoinHandle<()>,

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

    let consensus_task_handle = spawn(
        view_runner(hotshot.clone(), shut_down.clone())
            .instrument(info_span!("Consensus task handle",)),
    );

    let (broadcast_sender, broadcast_receiver) = channel();

    // let round_runner = round_runner::RoundRunner::new(hotshot.clone()).await;
    // let (round_runner, round_runner_join_handle) = {
    //     let sender = round_runner.sender.clone();
    //     let join_handle = async_std::task::spawn(round_runner.run());
    //     (sender, join_handle)
    // };

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
    });

    handle
}

pub async fn run_view<I: NodeImplementation<N>, const N: usize>(hotshot: HotShot<I, N>) -> Result<(), ()> {
    let next_view_timeout = hotshot.inner.config.next_view_timeout;

    // OBTAIN write lock on consensus
    let mut consensus = hotshot.hotstuff.write().await;

    let cur_view = consensus.increment_view();

    // Creates the view state if it does not already exist
    // Unwrap is fine b/c getting none here shouldn't be possible
    let (_, recv_replica) = consensus.get_future_view_pair(cur_view).unwrap();

    // let block = consensus.undecided_leaves.get(consensus.high_qc.leaf_hash);

    // TODO repalce these with sensible values
    let mut next_leader: NextLeader<I, N> = NextLeader {
        generic_qc: consensus.high_qc.clone(),
        vote_collection_chan: hotshot.recv_next_leader.clone(),
        cur_view,
    };
    let mut leader: Leader<I, N> = Leader {
        consensus: hotshot.hotstuff.clone(),
        high_qc: consensus.high_qc.clone(),
        cur_view,
        transactions: consensus.transactions.clone(),
    };
    let mut replica: Replica<I, N> = Replica {
        consensus: hotshot.hotstuff.clone(),
        proposal_collection_chan: recv_replica,
        cur_view,
        high_qc: consensus.high_qc.clone(),
    };
    // DROP write lock on consensus
    drop(consensus);


    let mut task_handles = Vec::new();

    // TODO move &api into struct such that we avoid this ugly code path

    let c_api = HotShotConsensusApi { inner: & hotshot.inner };

    {
        let inner = hotshot.inner.clone();
        let replica_handle = spawn(async move {
            let c_api = &HotShotConsensusApi { inner: & inner };
            replica.run_view(c_api).await
        });
        task_handles.push(replica_handle);
    }



    if c_api.is_leader(cur_view).await {
        {
            let inner = hotshot.inner.clone();
            let leader_handle = spawn(async move {
                let c_api = &HotShotConsensusApi { inner: & inner };
                leader.run_view(c_api).await
            });
            task_handles.push(leader_handle);
        }
    }

    if c_api.is_leader(cur_view + 1).await {
        {
            let inner = hotshot.inner.clone();
            let next_leader_handle = spawn(async move {
                let c_api = &HotShotConsensusApi { inner: & inner };
                next_leader.run_view(c_api).await
            });
            task_handles.push(next_leader_handle);
        }
    }


    let children_finished =
        futures::future::join_all(task_handles);

    spawn({
        let next_view_timeout = next_view_timeout.clone();
        let hotshot: HotShot<I, N> = hotshot.clone();
        async move {
            sleep(Duration::from_millis(next_view_timeout)).await;
            hotshot.timeout_view(cur_view).await;
        }
    });

    let results = children_finished.await;

    // unwrap is fine since results must have >= 1 item(s)
    let high_qc = results.into_iter().max_by_key(|qc| qc.view_number).unwrap();

    let mut consensus = hotshot.hotstuff.write().await;
    consensus.high_qc = high_qc;
    consensus.collect_garbage().await;
    // GC consensus step based on success of view
    Ok(())
}

/// main thread driving consensus
pub async fn view_runner<I: NodeImplementation<N>, const N: usize>(
    hotshot: HotShot<I, N>,
    shut_down: Arc<AtomicBool>,
) {
    while !shut_down.load(Ordering::Relaxed) {
        run_view(hotshot.clone()).await;
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
