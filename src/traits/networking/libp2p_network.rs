//! Libp2p based production networkign implementation
//! This module provides a libp2p based networking implementation where each node in the
//! network forms a tcp or udp connection to a subset of other nodes in the network

use super::NetworkingMetrics;
use async_lock::RwLock;
use async_trait::async_trait;
use bimap::BiHashMap;
use bincode::Options;
use dashmap::DashSet;
use hotshot_types::data::ViewNumber;
use hotshot_types::{
    message::Message,
    traits::{
        metrics::{Metrics, NoMetrics},
        network::{
            FailedToSerializeSnafu, NetworkChange, NetworkError, NetworkingImplementation,
            TestableNetworkingImplementation, UnboundedChannelDisconnectedSnafu,
        },
        node_implementation::NodeTypes,
        signature_key::{SignatureKey, TestableSignatureKey},
    },
};
use hotshot_utils::{
    art::{async_block_on, async_sleep, async_spawn},
    bincode::bincode_opts,
    channel::{unbounded, UnboundedReceiver, UnboundedSender},
};
use libp2p_networking::{
    network::{
        MeshParams,
        NetworkEvent::{self, DirectRequest, DirectResponse, GossipMsg},
        NetworkNodeConfig, NetworkNodeConfigBuilder, NetworkNodeHandle, NetworkNodeType,
    },
    reexport::{Multiaddr, PeerId},
};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use std::{
    collections::HashSet,
    num::NonZeroUsize,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tracing::{error, info, instrument, warn};

/// hardcoded topic of QC used
pub const QC_TOPIC: &str = "global";

/// Stubbed out Ack
#[derive(Serialize)]
pub enum Empty {
    /// Empty value
    Empty,
}

impl<TYPES: NodeTypes> std::fmt::Debug for Libp2pNetwork<TYPES> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Libp2p").field("inner", &"inner").finish()
    }
}

/// Type alias for a shared collection of peerid, multiaddrs
pub type PeerInfoVec = Arc<RwLock<Vec<(Option<PeerId>, Multiaddr)>>>;

/// The underlying state of the libp2p network
struct Libp2pNetworkInner<TYPES: NodeTypes> {
    /// this node's public key
    pk: TYPES::SignatureKey,
    /// handle to control the network
    handle: Arc<NetworkNodeHandle<()>>,
    /// Bidirectional map from public key provided by espresso
    /// to public key provided by libp2p
    pubkey_pid_map: RwLock<BiHashMap<TYPES::SignatureKey, PeerId>>,
    /// map of known replica peer ids to public keys
    broadcast_recv: UnboundedReceiver<Message<TYPES>>,
    /// Sender for broadcast messages
    broadcast_send: UnboundedSender<Message<TYPES>>,
    /// Sender for direct messages (only used for sending messages back to oneself)
    direct_send: UnboundedSender<Message<TYPES>>,
    /// Receiver for direct messages
    direct_recv: UnboundedReceiver<Message<TYPES>>,
    /// this is really cheating to enable local tests
    /// hashset of (bootstrap_addr, peer_id)
    bootstrap_addrs: PeerInfoVec,
    /// bootstrap
    bootstrap_addrs_len: usize,
    /// whether or not the network is ready to send
    is_ready: Arc<AtomicBool>,
    /// set of recently seen peers
    /// TODO jr make this LRU eventually/less jank
    recently_updated_peers: DashSet<PeerId>,
    /// max time before dropping message due to DHT error
    dht_timeout: Duration,
    /// whether or not we've bootstrapped into the DHT yet
    is_bootstrapped: Arc<AtomicBool>,
    /// The networking metrics we're keeping track of
    metrics: NetworkingMetrics,
}

/// Networking implementation that uses libp2p
/// generic over `M` which is the message type
#[derive(Clone)]
pub struct Libp2pNetwork<TYPES: NodeTypes> {
    /// holds the state of the libp2p network
    inner: Arc<Libp2pNetworkInner<TYPES>>,
}

