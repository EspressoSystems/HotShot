//! Provides a number of tasks that run continuously on a [`Phaselock`]

use crate::{types::PhaseLockHandle, PhaseLock};
use async_std::{
    sync::RwLock,
    task::{spawn, JoinHandle},
};
use phaselock_types::{
    data::Stage,
    event::{Event, EventType},
    message::Message,
    traits::{network::NetworkingImplementation, node_implementation::NodeImplementation},
};
use phaselock_utils::broadcast::{channel, BroadcastSender};
use std::{sync::Arc, time::Duration};
use tracing::{error, info, info_span, trace, warn, Instrument};

/// Spawn all tasks that operate on the given [`PhaseLock`].
///
/// For a list of which tasks are being spawned, see this module's documentation.
pub fn spawn_all<I: NodeImplementation<N>, const N: usize>(
    phaselock: &PhaseLock<I, N>,
) -> (JoinHandle<()>, PhaseLockHandle<I, N>) {
    spawn(
        network_broadcast_task(phaselock.clone()).instrument(info_span!(
            "PhaseLock Broadcast Task",
            id = phaselock.inner.public_key.nonce
        )),
    );
    spawn(
        network_direct_task(phaselock.clone()).instrument(info_span!(
            "PhaseLock Direct Task",
            id = phaselock.inner.public_key.nonce
        )),
    );

    let (sender, receiver) = channel();

    let pause = Arc::new(RwLock::new(true));
    let run_once = Arc::new(RwLock::new(false));
    let shut_down = Arc::new(RwLock::new(false));

    let handle = PhaseLockHandle {
        sender_handle: Arc::new(sender.clone()),
        phaselock: phaselock.clone(),
        stream_output: receiver,
        pause: pause.clone(),
        run_once: run_once.clone(),
        shut_down: shut_down.clone(),
        storage: phaselock.inner.storage.clone(),
    };
    let node_id = phaselock.inner.public_key.nonce;
    let task = spawn(
        round_runner_task(sender, phaselock.clone(), pause, run_once, shut_down)
            .instrument(info_span!("PhaseLock Background Driver", id = node_id)),
    );

    (task, handle)
}

/// Continually processes the incoming broadcast messages received on `phaselock.inner.networking`, redirecting them to `phaselock.handle_broadcast_*_message`.
pub async fn network_broadcast_task<I: NodeImplementation<N>, const N: usize>(
    phaselock: PhaseLock<I, N>,
) {
    info!("Launching broadcast processing task");
    let networking = &phaselock.inner.networking;
    let mut incremental_backoff_ms = 10;
    while let Ok(queue) = networking.broadcast_queue().await {
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
            }
        }
        trace!("Items processed, querying for more");
    }
}

/// Continually processes the incoming direct messages received on `phaselock.inner.networking`, redirecting them to `phaselock.handle_direct_*_message`.
pub async fn network_direct_task<I: NodeImplementation<N>, const N: usize>(
    phaselock: PhaseLock<I, N>,
) {
    info!("Launching direct processing task");
    let networking = &phaselock.inner.networking;
    let mut incremental_backoff_ms = 10;
    while let Ok(queue) = networking.direct_queue().await {
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
            }
        }
        trace!("Items processed, querying for more");
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
/// If any error occurs, they will be send to `channel` as an [`Event`] with `EventType::Error` or `EventType::Timeout` together with the current view number and stage.
///
/// [`phaselock.next_view`]: ../struct.PhaseLock.html#method.next_view
/// [`phaselock.run_round`]: ../struct.PhaseLock.html#method.run_round
pub async fn round_runner_task<I: NodeImplementation<N>, const N: usize>(
    channel: BroadcastSender<
        Event<<I as NodeImplementation<N>>::Block, <I as NodeImplementation<N>>::State>,
    >,
    phaselock: PhaseLock<I, N>,
    pause: Arc<RwLock<bool>>,
    run_once: Arc<RwLock<bool>>,
    shut_down: Arc<RwLock<bool>>,
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

        // Send the next view
        let next_view_res = phaselock.next_view(view, Some(&channel)).await;
        // If we fail to send the next view, broadcast the error and pause
        if let Err(e) = next_view_res {
            let x = channel
                .send_async(Event {
                    view_number: view,
                    stage: e.get_stage().unwrap_or(Stage::None),

                    event: EventType::Error { error: Arc::new(e) },
                })
                .await;
            if x.is_err() {
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
        let round_res =
            async_std::future::timeout(t, phaselock.run_round(view, Some(&channel))).await;
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
                let x = channel
                    .send_async(Event {
                        view_number: view,
                        stage: e.get_stage().unwrap_or(Stage::None),
                        event: EventType::Error { error: Arc::new(e) },
                    })
                    .await;
                if x.is_err() {
                    error!("All event streams closed! Shutting down.");
                    break;
                }
                continue;
            }
            // if we timed out, log it, send the event, and increase the timeout
            Err(_) => {
                warn!("Round timed out");
                let x = channel
                    .send_async(Event {
                        view_number: view,
                        stage: Stage::None,
                        event: EventType::ViewTimeout { view_number: view },
                    })
                    .await;
                if x.is_err() {
                    error!("All event streams closed! Shutting down.");
                    break;
                }
                int_duration = (int_duration * int_mul) / int_div;
            }
        }
    }
}
