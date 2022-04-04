//! Provides a number of tasks that run continuously on a [`Phaselock`]

use crate::{types::PhaseLockHandle, PhaseLock};
use async_std::{
    prelude::FutureExt,
    sync::RwLock,
    task::{spawn, JoinHandle},
};
use flume::{Receiver, SendError, Sender, TryRecvError};
use phaselock_types::{
    data::Stage,
    event::{Event, EventType},
    message::Message,
    traits::{network::NetworkingImplementation, node_implementation::NodeImplementation},
};
use phaselock_utils::broadcast::channel;
use std::{sync::Arc, time::Duration};
use tracing::{error, info, info_span, trace, warn, Instrument};

/// A handle with senders to send events to the background runners.
#[derive(Default)]
pub struct TaskHandle {
    /// Inner struct of the [`TaskHandle`]. This is `None` by default but should be initialized early on in the [`PhaseLock`] struct. It should be safe to `unwrap` this.
    inner: RwLock<Option<TaskHandleInner>>,
}
impl TaskHandle {
    /// Send a message to the [`round_runner_task`].
    pub async fn send_to_round_runner(
        &self,
        msg: ToRoundRunner,
    ) -> Result<(), SendError<ToRoundRunner>> {
        self.inner
            .read()
            .await
            .as_ref()
            .unwrap()
            .round_runner
            .send_async(msg)
            .await
    }

    /// Wait until all underlying handles are shut down
    pub async fn wait_shutdown(&self) {
        let inner = self.inner.write().await.take().unwrap();
        futures::future::join_all([
            inner.network_broadcast_task_handle,
            inner.network_direct_task_handle,
            inner.network_change_task_handle,
            inner.round_runner_task_handle,
        ])
        .timeout(Duration::from_secs(1))
        .await
        .expect("Background tasks did not shut down in time");
    }
}
/// Inner struct of the [`TaskHandle`]
struct TaskHandleInner {
    /// The sender for the [`round_runner_task`].
    pub round_runner: Sender<ToRoundRunner>,

    /// Join handle for `network_broadcast_task`
    pub network_broadcast_task_handle: JoinHandle<()>,

    /// Join handle for `network_direct_task`
    pub network_direct_task_handle: JoinHandle<()>,

    /// Join handle for `network_change_task`
    pub network_change_task_handle: JoinHandle<()>,

    /// Join handle for `round_runner`
    pub round_runner_task_handle: JoinHandle<()>,
}

/// Events going to the round runner.
pub enum ToRoundRunner {
    /// Notify the round runner that there is a new view number inserted externally that it should use from now on
    NewViewNumber(u64),
}

/// Spawn all tasks that operate on the given [`PhaseLock`].
///
/// For a list of which tasks are being spawned, see this module's documentation.
pub async fn spawn_all<I: NodeImplementation<N>, const N: usize>(
    phaselock: &PhaseLock<I, N>,
) -> PhaseLockHandle<I, N> {
    let (round_runner, round_runner_receiver) = flume::unbounded();
    let shut_down = Arc::new(RwLock::new(false));

    let network_broadcast_task_handle = spawn(
        network_broadcast_task(phaselock.clone(), shut_down.clone()).instrument(info_span!(
            "PhaseLock Broadcast Task",
            id = phaselock.inner.public_key.nonce
        )),
    );
    let network_direct_task_handle = spawn(
        network_direct_task(phaselock.clone(), shut_down.clone()).instrument(info_span!(
            "PhaseLock Direct Task",
            id = phaselock.inner.public_key.nonce
        )),
    );
    let network_change_task_handle = spawn(
        network_change_task(phaselock.clone(), shut_down.clone()).instrument(info_span!(
            "PhaseLock network change listener task",
            id = phaselock.inner.public_key.nonce
        )),
    );

    let (sender, receiver) = channel();

    let pause = Arc::new(RwLock::new(true));
    let run_once = Arc::new(RwLock::new(false));

    let handle = PhaseLockHandle {
        sender_handle: Arc::new(sender.clone()),
        phaselock: phaselock.clone(),
        stream_output: receiver,
        pause: pause.clone(),
        run_once: run_once.clone(),
        shut_down: shut_down.clone(),
        storage: phaselock.inner.storage.clone(),
    };
    *phaselock.inner.event_sender.write().await = Some(sender);

    let node_id = phaselock.inner.public_key.nonce;
    let round_runner_task_handle = spawn(
        round_runner_task(
            phaselock.clone(),
            pause,
            run_once,
            shut_down,
            round_runner_receiver,
        )
        .instrument(info_span!("PhaseLock Background Driver", id = node_id)),
    );

    let mut background_task_handle = phaselock.inner.background_task_handle.inner.write().await;
    *background_task_handle = Some(TaskHandleInner {
        round_runner,
        network_broadcast_task_handle,
        network_direct_task_handle,
        network_change_task_handle,
        round_runner_task_handle,
    });

    handle
}