impl<TYPES: NodeTypes> TestableNetworkingImplementation<TYPES> for Libp2pNetwork<TYPES>
where
    TYPES::SignatureKey: TestableSignatureKey,
{
    /// Returns a boxed function `f(node_id, public_key) -> Libp2pNetwork`
    /// with the purpose of generating libp2p networks.
    /// Generates `num_bootstrap` bootstrap nodes. The remainder of nodes are normal
    /// nodes with sane defaults.
    /// # Panics
    /// Returned function may panic either:
    /// - An invalid configuration
    ///   (probably an issue with the defaults of this function)
    /// - An inability to spin up the replica's network
    fn generator(
        expected_node_count: usize,
        num_bootstrap: usize,
    ) -> Box<dyn Fn(u64) -> Self + 'static> {
        let bootstrap_addrs: PeerInfoVec = Arc::default();
        // NOTE uncomment this for easier debugging
        // let start_port = 5000;
        Box::new({
            move |node_id| {
                info!(
                    "GENERATOR: Node id {:?}, is bootstrap: {:?}",
                    node_id,
                    node_id < num_bootstrap as u64
                );
                let addr =
                    // Multiaddr::from_str(&format!("/ip4/127.0.0.1/tcp/0")).unwrap();
                    Multiaddr::from_str(&format!("/ip4/127.0.0.1/tcp/{}", 5000 + node_id)).unwrap();
                let privkey = TYPES::SignatureKey::generate_test_key(node_id);
                let pubkey = TYPES::SignatureKey::from_private(&privkey);
                // we want the majority of peers to have this lying around.
                let replication_factor = NonZeroUsize::new(2 * expected_node_count / 3).unwrap();
                let config = if node_id < num_bootstrap as u64 {
                    NetworkNodeConfigBuilder::default()
                        // NOTICE the implicit assumption that bootstrap is less
                        // than half the network. This seems reasonable.
                        .mesh_params(Some(MeshParams {
                            mesh_n_high: expected_node_count,
                            mesh_n_low: 5,
                            mesh_outbound_min: 3,
                            // the worst case of 7/2+3 > 5
                            mesh_n: (expected_node_count / 2 + 3),
                        }))
                        .replication_factor(replication_factor)
                        .node_type(NetworkNodeType::Bootstrap)
                        .bound_addr(Some(addr))
                        .to_connect_addrs(HashSet::default())
                        .build()
                        .unwrap()
                } else {
                    NetworkNodeConfigBuilder::default()
                        // NOTE I'm hardcoding these because this is probably the MAX
                        // parameters. If there aren't this many nodes, gossip keeps looking
                        // for more. That is fine.
                        .mesh_params(Some(MeshParams {
                            mesh_n_high: 15,
                            mesh_n_low: 5,
                            mesh_outbound_min: 4,
                            mesh_n: 8,
                        }))
                        .replication_factor(replication_factor)
                        .node_type(NetworkNodeType::Regular)
                        .bound_addr(Some(addr))
                        .to_connect_addrs(HashSet::default())
                        .build()
                        .unwrap()
                };
                let bootstrap_addrs_ref = bootstrap_addrs.clone();
                async_block_on(async move {
                    Libp2pNetwork::new(
                        NoMetrics::new(),
                        config,
                        pubkey,
                        bootstrap_addrs_ref,
                        num_bootstrap,
                        node_id as usize,
                    )
                    .await
                    .unwrap()
                })
            }
        })
    }

    fn in_flight_message_count(&self) -> Option<usize> {
        None
    }
}

