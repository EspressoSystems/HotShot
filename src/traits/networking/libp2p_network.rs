//! Libp2p based production networkign implementation
//! This module provides a libp2p based networking implementation where each node in the
//! network forms a tcp or udp connection to a subset of other nodes in the network

use async_std::future::timeout;
use async_std::task::{sleep, spawn};
use bincode::Options;
use dashmap::DashMap;
use futures::future::{join_all, BoxFuture};
use futures::FutureExt;
use libp2p::PeerId;
use libp2p_networking::network::NetworkEvent::{DirectRequest, DirectResponse, GossipMsg};
use libp2p_networking::network::{ConnectionData, NetworkNodeConfig, NetworkNodeHandle};
use phaselock_types::{
    traits::network::{NetworkChange, NetworkError, NetworkingImplementation},
    PubKey,
};
use serde::Deserialize;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashSet;
use std::marker::PhantomData;
use std::{sync::Arc, time::Duration};
use tracing::{debug, error, info_span, trace, Instrument};

/// The underlying state of the libp2p network
struct Libp2pNetworkInner<
    M: Clone + Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + 'static,
> {
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
    pub async fn new(config: NetworkNodeConfig, idx: usize, pk: PubKey) -> Libp2pNetwork<M> {
        let bincode_options = bincode::DefaultOptions::new().with_limit(16_384);

        let timeout_duration = Duration::from_secs(5);

        // if we care about internal state, we could consider passing something in. We don't,
        // though.
        let network_handle = Arc::new(NetworkNodeHandle::<()>::new(config, idx).await.unwrap());

        // FIXME error handling. This should really return a error if timeout or failure to connect
        timeout(
            timeout_duration,
            NetworkNodeHandle::wait_to_connect(
                network_handle.clone(),
                5,
                network_handle.recv_network(),
                idx,
            ),
        )
        .await
        .unwrap()
        .unwrap();

        network_handle
            .put_record(&pk, &network_handle.peer_id())
            .await
            .unwrap();

        let pubkey_to_pid = DashMap::new();
        pubkey_to_pid.insert(pk.clone(), network_handle.peer_id());
        let pid_to_pubkey = DashMap::new();
        pid_to_pubkey.insert(network_handle.peer_id(), pk);

        let nw_recv = network_handle.recv_network();

        // unbounded channels may not be the best choice (spammed?)
        // if bounded figure out a way to log dropped msgs
        let (direct_send, direct_recv) = flume::bounded(128);
        let (broadcast_send, broadcast_recv) = flume::bounded(128);

        let result = Libp2pNetwork {
            inner: Arc::new(Libp2pNetworkInner {
                handle: network_handle,
                msg_type: PhantomData::<M>,
                pubkey_to_pid,
                pid_to_pubkey,
                broadcast_recv,
                direct_recv,
                last_connection_set: ConnectionData::default(),
            }),
        };

        // task to propagate messages to handlers
        // terminates on shut down of network
        spawn(async move {
            while let Ok(msg) = nw_recv.recv_async().await {
                match msg {
                    GossipMsg(msg) => {
                        // TODO error handling
                        let result: M = bincode_options.deserialize(&msg).unwrap();
                        direct_send.send_async(result).await.unwrap();
                    }
                    DirectRequest(msg, _pid, _) => {
                        let result: M = bincode_options.deserialize(&msg).unwrap();
                        broadcast_send.send_async(result).await.unwrap();
                    }
                    DirectResponse(_, _) => {
                        // we should never reach this part
                        unreachable!()
                    }
                }
            }
            error!("Network receiever shut down!");
        });

        let result_key_task = result.clone();

        // task to periodically look at other public keys
        // just to have knowledge of who exists
        spawn(async move {
            let timeout_dur = Duration::new(0, 500);
            while !result_key_task.inner.handle.is_killed().await {
                // get peer ids from dashmap
                // get peer ids from libp2p
                let known_nodes = result_key_task
                    .inner
                    .pubkey_to_pid
                    .iter()
                    .map(|kv| *kv.pair().1)
                    .collect::<HashSet<_>>();
                let libp2p_known_nodes = result_key_task.inner.handle.known_peers().await;
                let unknown_nodes = libp2p_known_nodes
                    .difference(&known_nodes)
                    .collect::<Vec<_>>();

                let mut futs = vec![];
                for pid in &unknown_nodes {
                    let fut = result_key_task
                        .inner
                        .handle
                        .get_record_timeout(pid, timeout_dur);
                    futs.push(fut);
                }

                let results: Vec<Result<PubKey, _>> = join_all(futs).await;

                for (idx, maybe_pk) in results.into_iter().enumerate() {
                    if let Ok(pk) = maybe_pk {
                        result_key_task
                            .inner
                            .pubkey_to_pid
                            .insert(pk.clone(), *unknown_nodes[idx]);
                        result_key_task
                            .inner
                            .pid_to_pubkey
                            .insert(*unknown_nodes[idx], pk);
                    }
                }

                // sleep then repeat
                sleep(timeout_dur).await;
            }
        });

        result
    }
}

