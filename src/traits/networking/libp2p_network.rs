//! Libp2p based production networkign implementation
//! This module provides a libp2p based networking implementation where each node in the
//! network forms a tcp or udp connection to a subset of other nodes in the network

use async_std::prelude::FutureExt;
use async_std::task::{sleep, spawn};
use async_trait::async_trait;
use bincode::Options;
use dashmap::DashMap;
use flume::Sender;
use futures::future::join_all;
use libp2p::PeerId;
use libp2p_networking::network::NetworkEvent::{DirectRequest, DirectResponse, GossipMsg};
use libp2p_networking::network::{ConnectionData, NetworkNodeConfig, NetworkNodeHandle};
use phaselock_types::traits::network::{FailedToSerializeSnafu, TimeoutSnafu};
use phaselock_types::{
    traits::network::{NetworkChange, NetworkError, NetworkingImplementation},
    PubKey,
};
use serde::Deserialize;
use serde::{de::DeserializeOwned, Serialize};
use snafu::ResultExt;
use std::collections::HashSet;
use std::marker::PhantomData;
use std::{sync::Arc, time::Duration};
use tracing::{debug, error, instrument, trace};

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
    /// type of the message
    msg_type: PhantomData<M>,
    /// Receiver for broadcast messages
    broadcast_recv: flume::Receiver<M>,
    /// Receiver for direct messages
    direct_recv: flume::Receiver<M>,
    /// holds the state of the previously held connections
    last_connection_set: ConnectionData,
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

impl<M: Clone + Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + 'static>
    Libp2pNetwork<M>
{
    /// FIXME is there any way to
    #[allow(dead_code)]
    pub async fn new(
        config: NetworkNodeConfig,
        idx: usize,
        pk: PubKey,
    ) -> Result<Libp2pNetwork<M>, NetworkError> {
        let timeout_duration = Duration::from_secs(5);

        // if we care about internal state, we could consider passing something in.
        // We don't, though.AFAICT
        let network_handle = Arc::new(
            NetworkNodeHandle::<()>::new(config, idx)
                .await
                .map_err(Into::<NetworkError>::into)?,
        );

        NetworkNodeHandle::wait_to_connect(
            network_handle.clone(),
            5,
            network_handle.recv_network(),
            idx,
        )
        .timeout(timeout_duration)
        .await
        .context(TimeoutSnafu)?
        .map_err(Into::<NetworkError>::into)?;

        network_handle
            .put_record(&pk, &network_handle.peer_id())
            .await
            .map_err(Into::<NetworkError>::into)?;

        let pubkey_to_pid = DashMap::new();
        pubkey_to_pid.insert(pk.clone(), network_handle.peer_id());
        let pid_to_pubkey = DashMap::new();
        pid_to_pubkey.insert(network_handle.peer_id(), pk.clone());

        // unbounded channels may not be the best choice (spammed?)
        // if bounded figure out a way to log dropped msgs
        let (direct_send, direct_recv) = flume::unbounded();
        let (broadcast_send, broadcast_recv) = flume::unbounded();

        let result = Libp2pNetwork {
            inner: Arc::new(Libp2pNetworkInner {
                handle: network_handle,
                msg_type: PhantomData::<M>,
                pubkey_to_pid,
                pid_to_pubkey,
                broadcast_recv,
                direct_recv,
                last_connection_set: ConnectionData::default(),
                pk,
            }),
        };

        result.spawn_event_generator(direct_send, broadcast_send);
        result.spawn_pk_gather();

        Ok(result)
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
                        // TODO error handling
                        let result: M = bincode_options
                            .deserialize(&msg)
                            .context(FailedToSerializeSnafu)?;
                        direct_send
                            .send_async(result)
                            .await
                            .map_err(|_| NetworkError::ChannelSend)?;
                    }
                    DirectRequest(msg, _pid, _) => {
                        let result: M = bincode_options
                            .deserialize(&msg)
                            .context(FailedToSerializeSnafu)?;
                        broadcast_send
                            .send_async(result)
                            .await
                            .map_err(|_| NetworkError::ChannelSend)?;
                    }
                    DirectResponse(_, _) => {
                        // we should never reach this part
                        unreachable!()
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
            let timeout_dur = Duration::new(0, 500);
            while !handle.inner.handle.is_killed().await {
                let known_nodes = handle
                    .inner
                    .pubkey_to_pid
                    .iter()
                    .map(|kv| *kv.pair().1)
                    .collect::<HashSet<_>>();
                let libp2p_known_nodes = handle.inner.handle.known_peers().await;
                let unknown_nodes = libp2p_known_nodes
                    .difference(&known_nodes)
                    .collect::<Vec<_>>();

                let mut futs = vec![];
                for pid in &unknown_nodes {
                    let fut = handle.inner.handle.get_record_timeout(pid, timeout_dur);
                    futs.push(fut);
                }

                let results: Vec<Result<PubKey, _>> = join_all(futs).await;

                for (idx, maybe_pk) in results.into_iter().enumerate() {
                    match maybe_pk {
                        Ok(pk) => {
                            handle
                                .inner
                                .pubkey_to_pid
                                .insert(pk.clone(), *unknown_nodes[idx]);
                            handle.inner.pid_to_pubkey.insert(*unknown_nodes[idx], pk);
                        }
                        Err(e) => {
                            // hopefully we'll eventually find the key. Try again next time.
                            error!(
                                ?e,
                                "error fetching public key for peer id {:?}", unknown_nodes[idx]
                            );
                        }
                    }
                }

                // sleep then repeat
                sleep(timeout_dur).await;
            }
        });
    }
}

