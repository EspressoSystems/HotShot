//! Provides a number of tasks that run continuously on a [`HotShot`]

use crate::{SystemContext, HotShotType, ViewRunner};
use async_compatibility_layer::{
    art::{async_sleep, async_spawn_local, async_timeout},
    channel::{UnboundedReceiver, UnboundedSender},
};
use async_lock::RwLock;
use hotshot_types::message::Message;
use hotshot_types::traits::election::ConsensusExchange;
use hotshot_types::{
    constants::LOOK_AHEAD,
    traits::{
        network::{CommunicationChannel, TransmitType},
        node_implementation::{ExchangesType, NodeImplementation, NodeType},
    },
};
use std::{
    collections::HashMap,
    marker::PhantomData,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tracing::{error, info, trace};

#[cfg(feature = "async-std-executor")]
use async_std::task::{yield_now, JoinHandle};
#[cfg(feature = "tokio-executor")]
use tokio::task::{yield_now, JoinHandle};
#[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
std::compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}

/// A handle with senders to send events to the background runners.
#[derive(Default)]
pub struct TaskHandle<TYPES: NodeType> {
    /// Inner struct of the [`TaskHandle`]. This is `None` by default but should be initialized
    /// early on in the [`HotShot`] struct. It should be safe to `unwrap` this.
    pub(crate) inner: RwLock<Option<TaskHandleInner>>,
    /// Reference to the [`NodeType`] used in this configuration
    _types: PhantomData<TYPES>,
}
impl<TYPES: NodeType> TaskHandle<TYPES> {
    /// Start the round runner. This will make it run until `pause` is called
    ///
    /// # Panics
    ///
    /// If the [`TaskHandle`] has not been properly initialized.
    pub async fn start(&self) {
        let handle = self.inner.read().await;
        if handle.is_some() {
            let handle = handle.as_ref().unwrap();
            handle.started.store(true, Ordering::Relaxed);
        }
    }

    /// Make the round runner run 1 round.
    /// Does/should not block.
    ///
    /// # Panics
    ///
    /// If the [`TaskHandle`] has not been properly initialized.
    pub async fn start_one_round(&self) {
        let handle = self.inner.read().await;
        if handle.is_some() {
            let handle = handle.as_ref().unwrap();
            if let Some(s) = &handle.run_view_channels {
                handle.started.store(true, Ordering::Relaxed);
                let _: Result<_, _> = s.send(()).await;
            } else {
                error!("Run one view channel not configured for this hotshot instance");
            }
        }
    }

    /// Wait until all underlying handles are shut down
    ///
    /// # Panics
    ///
    /// If the [`TaskHandle`] has not been properly initialized.
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
            (inner.consensus_task_handle, "network_change_task_handle"),
        ] {
            assert!(
                async_timeout(long_timeout, handle).await.is_ok(),
                "{name} did not shut down within a second",
            );
        }

        if let Some(committee_network_broadcast_task_handle) =
            inner.committee_network_broadcast_task_handle
        {
            assert!(
                async_timeout(long_timeout, committee_network_broadcast_task_handle)
                    .await
                    .is_ok(),
                "committee_network_broadcast_task_handle did not shut down within a second",
            );
            if let Some(committee_network_direct_task_handle) =
                inner.committee_network_direct_task_handle
            {
                assert!(
                    async_timeout(long_timeout, committee_network_direct_task_handle)
                        .await
                        .is_ok(),
                    "committee_network_direct_task_handle did not shut down within a second",
                );
            }
        }
    }
}
/// Inner struct of the [`TaskHandle`]
pub(crate) struct TaskHandleInner {
    /// for the client to indicate "increment a view"
    /// only Some in Incremental exeuction mode
    /// otherwise None
    pub run_view_channels: Option<UnboundedSender<()>>,

    /// Join handle for `network_broadcast_task`
    pub network_broadcast_task_handle: JoinHandle<()>,

    /// Join handle for `network_direct_task`
    pub network_direct_task_handle: JoinHandle<()>,