/// Continually processes the incoming broadcast messages received on `phaselock.inner.networking`, redirecting them to `phaselock.handle_broadcast_*_message`.
pub async fn network_broadcast_task<I: NodeImplementation<N>, const N: usize>(
    phaselock: PhaseLock<I, N>,
    shut_down: Arc<RwLock<bool>>,
) {
    info!("Launching broadcast processing task");
    let networking = &phaselock.inner.networking;
    let mut incremental_backoff_ms = 10;
    while !*shut_down.read().await {
        let queue = match networking.broadcast_queue().await {
            Ok(queue) => queue,
            Err(e) => {
                if !*shut_down.read().await {
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
            match item {
                Message::Consensus(msg) => {
                    phaselock.handle_broadcast_consensus_message(msg).await;
                }
                Message::Data(msg) => {
                    phaselock.handle_broadcast_data_message(msg).await;
                }
            }
        }
        trace!("Items processed, querying for more");
    }
}

/// Continually processes the incoming direct messages received on `phaselock.inner.networking`, redirecting them to `phaselock.handle_direct_*_message`.
pub async fn network_direct_task<I: NodeImplementation<N>, const N: usize>(
    phaselock: PhaseLock<I, N>,
    shut_down: Arc<RwLock<bool>>,
) {
    info!("Launching direct processing task");
    let networking = &phaselock.inner.networking;
    let mut incremental_backoff_ms = 10;
    while !*shut_down.read().await {
        let queue = match networking.direct_queue().await {
            Ok(queue) => queue,
            Err(e) => {
                if !*shut_down.read().await {
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
            match item {
                Message::Consensus(msg) => {
                    phaselock.handle_direct_consensus_message(msg).await;
                }
                Message::Data(msg) => {
                    phaselock.handle_direct_data_message(msg).await;
                }
            }
        }
        trace!("Items processed, querying for more");
    }
}

/// Runs a task that will call `phaselock.handle_network_change` whenever a change in the network is detected.
pub async fn network_change_task<I: NodeImplementation<N>, const N: usize>(
    phaselock: PhaseLock<I, N>,
    shut_down: Arc<RwLock<bool>>,
) {
    info!("Launching network change handler task");
    let networking = &phaselock.inner.networking;
    let mut incremental_backoff_ms = 10;
    while !*shut_down.read().await {
        let queue = match networking.network_changes().await {
            Ok(queue) => queue,
            Err(e) => {
                if !*shut_down.read().await {
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
            phaselock.handle_network_change(node).await;
        }
    }
}

/// Run the phaselock background handler loop.
///
/// - If `run_once` is set to `true`, it will only run once.
/// - If `pause` is set to `true` this will run until `pause` is set to `false`.
/// - If `shut_down` is set, this function will exit.
///
/// While this is running, it will continually call the following functions in order:
/// - [`phaselock.next_view`]
/// - [`phaselock.run_round`]
///
/// If any error occurs, they will be send to `phaselock.inner.event_sender` as an [`Event`] with `EventType::Error` or `EventType::Timeout` together with the current view number and stage.
///
/// [`phaselock.next_view`]: ../struct.PhaseLock.html#method.next_view
/// [`phaselock.run_round`]: ../struct.PhaseLock.html#method.run_round
#[allow(clippy::too_many_lines)]
pub async fn round_runner_task<I: NodeImplementation<N>, const N: usize>(
    phaselock: PhaseLock<I, N>,
    pause: Arc<RwLock<bool>>,
    run_once: Arc<RwLock<bool>>,
    shut_down: Arc<RwLock<bool>>,
    from_main: Receiver<ToRoundRunner>,
) {
    let duration = Duration::from_millis(phaselock.inner.config.start_delay);
    async_std::task::sleep(duration).await;
    let default_interrupt_duration = phaselock.inner.config.next_view_timeout;
    let (int_mul, int_div) = phaselock.inner.config.timeout_ratio;
    let mut int_duration = default_interrupt_duration;
    let mut view = 0;
    let mut incremental_backoff_ms = 10;
    // PhaseLock background handler loop
    loop {
        // First, check for shutdown signal and break if sent
        if *shut_down.read().await {
            break;
        }
        // Capture the pause and run_once flags
        // Reset the run_once flag if its set
        let p_flag = {
            let p = pause.read().await;
            let mut r = run_once.write().await;
            if *r {
                *r = false;
                false
            } else {
                *p
            }
        };
        // If we are paused, sleep and continue
        if p_flag {
            async_std::task::sleep(Duration::from_millis(incremental_backoff_ms)).await;
            incremental_backoff_ms = (incremental_backoff_ms * 2).min(1000);
            continue;
        }
        // Make sure to reset the backoff timeout
        incremental_backoff_ms = 10;

        // Listen for incoming messages
        loop {
            match from_main.try_recv() {
                Err(TryRecvError::Disconnected) => {
                    error!("Round runner can't receive events any more, `shut_down` should've notified us of this");
                    return;
                }
                Err(TryRecvError::Empty) => break,
                Ok(ToRoundRunner::NewViewNumber(new_view)) => {
                    info!(new_view, "Round runner received a new view number");
                    view = new_view;
                }
            }
        }

        // Send the next view
        let next_view_res = phaselock.next_view(view).await;
        // If we fail to send the next view, broadcast the error and pause
        if let Err(e) = next_view_res {
            if !phaselock
                .send_event(Event {
                    view_number: view,
                    stage: e.get_stage().unwrap_or(Stage::None),

                    event: EventType::Error { error: Arc::new(e) },
                })
                .await
            {
                error!("All event streams closed! Shutting down.");
                break;
            }
            *pause.write().await = true;
            continue;
        }
        // Increment the view counter
        view += 1;
        // run the next block, with a timeout
        let t = Duration::from_millis(int_duration);
        let round_res = async_std::future::timeout(t, phaselock.run_round(view)).await;
        match round_res {
            // If it succeded, simply reset the timeout
            Ok(Ok(x)) => {
                int_duration = default_interrupt_duration;
                // Check if we completed the same view we started
                if x != view {
                    info!(?x, ?view, "Round short circuited");
                    view = x;
                }
            }
            // If it errored, broadcast the error, reset the timeout, and continue
            Ok(Err(e)) => {
                if !phaselock
                    .send_event(Event {
                        view_number: view,
                        stage: e.get_stage().unwrap_or(Stage::None),
                        event: EventType::Error { error: Arc::new(e) },
                    })
                    .await
                {
                    error!("All event streams closed! Shutting down.");
                    break;
                }
                continue;
            }
            // if we timed out, log it, send the event, and increase the timeout
            Err(_) => {
                warn!("Round timed out");
                if !phaselock
                    .send_event(Event {
                        view_number: view,
                        stage: Stage::None,
                        event: EventType::ViewTimeout { view_number: view },
                    })
                    .await
                {
                    error!("All event streams closed! Shutting down.");
                    break;
                }
                int_duration = (int_duration * int_mul) / int_div;
            }
        }
    }
}