#[async_trait]
impl<M: Clone + Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + 'static>
    NetworkingImplementation<M> for Libp2pNetwork<M>
{
    #[instrument(
        name="Libp2pNetwork::broadcast_messagee",
        fields(node_id = ?self.inner.pk.nonce),
        skip_all
    )]
    async fn broadcast_message(&self, message: M) -> Result<(), NetworkError> {
        if self.inner.handle.is_killed().await {
            return Err(NetworkError::ShutDown);
        }
        self.inner
            .handle
            .gossip("global".to_string(), &message)
            .await
            .map_err(Into::<NetworkError>::into)?;
        Err(NetworkError::ListenerSend)
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
        // check local cache. if that fails, initiate search
        let pid: PeerId = if let Some(pid) = self.inner.pubkey_to_pid.get(&recipient) {
            *pid
        } else {
            self.inner
                .handle
                .get_record(&recipient)
                .await
                .map_err(Into::<NetworkError>::into)?
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
        debug!("Waiting for messages to show up");
        let mut ret = Vec::new();
        // Wait for the first message to come up
        let first = self.inner.broadcast_recv.recv_async().await;
        if let Ok(first) = first {
            trace!(?first, "First message in broadcast queue found");
            ret.push(first);
            while let Ok(x) = self.inner.broadcast_recv.try_recv() {
                ret.push(x);
            }
            Ok(ret)
        } else {
            error!("The underlying MemoryNetwork has shut down");
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
        debug!("Awaiting next broadcast");
        let x = self.inner.broadcast_recv.recv_async().await;
        if let Ok(x) = x {
            trace!(?x, "Found broadcast");
            Ok(x)
        } else {
            error!("The underlying MemoryNetwork has shutdown");
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
        debug!("Waiting for messages to show up");
        let mut ret = Vec::new();
        // Wait for the first message to come up
        let first = self.inner.direct_recv.recv_async().await;
        if let Ok(first) = first {
            trace!(?first, "First message in direct queue found");
            ret.push(first);
            while let Ok(x) = self.inner.direct_recv.try_recv() {
                ret.push(x);
            }
            Ok(ret)
        } else {
            error!("The underlying MemoryNetwork has shut down");
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
        debug!("Awaiting next direct");
        let x = self.inner.direct_recv.recv_async().await;
        if let Ok(x) = x {
            trace!(?x, "Found direct");
            Ok(x)
        } else {
            error!("The underlying MemoryNetwork has shutdown");
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

        // get peer ids that are new
        let old_connected = self.inner.last_connection_set.connected_peers.clone();

        // get peer ids that are old
        let cur_connected = self.inner.handle.connected_peers().await;

        // new - old -> added peers
        let added_peers = cur_connected.difference(&old_connected);

        for pid in added_peers {
            if let Some(pk) = self.inner.pid_to_pubkey.get(pid) {
                result.push(NetworkChange::NodeConnected(pk.clone()));
            }
        }

        let removed_peers = old_connected.difference(&cur_connected);

        for pid in removed_peers {
            if let Some(pk) = self.inner.pid_to_pubkey.get(pid) {
                result.push(NetworkChange::NodeDisconnected(pk.clone()));
            }
        }
        Ok(result)
    }

    #[instrument(
        name="Libp2pNetwork::shut_down",
        fields(node_id = ?self.inner.pk.nonce),
        skip_all
    )]
    async fn shut_down(&self) {
        if !self.inner.handle.is_killed().await {
            self.inner.handle.shutdown().await.unwrap();
        } else {
            error!("Called shut down when already shut down! Noop.");
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
        self.inner
            .handle
            .get_record(&key)
            .await
            .map_err(Into::<NetworkError>::into)
    }
}