    /// Join Handle for committee broadcast network task
    pub committee_network_broadcast_task_handle: Option<JoinHandle<()>>,

    /// Join Handle for committee direct network task
    pub committee_network_direct_task_handle: Option<JoinHandle<()>>,

    /// Join handle for `consensus_task`
    pub consensus_task_handle: JoinHandle<()>,

    /// Global to signify the `HotShot` should be started
    pub(crate) started: Arc<AtomicBool>,

    /// same as hotshot's view_timeout such that
    /// there is not an accidental race between the two
    pub(crate) shutdown_timeout: Duration,
}

/// main thread driving consensus
pub async fn view_runner<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    hotshot: SystemContext<TYPES::ConsensusType, TYPES, I>,
    started: Arc<AtomicBool>,
    shut_down: Arc<AtomicBool>,
    run_once: Option<UnboundedReceiver<()>>,
) where
    SystemContext<TYPES::ConsensusType, TYPES, I>: ViewRunner<TYPES, I>,
{
    while !shut_down.load(Ordering::Relaxed) && !started.load(Ordering::Relaxed) {
        yield_now().await;
    }

    while !shut_down.load(Ordering::Relaxed) && started.load(Ordering::Relaxed) {
        if let Some(ref recv) = run_once {
            let _: Result<(), _> = recv.recv().await;
        }
        let _: Result<_, _> =
            SystemContext::<TYPES::ConsensusType, TYPES, I>::run_view(hotshot.clone()).await;
    }
}

/// Task to look up a node in the future as needed
pub async fn network_lookup_task<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    hotshot: SystemContext<TYPES::ConsensusType, TYPES, I>,
    shut_down: Arc<AtomicBool>,
) {
    info!("Launching network lookup task");
    let networking = hotshot.inner.exchanges.quorum_exchange().network().clone();
    let inner = hotshot.inner.clone();

    let mut completion_map: HashMap<TYPES::Time, Arc<AtomicBool>> = HashMap::default();

    while !shut_down.load(Ordering::Relaxed) {
        let lock = hotshot.recv_network_lookup.lock().await;

        if let Ok(Some(cur_view)) = lock.recv().await {
            // Injecting consensus data into the networking implementation
            let _result = networking
                .inject_consensus_info((
                    (*cur_view),
                    inner.exchanges.quorum_exchange().is_leader(cur_view),
                    inner.exchanges.quorum_exchange().is_leader(cur_view + 1),
                ))
                .await;

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
            if !inner.exchanges.quorum_exchange().is_leader(view_to_lookup) {
                let is_done = Arc::new(AtomicBool::new(false));
                completion_map.insert(view_to_lookup, is_done.clone());
                let inner = inner.clone();
                let networking = networking.clone();
                async_spawn_local(async move {
                    info!("starting lookup for {:?}", view_to_lookup);
                    let _result = networking
                        .lookup_node(inner.exchanges.quorum_exchange().get_leader(view_to_lookup))
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

/// Continually processes the incoming broadcast messages received on `hotshot.inner.networking`, redirecting them to their relevant handler
pub async fn network_task<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    EXCHANGE: ConsensusExchange<TYPES, Message<TYPES, I>>,
>(
    hotshot: SystemContext<TYPES::ConsensusType, TYPES, I>,
    shut_down: Arc<AtomicBool>,
    transmit_type: TransmitType,
    exchange: Arc<EXCHANGE>,
) where
    SystemContext<TYPES::ConsensusType, TYPES, I>: HotShotType<TYPES, I>,
{
    info!(
        "Launching network processing task for {:?} messages",
        transmit_type
    );
    let networking = &exchange.network();
    let mut incremental_backoff_ms = 10;
    while !shut_down.load(Ordering::Relaxed) {
        let queue = match networking.recv_msgs(transmit_type).await {
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
            let _metrics = Arc::clone(&hotshot.consensus.read().await.metrics);
            trace!(?item, "Processing item");
            hotshot.handle_message(item, transmit_type).await;
        }
        trace!(
            "Items processed in network {:?} task, querying for more",
            transmit_type
        );
    }
}