impl<M: Clone + Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + 'static>
    NetworkingImplementation<M> for Libp2pNetwork<M>
{
    /// Broadcasts a message to the network
    ///
    /// Should provide that the message eventually reach all non-faulty nodes
    fn broadcast_message(&self, message: M) -> BoxFuture<'_, Result<(), NetworkError>> {
        async move {
            if !self.inner.handle.is_killed().await {
                return Err(NetworkError::ShutDown);
            }
            self.inner
                .handle
                .gossip("global".to_string(), &message)
                .await
                .unwrap();
            // FIXME types
            Err(NetworkError::ListenerSend)
        }
        .instrument(info_span!("Libp2pNetwork::broadcast_message"))
        .boxed()
    }

    /// Sends a direct message to a specific node
    fn message_node(
        &self,
        message: M,
        recipient: PubKey,
    ) -> BoxFuture<'_, Result<(), NetworkError>> {
        async move {
            if !self.inner.handle.is_killed().await {
                return Err(NetworkError::ShutDown);
            }
            // check local cache. if that fails, initiate search
            let pid: PeerId = if let Some(pid) = self.inner.pubkey_to_pid.get(&recipient) {
                *pid
            } else {
                self.inner.handle.get_record(&recipient).await.unwrap()
            };
            self.inner
                .handle
                .direct_request(pid, &message)
                .await
                .unwrap();
            Ok(())
        }
        .boxed()
    }

    /// Moves out the entire queue of received broadcast messages, should there be any
    ///
    /// Provided as a future to allow the backend to do async locking
    fn broadcast_queue(&self) -> BoxFuture<'_, Result<Vec<M>, NetworkError>> {
        async move {
            if !self.inner.handle.is_killed().await {
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
        .instrument(info_span!("Libp2pNetwork::broadcast_queue"))
        .boxed()
    }

    /// Provides a future for the next received broadcast
    ///
    /// Will unwrap the underlying `NetworkMessage`
    fn next_broadcast(&self) -> BoxFuture<'_, Result<M, NetworkError>> {
        async move {
            if !self.inner.handle.is_killed().await {
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
        .instrument(info_span!("Libp2pNetwork::next_broadcast"))
        .boxed()
    }

    /// Moves out the entire queue of received direct messages to this node
    fn direct_queue(&self) -> BoxFuture<'_, Result<Vec<M>, NetworkError>> {
        async move {
            if !self.inner.handle.is_killed().await {
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
        .instrument(info_span!("Libp2pNetwork::direct_queue"))
        .boxed()
    }

    /// Provides a future for the next received direct message to this node
    ///
    /// Will unwrap the underlying `NetworkMessage`
    fn next_direct(&self) -> BoxFuture<'_, Result<M, NetworkError>> {
        async move {
            if !self.inner.handle.is_killed().await {
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
        .instrument(info_span!("Libp2pNetwork::next_direct"))
        .boxed()
    }

    /// Node's currently known to the networking implementation
    ///
    /// Kludge function to work around leader election
    fn known_nodes(&self) -> BoxFuture<'_, Vec<PubKey>> {
        async move {
            self.inner
                .pubkey_to_pid
                .iter()
                .map(|kv| kv.pair().0.clone())
                .collect()
        }
        .boxed()
    }

    /// Returns a list of changes in the network that have been observed. Calling this function will clear the internal list.
    fn network_changes(&self) -> BoxFuture<'_, Result<Vec<NetworkChange>, NetworkError>> {
        async move {
            if !self.inner.handle.is_killed().await {
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
        .boxed()
    }

    /// Shut down this network. Afterwards this network should no longer be used.
    ///
    /// This should also cause other functions to immediately return with a [`NetworkError`]
    fn shut_down(&self) -> BoxFuture<'_, ()> {
        async move {
            if self.inner.handle.is_killed().await {
                self.inner.handle.shutdown().await.unwrap();
            }
        }
        .boxed()
    }

    fn put_record(
        &self,
        key: impl Serialize + Send + Sync + 'static,
        value: impl Serialize + Send + Sync + 'static,
    ) -> BoxFuture<'_, Result<(), NetworkError>> {
        async move {
            self.inner.handle.put_record(&key, &value).await.unwrap();
            Ok(())
        }
        .boxed()
    }

    fn get_record<V: for<'a> Deserialize<'a>>(
        &self,
        key: impl Serialize + Send + Sync + 'static,
    ) -> BoxFuture<'_, Result<V, NetworkError>> {
        async move {
            let result = self.inner.handle.get_record(&key).await.unwrap();
            Ok(result)
        }
        .boxed()
    }
}
