//! Libp2p based production networkign implementation
//! This module provides a libp2p based networking implementation where each node in the
//! network forms a tcp or udp connection to a subset of other nodes in the network

use async_std::prelude::FutureExt;
use async_std::sync::RwLock;
use async_std::task::{block_on, sleep, spawn};
use async_trait::async_trait;
use bincode::Options;
use dashmap::{DashMap, DashSet};
use flume::Sender;
use futures::future::join_all;
use libp2p::{Multiaddr, PeerId};
use libp2p_networking::network::NetworkEvent::{DirectRequest, DirectResponse, GossipMsg};
use libp2p_networking::network::{
    NetworkNodeConfig, NetworkNodeConfigBuilder, NetworkNodeHandle, NetworkNodeType,
};
use phaselock_types::traits::network::{FailedToSerializeSnafu, TestableNetworkingImplementation};
use phaselock_types::{
    traits::network::{NetworkChange, NetworkError, NetworkingImplementation},
    PubKey,
};
use phaselock_utils::subscribable_rwlock::{SubscribableRwLock, ThreadedReadView};
use serde::Deserialize;
use serde::{de::DeserializeOwned, Serialize};
use snafu::ResultExt;
use std::collections::HashSet;
use std::num::NonZeroUsize;

use std::{sync::Arc, time::Duration};

use tracing::{error, info, instrument, warn};

/// Type alias for a shared collection of peerid, multiaddrs
pub type PeerInfoVec = Arc<RwLock<Vec<(Option<PeerId>, Multiaddr)>>>;

/// The underlying state of the libp2p network
struct Libp2pNetworkInner<
    M: Clone + Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + 'static,
> {
    /// this node's public key
    pk: PubKey,
    /// handle to control the network
    handle: Arc<NetworkNodeHandle<()>>,
    // FIXME ideally this is a bidirectional map
    // unfortunately a threadsafe version of this
    // does not exist for rust.
    // Perhaps worth implementing on top or forking of bimap-rs
    /// map of known replica public keys to peer id
    pubkey_to_pid: DashMap<PubKey, PeerId>,
    /// map of known replica peer ids to public keys
    pid_to_pubkey: DashMap<PeerId, PubKey>,
    /// Receiver for broadcast messages
    broadcast_recv: flume::Receiver<M>,
    /// Sender for broadcast messages
    broadcast_send: flume::Sender<M>,
    /// Receiver for direct messages
    direct_recv: flume::Receiver<M>,
    /// this is really cheating to enable local tests
    /// hashset of (bootstrap_addr, peer_id)
    bootstrap_addrs: PeerInfoVec,
    /// whether or not the network is ready to send
    is_ready: Arc<SubscribableRwLock<bool>>,
    /// listener for when network is ready
    is_ready_listener: Arc<dyn ThreadedReadView<bool>>,
    /// set of recently seen peers
    /// TODO jr make this LRU eventually/less jank
    recently_updated_peers: DashSet<PeerId>,
    /// max time before dropping message due to DHT error
    dht_timeout: Duration,
}

/// Networking implementation that uses libp2p
/// generic over `M` which is the message type
#[derive(Clone)]
pub struct Libp2pNetwork<
    M: Clone + Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + 'static,
> {
    /// holds the state of the libp2p network
    inner: Arc<Libp2pNetworkInner<M>>,
}

impl<T: Clone + Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + 'static>
    TestableNetworkingImplementation<T> for Libp2pNetwork<T>
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
    /// TODO error handling!! unwraps is bad
    fn generator(
        expected_node_count: usize,
        num_bootstrap: usize,
        sks: threshold_crypto::SecretKeySet,
    ) -> Box<dyn Fn(u64) -> Self + 'static> {
        let bootstrap_addrs: PeerInfoVec = Arc::default();
        Box::new({
            move |node_id| {
                let pubkey = PubKey::from_secret_key_set_escape_hatch(&sks, node_id);
                let replication_factor = NonZeroUsize::new(expected_node_count as usize).unwrap();
                let config = if node_id < num_bootstrap as u64 {
                    NetworkNodeConfigBuilder::default()
                        .replication_factor(replication_factor)
                        .node_type(NetworkNodeType::Bootstrap)
                        .max_num_peers(0)
                        .min_num_peers(0)
                        .build()
                        .unwrap()
                } else {
                    let min_num_peers = expected_node_count / 2;
                    let max_num_peers = expected_node_count;
                    NetworkNodeConfigBuilder::default()
                        .node_type(NetworkNodeType::Regular)
                        .min_num_peers(min_num_peers as usize)
                        .max_num_peers(max_num_peers as usize)
                        .replication_factor(replication_factor)
                        .build()
                        .unwrap()
                };
                let bootstrap_addrs_ref = bootstrap_addrs.clone();
                block_on(async move {
                    Libp2pNetwork::new(config, pubkey, bootstrap_addrs_ref)
                        .await
                        .unwrap()
                })
            }
        })
    }
}

