//! Networking Implementation that has a primary and a fallback network.  If the primary
//! Errors we will use the backup to send or receive
use std::{
    collections::{hash_map::DefaultHasher, BTreeMap, BTreeSet, HashMap},
    future::Future,
    hash::{Hash, Hasher},
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use async_compatibility_layer::{
    art::{async_sleep, async_spawn},
    channel::UnboundedSendError,
};
use async_lock::RwLock;
#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
use async_trait::async_trait;
use futures::{channel::mpsc, future::join_all, join, select, FutureExt};
use hotshot_task_impls::helpers::cancel_task;
#[cfg(feature = "hotshot-testing")]
use hotshot_types::traits::network::{
    AsyncGenerator, NetworkReliability, TestableNetworkingImplementation,
};
use hotshot_types::{
    boxed_sync,
    constants::{COMBINED_NETWORK_CACHE_SIZE, COMBINED_NETWORK_MIN_PRIMARY_FAILURES},
    data::ViewNumber,
    traits::{
        network::{BroadcastDelay, ConnectedNetwork, ResponseChannel},
        node_implementation::NodeType,
    },
    BoxSyncFuture,
};
use lru::LruCache;
#[cfg(async_executor_impl = "tokio")]
use tokio::task::JoinHandle;
use tracing::{info, warn};

use super::{push_cdn_network::PushCdnNetwork, NetworkError};
use crate::traits::implementations::Libp2pNetwork;

/// Helper function to calculate a hash of a type that implements Hash
pub fn calculate_hash_of<T: Hash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

/// thread-safe ref counted lock to a map of delayed tasks
type DelayedTasksLockedMap = Arc<RwLock<BTreeMap<u64, Vec<JoinHandle<Result<(), NetworkError>>>>>>;

/// A communication channel with 2 networks, where we can fall back to the slower network if the
/// primary fails
#[derive(Clone)]
pub struct CombinedNetworks<TYPES: NodeType> {
    /// The two networks we'll use for send/recv
    networks: Arc<UnderlyingCombinedNetworks<TYPES>>,

    /// Last n seen messages to prevent processing duplicates
    message_cache: Arc<RwLock<LruCache<u64, ()>>>,

    /// How many times primary failed to deliver
    primary_fail_counter: Arc<AtomicU64>,

    /// Whether primary is considered down
    primary_down: Arc<AtomicBool>,

    /// delayed, cancelable tasks for secondary network
    delayed_tasks: DelayedTasksLockedMap,

    /// how long to delay
    delay_duration: Arc<RwLock<Duration>>,
}

impl<TYPES: NodeType> CombinedNetworks<TYPES> {
    /// Constructor
    ///
    /// # Panics
    ///
    /// Panics if `COMBINED_NETWORK_CACHE_SIZE` is 0
    #[must_use]
    pub fn new(
        primary_network: PushCdnNetwork<TYPES>,
        secondary_network: Libp2pNetwork<TYPES::SignatureKey>,
        delay_duration: Duration,
    ) -> Self {
        // Create networks from the ones passed in
        let networks = Arc::from(UnderlyingCombinedNetworks(
            primary_network,
            secondary_network,
        ));

        Self {
            networks,
            message_cache: Arc::new(RwLock::new(LruCache::new(
                NonZeroUsize::new(COMBINED_NETWORK_CACHE_SIZE).unwrap(),
            ))),
            primary_fail_counter: Arc::new(AtomicU64::new(0)),
            primary_down: Arc::new(AtomicBool::new(false)),
            delayed_tasks: Arc::default(),
            delay_duration: Arc::new(RwLock::new(delay_duration)),
        }
    }

    /// Get a ref to the primary network
    #[must_use]
    pub fn primary(&self) -> &PushCdnNetwork<TYPES> {
        &self.networks.0
    }

    /// Get a ref to the backup network
    #[must_use]
    pub fn secondary(&self) -> &Libp2pNetwork<TYPES::SignatureKey> {
        &self.networks.1
    }

    /// a helper function to send messages through both networks (possibly delayed)
    async fn send_both_networks(
        &self,
        _message: Vec<u8>,
        primary_future: impl Future<Output = Result<(), NetworkError>> + Send + 'static,
        secondary_future: impl Future<Output = Result<(), NetworkError>> + Send + 'static,
        broadcast_delay: BroadcastDelay,
    ) -> Result<(), NetworkError> {
        // Check if primary is down
        let mut primary_failed = false;
        if self.primary_down.load(Ordering::Relaxed) {
            primary_failed = true;
        } else if self.primary_fail_counter.load(Ordering::Relaxed)
            > COMBINED_NETWORK_MIN_PRIMARY_FAILURES
        {
            warn!(
                "Primary failed more than {} times and is considered down now",
                COMBINED_NETWORK_MIN_PRIMARY_FAILURES
            );
            self.primary_down.store(true, Ordering::Relaxed);
            primary_failed = true;
        }

        // always send on the primary network
        if let Err(e) = primary_future.await {
            warn!("Error on primary network: {}", e);
            self.primary_fail_counter.fetch_add(1, Ordering::Relaxed);
            primary_failed = true;
        };

        if let (BroadcastDelay::View(view), false) = (broadcast_delay, primary_failed) {
            let duration = *self.delay_duration.read().await;
            let primary_fail_counter = Arc::clone(&self.primary_fail_counter);
            self.delayed_tasks
                .write()
                .await
                .entry(view)
                .or_default()
                .push(async_spawn(async move {
                    async_sleep(duration).await;
                    info!("Sending on secondary after delay, message possibly has not reached recipient on primary");
                    primary_fail_counter.fetch_add(1, Ordering::Relaxed);
                    secondary_future.await
                }));
            Ok(())
        } else {
            secondary_future.await
        }
    }
}

/// Wrapper for the tuple of `PushCdnNetwork` and `Libp2pNetwork`
/// We need this so we can impl `TestableNetworkingImplementation`
/// on the tuple
#[derive(Clone)]
pub struct UnderlyingCombinedNetworks<TYPES: NodeType>(
    pub PushCdnNetwork<TYPES>,
    pub Libp2pNetwork<TYPES::SignatureKey>,
);

#[cfg(feature = "hotshot-testing")]
impl<TYPES: NodeType> TestableNetworkingImplementation<TYPES> for CombinedNetworks<TYPES> {
    fn generator(
        expected_node_count: usize,
        num_bootstrap: usize,
        network_id: usize,
        da_committee_size: usize,
        is_da: bool,
        reliability_config: Option<Box<dyn NetworkReliability>>,
        secondary_network_delay: Duration,
    ) -> AsyncGenerator<(Arc<Self>, Arc<Self>)> {
        let generators = (
            <PushCdnNetwork<TYPES> as TestableNetworkingImplementation<TYPES>>::generator(
                expected_node_count,
                num_bootstrap,
                network_id,
                da_committee_size,
                is_da,
                None,
                Duration::default(),
            ),
            <Libp2pNetwork<TYPES::SignatureKey> as TestableNetworkingImplementation<TYPES>>::generator(
                expected_node_count,
                num_bootstrap,
                network_id,
                da_committee_size,
                is_da,
                reliability_config,
                Duration::default(),
            )
        );
        Box::pin(move |node_id| {
            let gen0 = generators.0(node_id);
            let gen1 = generators.1(node_id);

            Box::pin(async move {
                let (cdn, _) = gen0.await;
                let cdn = Arc::<PushCdnNetwork<TYPES>>::into_inner(cdn).unwrap();

                let (quorum_p2p, da_p2p) = gen1.await;
                let da_networks = UnderlyingCombinedNetworks(
                    cdn.clone(),
                    Arc::<Libp2pNetwork<TYPES::SignatureKey>>::unwrap_or_clone(da_p2p),
                );
                let quorum_networks = UnderlyingCombinedNetworks(
                    cdn,
                    Arc::<Libp2pNetwork<TYPES::SignatureKey>>::unwrap_or_clone(quorum_p2p),
                );

                // We want to  the message cache between the two networks
                let message_cache = Arc::new(RwLock::new(LruCache::new(
                    NonZeroUsize::new(COMBINED_NETWORK_CACHE_SIZE).unwrap(),
                )));

                // Create the quorum and da networks
                let quorum_net = Self {
                    networks: Arc::new(quorum_networks),
                    primary_fail_counter: Arc::new(AtomicU64::new(0)),
                    primary_down: Arc::new(AtomicBool::new(false)),
                    message_cache: Arc::clone(&message_cache),
                    delayed_tasks: Arc::default(),
                    delay_duration: Arc::new(RwLock::new(secondary_network_delay)),
                };
                let da_net = Self {
                    networks: Arc::new(da_networks),
                    message_cache,
                    primary_fail_counter: Arc::new(AtomicU64::new(0)),
                    primary_down: Arc::new(AtomicBool::new(false)),
                    delayed_tasks: Arc::default(),
                    delay_duration: Arc::new(RwLock::new(secondary_network_delay)),
                };
                (quorum_net.into(), da_net.into())
            })
        })
    }

    /// Get the number of messages in-flight.
    ///
    /// Some implementations will not be able to tell how many messages there are in-flight. These implementations should return `None`.
    fn in_flight_message_count(&self) -> Option<usize> {
        None
    }
}

#[async_trait]
impl<TYPES: NodeType> ConnectedNetwork<TYPES::SignatureKey> for CombinedNetworks<TYPES> {
    async fn request_data<T: NodeType>(
        &self,
        request: Vec<u8>,
        recipient: &TYPES::SignatureKey,
    ) -> Result<Vec<u8>, NetworkError> {
        self.secondary()
            .request_data::<TYPES>(request, recipient)
            .await
    }

    async fn spawn_request_receiver_task(
        &self,
    ) -> Option<mpsc::Receiver<(Vec<u8>, ResponseChannel<Vec<u8>>)>> {
        self.secondary().spawn_request_receiver_task().await
    }

    fn pause(&self) {
        self.networks.0.pause();
    }

    fn resume(&self) {
        self.networks.0.resume();
    }

    async fn wait_for_ready(&self) {
        join!(
            self.primary().wait_for_ready(),
            self.secondary().wait_for_ready()
        );
    }

    fn shut_down<'a, 'b>(&'a self) -> BoxSyncFuture<'b, ()>
    where
        'a: 'b,
        Self: 'b,
    {
        let closure = async move {
            join!(self.primary().shut_down(), self.secondary().shut_down());
        };
        boxed_sync(closure)
    }

    async fn broadcast_message(
        &self,
        message: Vec<u8>,
        recipients: BTreeSet<TYPES::SignatureKey>,
        broadcast_delay: BroadcastDelay,
    ) -> Result<(), NetworkError> {
        let primary = self.primary().clone();
        let secondary = self.secondary().clone();
        let primary_message = message.clone();
        let secondary_message = message.clone();
        let primary_recipients = recipients.clone();
        self.send_both_networks(
            message,
            async move {
                primary
                    .broadcast_message(primary_message, primary_recipients, BroadcastDelay::None)
                    .await
            },
            async move {
                secondary
                    .broadcast_message(secondary_message, recipients, BroadcastDelay::None)
                    .await
            },
            broadcast_delay,
        )
        .await
    }

    async fn da_broadcast_message(
        &self,
        message: Vec<u8>,
        recipients: BTreeSet<TYPES::SignatureKey>,
        broadcast_delay: BroadcastDelay,
    ) -> Result<(), NetworkError> {
        let primary = self.primary().clone();
        let secondary = self.secondary().clone();
        let primary_message = message.clone();
        let secondary_message = message.clone();
        let primary_recipients = recipients.clone();
        self.send_both_networks(
            message,
            async move {
                primary
                    .da_broadcast_message(primary_message, primary_recipients, BroadcastDelay::None)
                    .await
            },
            async move {
                secondary
                    .da_broadcast_message(secondary_message, recipients, BroadcastDelay::None)
                    .await
            },
            broadcast_delay,
        )
        .await
    }

    async fn direct_message(
        &self,
        message: Vec<u8>,
        recipient: TYPES::SignatureKey,
    ) -> Result<(), NetworkError> {
        let primary = self.primary().clone();
        let secondary = self.secondary().clone();
        let primary_message = message.clone();
        let secondary_message = message.clone();
        let primary_recipient = recipient.clone();
        self.send_both_networks(
            message,
            async move {
                primary
                    .direct_message(primary_message, primary_recipient)
                    .await
            },
            async move { secondary.direct_message(secondary_message, recipient).await },
            BroadcastDelay::None,
        )
        .await
    }

    async fn vid_broadcast_message(
        &self,
        messages: HashMap<TYPES::SignatureKey, Vec<u8>>,
    ) -> Result<(), NetworkError> {
        self.networks.0.vid_broadcast_message(messages).await
    }

    /// Receive one or many messages from the underlying network.
    ///
    /// # Errors
    /// Does not error
    async fn recv_msgs(&self) -> Result<Vec<Vec<u8>>, NetworkError> {
        // recv on both networks because nodes may be accessible only on either. discard duplicates
        // TODO: improve this algorithm: https://github.com/EspressoSystems/HotShot/issues/2089
        let mut primary_fut = self.primary().recv_msgs().fuse();
        let mut secondary_fut = self.secondary().recv_msgs().fuse();

        let msgs = select! {
            p = primary_fut => p?,
            s = secondary_fut => s?,
        };

        let mut filtered_msgs = Vec::with_capacity(msgs.len());

        // For each message,
        for msg in msgs {
            // Calculate hash of the message
            let message_hash = calculate_hash_of(&msg);

            // Add the hash to the cache
            if !self.message_cache.read().await.contains(&message_hash) {
                // If the message is not in the cache, process it
                filtered_msgs.push(msg.clone());

                // Add it to the cache
                self.message_cache.write().await.put(message_hash, ());
            }
        }

        Ok(filtered_msgs)
    }

    async fn queue_node_lookup(
        &self,
        view_number: ViewNumber,
        pk: TYPES::SignatureKey,
    ) -> Result<(), UnboundedSendError<Option<(ViewNumber, TYPES::SignatureKey)>>> {
        self.primary()
            .queue_node_lookup(view_number, pk.clone())
            .await?;
        self.secondary().queue_node_lookup(view_number, pk).await
    }

    async fn update_view<'a, T>(&'a self, view: u64, membership: &T::Membership)
    where
        T: NodeType<SignatureKey = TYPES::SignatureKey> + 'a,
    {
        let delayed_map = Arc::clone(&self.delayed_tasks);
        async_spawn(async move {
            let mut cancel_tasks = Vec::new();
            {
                let mut map_lock = delayed_map.write().await;
                while let Some((first_view, _tasks)) = map_lock.first_key_value() {
                    if *first_view < view {
                        if let Some((_view, tasks)) = map_lock.pop_first() {
                            let mut ctasks = tasks.into_iter().map(cancel_task).collect();
                            cancel_tasks.append(&mut ctasks);
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            join_all(cancel_tasks).await;
        });

        // View changed, let's start primary again
        self.primary_down.store(false, Ordering::Relaxed);
        self.primary_fail_counter.store(0, Ordering::Relaxed);

        // Run `update_view` logic for the libp2p network
        self.networks.1.update_view::<T>(view, membership).await;
    }

    fn is_primary_down(&self) -> bool {
        self.primary_down.load(Ordering::Relaxed)
    }
}