impl<TYPES: NodeTypes> Libp2pNetwork<TYPES> {
    /// Returns when network is ready
    pub async fn wait_for_ready(&self) {
        loop {
            if self
                .inner
                .is_ready
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                break;
            }
            async_sleep(Duration::from_secs(1)).await;
        }
        info!("LIBP2P: IS READY GOT TRIGGERED!!");
    }

    /// Constructs new network for a node. Note that this network is unconnected.
    /// One must call `connect` in order to connect.
    /// * `config`: the configuration of the node
    /// * `pk`: public key associated with the node
    /// * `bootstrap_addrs`: rwlock containing the bootstrap addrs
    /// # Errors
    /// Returns error in the event that the underlying libp2p network
    /// is unable to create a network.
    ///
    /// # Panics
    ///
    /// This will panic if there are less than 5 bootstrap nodes
    pub async fn new(
        metrics: Box<dyn Metrics>,
        config: NetworkNodeConfig,
        pk: TYPES::SignatureKey,
        bootstrap_addrs: Arc<RwLock<Vec<(Option<PeerId>, Multiaddr)>>>,
        bootstrap_addrs_len: usize,
        id: usize,
    ) -> Result<Libp2pNetwork<TYPES>, NetworkError> {
        assert!(bootstrap_addrs_len > 4, "Need at least 5 bootstrap nodes");
        let network_handle = Arc::new(
            NetworkNodeHandle::<()>::new(config, id)
                .await
                .map_err(Into::<NetworkError>::into)?,
        );

        // Make bootstrap mappings known
        if matches!(
            network_handle.config().node_type,
            NetworkNodeType::Bootstrap
        ) {
            let addr = network_handle.listen_addr();
            let pid = network_handle.peer_id();
            let mut bs_cp = bootstrap_addrs.write().await;
            bs_cp.push((Some(pid), addr));
            drop(bs_cp);
        }

        let mut pubkey_pid_map = BiHashMap::new();

        pubkey_pid_map.insert(pk.clone(), network_handle.peer_id());

        // unbounded channels may not be the best choice (spammed?)
        // if bounded figure out a way to log dropped msgs
        let (direct_send, direct_recv) = unbounded();
        let (broadcast_send, broadcast_recv) = unbounded();

        let result = Libp2pNetwork {
            inner: Arc::new(Libp2pNetworkInner {
                handle: network_handle,
                pubkey_pid_map: RwLock::new(pubkey_pid_map),
                broadcast_recv,
                direct_send: direct_send.clone(),
                direct_recv,
                pk,
                broadcast_send: broadcast_send.clone(),
                bootstrap_addrs_len,
                bootstrap_addrs,
                is_ready: Arc::new(AtomicBool::new(false)),
                recently_updated_peers: DashSet::default(),
                dht_timeout: Duration::from_secs(30),
                is_bootstrapped: Arc::new(AtomicBool::new(false)),
                metrics: NetworkingMetrics::new(metrics),
            }),
        };

        result.spawn_event_generator(direct_send, broadcast_send);

        result.spawn_connect(id);

        Ok(result)
    }

    /// Initiates connection to the outside world
    fn spawn_connect(&self, id: usize) {
        let pk = self.inner.pk.clone();
        let bootstrap_ref = self.inner.bootstrap_addrs.clone();
        let num_bootstrap = self.inner.bootstrap_addrs_len;
        let handle = self.inner.handle.clone();
        let is_bootstrapped = self.inner.is_bootstrapped.clone();
        let node_type = self.inner.handle.config().node_type;
        async_spawn({
            let is_ready = self.inner.is_ready.clone();
            async move {
                let bs_addrs = loop {
                    let bss = bootstrap_ref.read().await;
                    let bs_addrs = bss.clone();
                    drop(bss);
                    if bs_addrs.len() >= num_bootstrap {
                        break bs_addrs;
                    }
                    info!(
                        "NODE {:?} bs addr len {:?}, number of bootstrap expected {:?}",
                        id,
                        bs_addrs.len(),
                        num_bootstrap
                    );
                };
                handle.add_known_peers(bs_addrs).await.unwrap();

                // 10 minute timeout
                let timeout_duration = Duration::from_secs(600);
                // perform connection
                error!("WAITING TO CONNECT ON NODE {:?}", id);
                handle
                    .wait_to_connect(4, id, timeout_duration)
                    .await
                    .unwrap();

                while !is_bootstrapped.load(std::sync::atomic::Ordering::Relaxed) {
                    async_sleep(Duration::from_secs(1)).await;
                }

                handle.subscribe(QC_TOPIC.to_string()).await.unwrap();

                error!(
                    "peer {:?} waiting for publishing, type: {:?}",
                    handle.peer_id(),
                    node_type
                );

                // we want our records published before
                // we begin participating in consensus
                while handle.put_record(&pk, &handle.peer_id()).await.is_err() {
                    async_sleep(Duration::from_secs(1)).await;
                }

                error!(
                    "Node {:?} is ready, type: {:?}",
                    handle.peer_id(),
                    node_type
                );

                while handle.put_record(&handle.peer_id(), &pk).await.is_err() {
                    async_sleep(Duration::from_secs(1)).await;
                }

                error!(
                    "node {:?} is barring bootstrap, type: {:?}",
                    handle.peer_id(),
                    node_type
                );

                is_ready.store(true, std::sync::atomic::Ordering::Relaxed);
                error!("STARTING CONSENSUS ON {:?}", handle.peer_id());
                Ok::<(), NetworkError>(())
            }
        });
    }

    /// make network aware of known peers
    async fn _add_known_peers(
        &self,
        known_peers: Vec<(Option<PeerId>, Multiaddr)>,
    ) -> Result<(), NetworkError> {
        self.inner
            .handle
            .add_known_peers(known_peers)
            .await
            .map_err(Into::<NetworkError>::into)
    }

    /// task to propagate messages to handlers
    /// terminates on shut down of network
    fn spawn_event_generator(
        &self,
        direct_send: UnboundedSender<Message<TYPES>>,
        broadcast_send: UnboundedSender<Message<TYPES>>,
    ) {
        let handle = self.clone();
        let is_bootstrapped = self.inner.is_bootstrapped.clone();
        async_spawn(async move {
            while let Ok(msg) = handle.inner.handle.receiver().recv().await {
                match msg {
                    GossipMsg(msg, _topic) => {
                        let result: Result<Message<TYPES>, _> = bincode_opts().deserialize(&msg);
                        if let Ok(result) = result {
                            broadcast_send
                                .send(result)
                                .await
                                .map_err(|_| NetworkError::ChannelSend)?;
                        }
                    }
                    DirectRequest(msg, _pid, chan) => {
                        let result: Result<Message<TYPES>, _> = bincode_opts()
                            .deserialize(&msg)
                            .context(FailedToSerializeSnafu);
                        if let Ok(result) = result {
                            direct_send
                                .send(result)
                                .await
                                .map_err(|_| NetworkError::ChannelSend)?;
                        }
                        if handle
                            .inner
                            .handle
                            .direct_response(chan, &Empty::Empty)
                            .await
                            .is_err()
                        {
                            error!("failed to ack!");
                        };
                    }
                    DirectResponse(msg, _) => {
                        let _result: Result<Message<TYPES>, _> = bincode_opts()
                            .deserialize(&msg)
                            .context(FailedToSerializeSnafu);
                    }
                    NetworkEvent::IsBootstrapped => {
                        is_bootstrapped.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }
            error!("Network receiever shut down!");
            Ok::<(), NetworkError>(())
        });
    }
}

#[async_trait]
impl<TYPES: NodeTypes> NetworkingImplementation<TYPES> for Libp2pNetwork<TYPES> {
    #[instrument(name = "Libp2pNetwork::ready", skip_all)]
    async fn ready(&self) -> bool {
        self.wait_for_ready().await;
        true
    }

    #[instrument(name = "Libp2pNetwork::broadcast_message", skip_all)]
    async fn broadcast_message(&self, message: Message<TYPES>) -> Result<(), NetworkError> {
        if self.inner.handle.is_killed() {
            return Err(NetworkError::ShutDown);
        }
        self.wait_for_ready().await;
        info!(
            "broadcasting msg: {:?} with nodes: {:?} connected",
            message,
            self.inner.handle.connected_pids().await
        );
        // send to self?
        self.inner
            .broadcast_send
            .send(message.clone())
            .await
            .unwrap();
        match self
            .inner
            .handle
            .gossip(QC_TOPIC.to_string(), &message)
            .await
        {
            Ok(()) => {
                self.inner.metrics.outgoing_message_count.add(1);
                Ok(())
            }
            Err(e) => {
                self.inner.metrics.message_failed_to_send.add(1);
                Err(e.into())
            }
        }
    }

    #[instrument(name = "Libp2pNetwork::message_node", skip_all)]
    async fn message_node(
        &self,
        message: Message<TYPES>,
        recipient: TYPES::SignatureKey,
    ) -> Result<(), NetworkError> {
        if self.inner.handle.is_killed() {
            return Err(NetworkError::ShutDown);
        }

        // short circuit if we're dming ourselves
        if recipient == self.inner.pk {
            // panic if we already shut down?
            self.inner.direct_send.send(message).await.unwrap();
            return Ok(());
        }

        self.wait_for_ready().await;
        // check local cache. if that fails, initiate search
        // if search fails, just error out
        // NOTE: relay may be a good way to fix this in the future .
        let pid: PeerId = if let Some(pid) = self
            .inner
            .pubkey_pid_map
            .read()
            .await
            .get_by_left(&recipient)
        {
            *pid
        } else {
            match self
                .inner
                .handle
                .get_record_timeout(&recipient, self.inner.dht_timeout)
                .await
            {
                Ok(r) => r,
                Err(_e) => {
                    self.inner.metrics.message_failed_to_send.add(1);
                    error!("Failed to message {:?} because could not find recipient peer id for pk {:?}", message, recipient);
                    return Err(NetworkError::DHTError);
                }
            }
        };

        if let Err(e) = self.inner.handle.lookup_pid(pid).await {
            self.inner.metrics.message_failed_to_send.add(1);
            return Err(e.into());
        }
        match self.inner.handle.direct_request(pid, &message).await {
            Ok(()) => {
                self.inner.metrics.outgoing_message_count.add(1);
                Ok(())
            }
            Err(e) => {
                self.inner.metrics.message_failed_to_send.add(1);
                Err(e.into())
            }
        }
    }

    #[instrument(name = "Libp2pNetwork::broadcast_queue", skip_all)]
    async fn broadcast_queue(&self) -> Result<Vec<Message<TYPES>>, NetworkError> {
        if self.inner.handle.is_killed() {
            Err(NetworkError::ShutDown)
        } else {
            let result = self
                .inner
                .broadcast_recv
                .drain_at_least_one()
                .await
                .context(UnboundedChannelDisconnectedSnafu)?;
            self.inner.metrics.incoming_message_count.add(result.len());
            Ok(result)
        }
    }

    #[instrument(name = "Libp2pNetwork::next_broadcast", skip_all)]
    async fn next_broadcast(&self) -> Result<Message<TYPES>, NetworkError> {
        if self.inner.handle.is_killed() {
            Err(NetworkError::ShutDown)
        } else {
            let result = self
                .inner
                .broadcast_recv
                .recv()
                .await
                .map_err(|_| NetworkError::ShutDown)?;
            self.inner.metrics.incoming_message_count.add(1);
            Ok(result)
        }
    }

    #[instrument(name = "Libp2pNetwork::direct_queue", skip_all)]
    async fn direct_queue(&self) -> Result<Vec<Message<TYPES>>, NetworkError> {
        if self.inner.handle.is_killed() {
            Err(NetworkError::ShutDown)
        } else {
            let result = self
                .inner
                .direct_recv
                .drain_at_least_one()
                .await
                .context(UnboundedChannelDisconnectedSnafu)?;
            self.inner.metrics.incoming_message_count.add(result.len());
            Ok(result)
        }
    }

    #[instrument(name = "Libp2pNetwork::next_direct", skip_all)]
    async fn next_direct(&self) -> Result<Message<TYPES>, NetworkError> {
        if self.inner.handle.is_killed() {
            Err(NetworkError::ShutDown)
        } else {
            let result = self
                .inner
                .direct_recv
                .recv()
                .await
                .map_err(|_| NetworkError::ShutDown)?;
            self.inner.metrics.incoming_message_count.add(1);
            Ok(result)
        }
    }

    #[instrument(name = "Libp2pNetwork::known_nodes", skip_all)]
    async fn known_nodes(&self) -> Vec<TYPES::SignatureKey> {
        self.inner
            .pubkey_pid_map
            .read()
            .await
            .left_values()
            .cloned()
            .collect()
    }

    #[instrument(name = "Libp2pNetwork::network_changes", skip_all, self.peer_id)]
    async fn network_changes(
        &self,
    ) -> Result<Vec<NetworkChange<TYPES::SignatureKey>>, NetworkError> {
        if self.inner.handle.is_killed() {
            return Err(NetworkError::ShutDown);
        }
        let mut result = vec![];

        let old_connected = self
            .inner
            .recently_updated_peers
            .clone()
            .into_iter()
            .collect();

        let cur_connected: HashSet<_> = self.inner.handle.connected_pids().await?;

        // new - old -> added peers
        let added_peers = cur_connected.difference(&old_connected);

        for pid in added_peers.clone() {
            let pk: Result<TYPES::SignatureKey, _> = self
                .inner
                .handle
                .get_record_timeout(&pid, self.inner.dht_timeout)
                .await
                .map_err(Into::<NetworkError>::into);
            if let Ok(pk) = pk {
                result.push(NetworkChange::NodeConnected(pk.clone()));
            } else {
                warn!(
                    "Couldn't get pk from DHT on peer {:?} for peer {:?}!",
                    self.inner.pk, pid
                );
            }
        }
        self.inner.recently_updated_peers.clear();
        for pid in cur_connected {
            self.inner.recently_updated_peers.insert(pid);
        }
        self.inner
            .metrics
            .gossipsub_mesh_connected
            .set(self.inner.recently_updated_peers.len());

        Ok(result)
    }

    #[instrument(name = "Libp2pNetwork::shut_down", skip_all)]
    async fn shut_down(&self) {
        if self.inner.handle.is_killed() {
            error!("Called shut down when already shut down! Noop.");
        } else {
            self.inner.handle.shutdown().await.unwrap();
        }
    }

    #[instrument(name = "Libp2pNetwork::put_record", skip_all)]
    async fn put_record(
        &self,
        key: impl Serialize + Send + Sync + 'static,
        value: impl Serialize + Send + Sync + 'static,
    ) -> Result<(), NetworkError> {
        self.wait_for_ready().await;
        self.inner
            .handle
            .put_record(&key, &value)
            .await
            .map_err(Into::<NetworkError>::into)
    }

    #[instrument(name = "Libp2pNetwork::get_record", skip_all)]
    async fn get_record<V: for<'a> Deserialize<'a>>(
        &self,
        key: impl Serialize + Send + Sync + 'static,
    ) -> Result<V, NetworkError> {
        self.wait_for_ready().await;
        self.inner
            .handle
            .get_record_timeout(&key, self.inner.dht_timeout)
            .await
            .map_err(Into::<NetworkError>::into)
    }

    async fn notify_of_subsequent_leader(
        &self,
        pk: TYPES::SignatureKey,
        is_cancelled: Arc<AtomicBool>,
    ) {
        self.wait_for_ready().await;

        if is_cancelled.load(Ordering::Relaxed) {
            return;
        }

        let maybe_pid = self.get_record::<PeerId>(pk.clone()).await;
        if is_cancelled.load(Ordering::Relaxed) {
            return;
        }

        if let Ok(pid) = maybe_pid {
            if self.inner.handle.lookup_pid(pid).await.is_err() {
                error!("Failed to look up pid");
            };
            if is_cancelled.load(Ordering::Relaxed) {
                return;
            }

            // TODO in the future we will probably want to connect too
        } else {
            error!("Unable to look up pubkey {:?} ahead of time!", pk);
        }
    }

    async fn inject_view_number(&self, view_number: ViewNumber) {
        // Do nothing
    }
}