impl<M: Clone + Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + 'static>
    Libp2pNetwork<M>
{
    /// returns when network is ready
    async fn wait_for_ready(&self) {
        let recv_chan = self.inner.is_ready_listener.subscribe().await;
        if !self.inner.is_ready_listener.cloned_async().await {
            while !recv_chan.recv_async().await.unwrap_or(true) {}
            // a oneshot
        }
    }

    /// Constructs new network for a node. Note that this network is unconnected.
    /// One must call `connect` in order to connect.
    /// * `config`: the configuration of the node
    /// * `pk`: public key associated with the node
    /// # Errors
    /// Returns error in the event that the underlying libp2p network
    /// is unable to create a network.
    pub async fn new(
        config: NetworkNodeConfig,
        pk: PubKey,
        bootstrap_addrs: Arc<RwLock<Vec<(Option<PeerId>, Multiaddr)>>>,
    ) -> Result<Libp2pNetwork<M>, NetworkError> {
        // TODO do we want to use pk.nonce? or idx? or are they the same?

        // if we care about internal state, we could consider passing something in.
        // We don't, though. AFAICT
        let network_handle = Arc::new(
            NetworkNodeHandle::<()>::new(config, pk.nonce as usize)
                .await
                .map_err(Into::<NetworkError>::into)?,
        );

        if matches!(
            network_handle.config().node_type,
            NetworkNodeType::Bootstrap
        ) {
            let addr = network_handle.listen_addr();
            let pid = network_handle.peer_id();
            bootstrap_addrs.write().await.push((Some(pid), addr));
        }

        let pubkey_to_pid = DashMap::new();
        pubkey_to_pid.insert(pk.clone(), network_handle.peer_id());
        let pid_to_pubkey = DashMap::new();
        pid_to_pubkey.insert(network_handle.peer_id(), pk.clone());

        // unbounded channels may not be the best choice (spammed?)
        // if bounded figure out a way to log dropped msgs
        let (direct_send, direct_recv) = flume::unbounded();
        let (broadcast_send, broadcast_recv) = flume::unbounded();

        let is_ready = Arc::new(SubscribableRwLock::new(false));

        let result = Libp2pNetwork {
            inner: Arc::new(Libp2pNetworkInner {
                handle: network_handle,
                pubkey_to_pid,
                pid_to_pubkey,
                broadcast_recv,
                direct_recv,
                pk,
                broadcast_send: broadcast_send.clone(),
                bootstrap_addrs,
                is_ready: is_ready.clone(),
                is_ready_listener: is_ready,
                recently_updated_peers: DashSet::default(),
                dht_timeout: Duration::from_secs(2),
            }),
        };

        result.spawn_event_generator(direct_send, broadcast_send);
        result.spawn_connect();

        Ok(result)
    }

    /// Initiates connection to the outside world
    fn spawn_connect(&self) {
        let handle = self.inner.handle.clone();
        let pk = self.inner.pk.clone();
        block_on(async move {
            let bs_addrs = self.inner.bootstrap_addrs.read().await.to_vec().clone();
            self.add_known_peers(bs_addrs.clone()).await.unwrap();
            info!("added peers! {:?}", bs_addrs);
        });
        spawn({
            let is_ready = self.inner.is_ready.clone();
            async move {
                let timeout_duration = Duration::from_secs(20);
                // perform connection
                let connected = NetworkNodeHandle::wait_to_connect(
                    handle.clone(),
                    // this is a safe lower bet on the number of nodes in the network.
                    2,
                    handle.recv_network(),
                    pk.nonce as usize,
                )
                .timeout(timeout_duration)
                .await;
                // FIXME should this be parametrized?
                // do we care?
                handle.subscribe("global".to_string()).await.unwrap();

                // these will spin indefinitely but since this is async, that's okay.
                // we want our records published before
                // we begin participating in consensus
                handle
                    .put_record(&pk, &handle.peer_id())
                    .await
                    .map_err(Into::<NetworkError>::into)?;
                handle
                    .put_record(&handle.peer_id(), &pk)
                    .await
                    .map_err(Into::<NetworkError>::into)?;
                info!("connected status for {} is {:?}", pk.nonce, connected);

                is_ready
                    .modify_async(|s| {
                        *s = true;
                    })
                    .await;
                Ok::<(), NetworkError>(())
            }
        });
        self.spawn_pk_gather();
    }

    /// make network aware of known peers
    async fn add_known_peers(
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
    fn spawn_event_generator(&self, direct_send: Sender<M>, broadcast_send: Sender<M>) {
        let handle = self.clone();
        spawn(async move {
            let bincode_options = bincode::DefaultOptions::new().with_limit(16_384);
            let nw_recv = handle.inner.handle.recv_network();
            while let Ok(msg) = nw_recv.recv_async().await {
                match msg {
                    GossipMsg(msg) => {
                        let result: Result<M, _> = bincode_options.deserialize(&msg);
                        if let Ok(result) = result {
                            broadcast_send
                                .send_async(result)
                                .await
                                .map_err(|_| NetworkError::ChannelSend)?;
                        }
                    }
                    DirectRequest(msg, _pid, _) => {
                        let result: Result<M, _> = bincode_options
                            .deserialize(&msg)
                            .context(FailedToSerializeSnafu);
                        if let Ok(result) = result {
                            direct_send
                                .send_async(result)
                                .await
                                .map_err(|_| NetworkError::ChannelSend)?;
                        }
                    }
                    DirectResponse(msg, _) => {
                        let _result: Result<M, _> = bincode_options
                            .deserialize(&msg)
                            .context(FailedToSerializeSnafu);
                    }
                }
            }
            error!("Network receiever shut down!");
            Ok::<(), NetworkError>(())
        });
    }

    /// Task to periodically look at other public keys
    /// just to have knowledge of who exists
    fn spawn_pk_gather(&self) {
        let handle = self.clone();
        spawn(async move {
            // time to sleep between getting metadata info
            // should probably implement some sort of implicit message passing to figure out how
            // many nodes in the network so we can do this on-demand
            let timeout_get_dur = Duration::new(2, 0);
            let sleep_dur = Duration::new(0, 25);
            while !handle.inner.handle.is_killed().await {
                let known_nodes = handle
                    .inner
                    .pubkey_to_pid
                    .clone()
                    .into_read_only()
                    .values()
                    .copied()
                    .collect::<HashSet<_>>();
                let libp2p_known_nodes = handle.inner.handle.known_peers().await;
                let unknown_nodes = libp2p_known_nodes
                    .difference(&known_nodes)
                    .collect::<Vec<_>>();

                let mut futs = vec![];
                for pid in &unknown_nodes {
                    let fut = handle.inner.handle.get_record_timeout(pid, timeout_get_dur);
                    futs.push(fut);
                }

                let results: Vec<Result<PubKey, _>> = join_all(futs).await;

                for (idx, maybe_pk) in results.into_iter().enumerate() {
                    match maybe_pk {
                        Ok(pk) => {
                            info!("found pk for peer id {:?}", unknown_nodes[idx]);
                            handle
                                .inner
                                .pubkey_to_pid
                                .insert(pk.clone(), *unknown_nodes[idx]);
                            handle.inner.pid_to_pubkey.insert(*unknown_nodes[idx], pk);
                        }
                        Err(_e) => {
                            // hopefully we'll eventually find the key. Try again
                        }
                    }
                }

                // sleep then repeat
                sleep(sleep_dur).await;
            }
        });
    }
}

#[async_trait]
impl<M: Clone + Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + 'static>
    NetworkingImplementation<M> for Libp2pNetwork<M>
{
    #[instrument(
        name="Libp2pNetwork::ready",
        fields(node_id = ?self.inner.pk.nonce),
        skip_all
    )]
    async fn ready(&self) -> bool {
        self.wait_for_ready().await;
        true
    }

    #[instrument(
        name="Libp2pNetwork::broadcast_message",
        fields(node_id = ?self.inner.pk.nonce),
        skip_all
    )]
    async fn broadcast_message(&self, message: M) -> Result<(), NetworkError> {
        if self.inner.handle.is_killed().await {
            return Err(NetworkError::ShutDown);
        }
        self.wait_for_ready().await;
        info!(
            "broadcasting msg: {:?} on node {:?} with nodes: {:?} connected",
            message,
            self.inner.pk.nonce,
            self.inner.handle.connected_peers().await
        );
        // send to self?
        self.inner
            .broadcast_send
            .send_async(message.clone())
            .await
            .unwrap();
        self.inner
            .handle
            .gossip("global".to_string(), &message)
            .await
            .map_err(Into::<NetworkError>::into)?;
        Ok(())
    }

    #[instrument(
        name="Libp2pNetwork::message_node",
        fields(node_id = ?self.inner.pk.nonce, recipient_id = ?recipient.nonce),
        skip_all
    )]
    async fn message_node(&self, message: M, recipient: PubKey) -> Result<(), NetworkError> {
        if self.inner.handle.is_killed().await {
            return Err(NetworkError::ShutDown);
        }
        self.wait_for_ready().await;
        // check local cache. if that fails, initiate search
        let pid: PeerId = if let Some(pid) = self.inner.pubkey_to_pid.get(&recipient) {
            *pid
        } else {
            // TODO separate out into separate function
            match self
                .inner
                .handle
                .get_record_timeout(&recipient, self.inner.dht_timeout)
                .await
                .map_err(Into::<NetworkError>::into)
            {
                Ok(r) => r,
                Err(_e) => {
                    unreachable!();
                }
            }
            // TODO insert
        };
        self.inner
            .handle
            .direct_request(pid, &message)
            .await
            .map_err(Into::<NetworkError>::into)?;
        Ok(())
    }

    #[instrument(
        name="Libp2pNetwork::broadcast_queue",
        fields(node_id = ?self.inner.pk.nonce),
        skip_all
    )]
    async fn broadcast_queue(&self) -> Result<Vec<M>, NetworkError> {
        if self.inner.handle.is_killed().await {
            return Err(NetworkError::ShutDown);
        }
        let mut ret = Vec::new();
        // Wait for the first message to come up
        let first = self.inner.broadcast_recv.recv_async().await;
        if let Ok(first) = first {
            ret.push(first);
            while let Ok(x) = self.inner.broadcast_recv.try_recv() {
                ret.push(x);
            }
            Ok(ret)
        } else {
            error!("The underlying Libp2pNetwork has shut down");
            Err(NetworkError::ShutDown)
        }
    }

    #[instrument(
        name="Libp2pNetwork::next_broadcast",
        fields(node_id = ?self.inner.pk.nonce),
        skip_all
    )]
    async fn next_broadcast(&self) -> Result<M, NetworkError> {
        if self.inner.handle.is_killed().await {
            return Err(NetworkError::ShutDown);
        }
        let x = self.inner.broadcast_recv.recv_async().await;
        if let Ok(x) = x {
            Ok(x)
        } else {
            error!("The underlying Libp2pNetwork has shutdown");
            Err(NetworkError::ShutDown)
        }
    }

    #[instrument(
        name="Libp2pNetwork::direct_queue",
        fields(node_id = ?self.inner.pk.nonce),
        skip_all
    )]
    async fn direct_queue(&self) -> Result<Vec<M>, NetworkError> {
        if self.inner.handle.is_killed().await {
            return Err(NetworkError::ShutDown);
        }
        let mut ret = Vec::new();
        // Wait for the first message to come up
        let first = self.inner.direct_recv.recv_async().await;
        if let Ok(first) = first {
            ret.push(first);
            while let Ok(x) = self.inner.direct_recv.try_recv() {
                ret.push(x);
            }
            Ok(ret)
        } else {
            error!("The underlying Libp2pNetwork has shut down");
            Err(NetworkError::ShutDown)
        }
    }

    #[instrument(
        name="Libp2pNetwork::next_direct",
        fields(node_id = ?self.inner.pk.nonce),
        skip_all
    )]
    async fn next_direct(&self) -> Result<M, NetworkError> {
        if self.inner.handle.is_killed().await {
            return Err(NetworkError::ShutDown);
        }
        let x = self.inner.direct_recv.recv_async().await;
        if let Ok(x) = x {
            Ok(x)
        } else {
            Err(NetworkError::ShutDown)
        }
    }

    #[instrument(
        name="Libp2pNetwork::known_nodes",
        fields(node_id = ?self.inner.pk.nonce),
        skip_all
    )]
    async fn known_nodes(&self) -> Vec<PubKey> {
        self.inner
            .pubkey_to_pid
            .iter()
            .map(|kv| kv.pair().0.clone())
            .collect()
    }

    #[instrument(
        name="Libp2pNetwork::network_changes",
        fields(node_id = ?self.inner.pk.nonce),
        skip_all
    )]
    async fn network_changes(&self) -> Result<Vec<NetworkChange>, NetworkError> {
        if self.inner.handle.is_killed().await {
            return Err(NetworkError::ShutDown);
        }
        let mut result = vec![];

        let old_connected = self
            .inner
            .recently_updated_peers
            .clone()
            .into_iter()
            .collect();

        let cur_connected: HashSet<_> = self.inner.handle.connected_peers().await;

        // new - old -> added peers
        let added_peers = cur_connected.difference(&old_connected);

        for pid in added_peers.clone() {
            let pk: Result<PubKey, _> = self
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

        Ok(result)
    }

    #[instrument(
        name="Libp2pNetwork::shut_down",
        fields(node_id = ?self.inner.pk.nonce),
        skip_all
    )]
    async fn shut_down(&self) {
        if self.inner.handle.is_killed().await {
            error!("Called shut down when already shut down! Noop.");
        } else {
            self.inner.handle.shutdown().await.unwrap();
        }
    }

    #[instrument(
        name="Libp2pNetwork::put_record",
        fields(node_id = ?self.inner.pk.nonce),
        skip_all
    )]
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

    #[instrument(
        name="Libp2pNetwork::get_record",
        fields(node_id = ?self.inner.pk.nonce),
        skip_all
    )]
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
}
