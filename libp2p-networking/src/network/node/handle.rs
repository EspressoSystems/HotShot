use crate::network::{
    behaviours::direct_message_codec::DirectMessageResponse, error::DHTError, gen_multiaddr,
    ClientRequest, NetworkError, NetworkEvent, NetworkNode, NetworkNodeConfig,
    NetworkNodeConfigBuilderError,
};
use async_std::{
    future::TimeoutError,
    prelude::FutureExt,
    sync::{Condvar, Mutex},
    task::sleep,
};
use bincode::Options;
use flume::{bounded, Receiver, SendError, Sender};
use futures::{stream::FuturesOrdered, Future};
use hotshot_types::traits::network::NetworkError as HotShotNetworkError;
use hotshot_utils::{bincode::bincode_opts, subscribable_mutex::SubscribableMutex};
use libp2p::{request_response::ResponseChannel, Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};
use std::{collections::HashSet, fmt::Debug, sync::Arc, time::Duration};
use tracing::{error, info, instrument};

/// A handle containing:
/// - A reference to the state
/// - Controls for the swarm
#[derive(Debug)]
pub struct NetworkNodeHandle<S> {
    /// network configuration
    network_config: NetworkNodeConfig,
    /// notifies that a state change has occurred
    state_changed: Condvar,
    /// the state of the replica
    state: Arc<SubscribableMutex<S>>,
    /// send an action to the networkbehaviour
    send_network: Sender<ClientRequest>,
    /// receive an action from the networkbehaviour
    recv_network: Receiver<NetworkEvent>,
    /// whether or not the handle has been killed
    killed: Arc<Mutex<bool>>,
    /// kill the event handler for events from the swarm
    kill_switch: Sender<()>,
    /// receiving end of `kill_switch`
    recv_kill: Receiver<()>,
    /// the local address we're listening on
    listen_addr: Multiaddr,
    /// the peer id of the networkbehaviour
    peer_id: PeerId,
    /// human readable id
    id: usize,

    /// A list of webui listeners that are listening for changes on this node
    webui_listeners: Arc<Mutex<Vec<Sender<()>>>>,
}

impl<S: Default + Debug> NetworkNodeHandle<S> {
    /// constructs a new node listening on `known_addr`
    #[instrument]
    pub async fn new(config: NetworkNodeConfig, id: usize) -> Result<Self, NetworkNodeHandleError> {
        //`randomly assigned port
        let listen_addr = config
            .bound_addr
            .clone()
            .unwrap_or_else(|| gen_multiaddr(0));
        let mut network = NetworkNode::new(config.clone())
            .await
            .context(NetworkSnafu)?;

        let peer_id = network.peer_id();
        let listen_addr = network
            .start_listen(listen_addr)
            .await
            .context(NetworkSnafu)?;
        info!("LISTEN ADDRESS IS {:?}", listen_addr);
        let (send_chan, recv_chan) = network.spawn_listeners().await.context(NetworkSnafu)?;
        let (kill_switch, recv_kill) = flume::bounded(1);

        Ok(NetworkNodeHandle {
            network_config: config,
            state_changed: Condvar::new(),
            state: std::sync::Arc::default(),
            send_network: send_chan,
            recv_network: recv_chan,
            killed: Arc::new(Mutex::new(false)),
            kill_switch,
            recv_kill,
            listen_addr,
            peer_id,
            id,
            webui_listeners: Arc::default(),
        })
    }

    /// Cleanly shuts down a swarm node
    /// This is done by sending a message to
    /// the swarm event handler to stop handling events
    /// and a message to the swarm itself to spin down
    #[instrument]
    pub async fn shutdown(&self) -> Result<(), NetworkNodeHandleError> {
        self.send_network
            .send_async(ClientRequest::Shutdown)
            .await
            .map_err(|_| NetworkNodeHandleError::SendError)?;
        // if this fails, the thread has already been killed.
        self.kill_switch
            .send_async(())
            .await
            .context(CantKillTwiceSnafu)?;
        Ok(())
    }

    /// Wait for a node to connect to other nodes
    /// * `node`: reference to the node
    /// * `num_peers`: number of peers required to be connected successfully before returning
    /// * `chan`: listener for connection events
    /// * `node_idx`: the node id
    #[instrument]
    pub async fn wait_to_connect(
        node: Arc<NetworkNodeHandle<S>>,
        num_peers: usize,
        chan: Receiver<NetworkEvent>,
        node_idx: usize,
    ) -> Result<(), NetworkNodeHandleError> {
        let mut connected_ok = false;
        while !connected_ok {
            sleep(Duration::from_secs(1)).await;
            let num_connected = node.num_connected().await.unwrap();
            error!(
                "WAITING TO CONNECT, conencted to {:?} peers ON NODE {:?}",
                num_connected, node_idx
            );
            error!("connected to {:?}", node.connected_pids().await);
            connected_ok = num_connected > num_peers;
        }
        Ok(())
    }

    /// Get a reference to the network node handle's listen addr.
    pub fn listen_addr(&self) -> Multiaddr {
        self.listen_addr.clone()
    }
}

impl<S> NetworkNodeHandle<S> {
    /// Print out the routing table used by kademlia
    /// NOTE: only for debugging purposes currently
    /// # Errors
    /// if the client has stopped listening for a response
    pub async fn print_routing_table(&self) -> Result<(), NetworkNodeHandleError> {
        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::GetRoutingTable(s);
        self.send_network
            .send_async(req)
            .await
            .map_err(|_| NetworkNodeHandleError::SendError)?;
        r.await.map_err(|_| NetworkNodeHandleError::RecvError)
    }

    /// Look up a peer's addresses in kademlia
    /// NOTE: this should always be called before any `request_response` is initiated
    /// # Errors
    /// if the client has stopped listening for a response
    pub async fn lookup_pid(&self, peer_id: PeerId) -> Result<(), NetworkNodeHandleError> {
        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::LookupPeer(peer_id, s);
        self.send_network
            .send_async(req)
            .await
            .map_err(|_| NetworkNodeHandleError::SendError)?;
        r.await.map_err(|_| NetworkNodeHandleError::RecvError)
    }

    /// Insert a record into the kademlia DHT
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::DHTError`] when encountering an error putting to DHT
    /// - Will return [`NetworkNodeHandleError::SerializationError`] when unable to serialize the key or value
    pub async fn put_record(
        &self,
        key: &impl Serialize,
        value: &impl Serialize,
    ) -> Result<(), NetworkNodeHandleError> {
        use crate::network::error::CancelledRequestSnafu;

        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::PutDHT {
            key: bincode_opts().serialize(key).context(SerializationSnafu)?,
            value: bincode_opts()
                .serialize(value)
                .context(SerializationSnafu)?,
            notify: s,
        };

        self.send_request(req).await?;

        r.await.context(CancelledRequestSnafu).context(DHTSnafu)
    }

    /// Receive a record from the kademlia DHT if it exists.
    /// Must be replicated on at least 2 nodes
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::DHTError`] when encountering an error putting to DHT
    /// - Will return [`NetworkNodeHandleError::SerializationError`] when unable to serialize the key
    /// - Will return [`NetworkNodeHandleError::DeserializationError`] when unable to deserialize the returned value
    pub async fn get_record<V: for<'a> Deserialize<'a>>(
        &self,
        key: &impl Serialize,
    ) -> Result<V, NetworkNodeHandleError> {
        use crate::network::error::CancelledRequestSnafu;

        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::GetDHT {
            key: bincode_opts().serialize(key).context(SerializationSnafu)?,
            notify: s,
        };
        self.send_request(req).await?;

        match r.await.context(CancelledRequestSnafu) {
            Ok(result) => bincode_opts()
                .deserialize(&*result)
                .context(DeserializationSnafu),
            Err(e) => Err(e).context(DHTSnafu),
        }
    }

    /// Get a record from the kademlia DHT with a timeout
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::DHTError`] when encountering an error putting to DHT
    /// - Will return [`NetworkNodeHandleError::TimeoutError`] when times out
    /// - Will return [`NetworkNodeHandleError::SerializationError`] when unable to serialize the key
    /// - Will return [`NetworkNodeHandleError::DeserializationError`] when unable to deserialize the returned value
    pub async fn get_record_timeout<V: for<'a> Deserialize<'a>>(
        &self,
        key: &impl Serialize,
        timeout: Duration,
    ) -> Result<V, NetworkNodeHandleError> {
        let result = self.get_record(key).timeout(timeout).await;
        match result {
            Err(e) => Err(e).context(TimeoutSnafu),
            Ok(r) => r,
        }
    }

    /// Insert a record into the kademlia DHT with a timeout
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::DHTError`] when encountering an error putting to DHT
    /// - Will return [`NetworkNodeHandleError::TimeoutError`] when times out
    /// - Will return [`NetworkNodeHandleError::SerializationError`] when unable to serialize the key or value
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    pub async fn put_record_timeout(
        &self,
        key: &impl Serialize,
        value: &impl Serialize,
        timeout: Duration,
    ) -> Result<(), NetworkNodeHandleError> {
        let result = self.put_record(key, value).timeout(timeout).await;
        match result {
            Err(e) => Err(e).context(TimeoutSnafu),
            Ok(r) => r,
        }
    }

    /// Notify the webui that either the `state` or `connection_state` has changed.
    ///
    /// If the webui is not started, this will do nothing.
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    pub async fn notify_webui(&self) {
        let mut lock = self.webui_listeners.lock().await;
        // Keep a list of indexes that are unable to send the update
        let mut indexes_to_remove = Vec::new();
        for (idx, sender) in lock.iter().enumerate() {
            if sender.send_async(()).await.is_err() {
                indexes_to_remove.push(idx);
            }
        }
        // Make sure to remove the indexes in reverse other, else removing an index will invalidate the following indexes.
        for idx in indexes_to_remove.into_iter().rev() {
            lock.remove(idx);
        }
    }

    /// Subscribe to a topic
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    pub async fn subscribe(&self, topic: String) -> Result<(), NetworkNodeHandleError> {
        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::Subscribe(topic, Some(s));
        self.send_network
            .send_async(req)
            .await
            .map_err(|_| NetworkNodeHandleError::SendError)?;
        r.await.map_err(|_| NetworkNodeHandleError::RecvError)
    }

    /// Unsubscribe from a topic
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    pub async fn unsubscribe(&self, topic: String) -> Result<(), NetworkNodeHandleError> {
        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::Unsubscribe(topic, s);
        self.send_network
            .send_async(req)
            .await
            .map_err(|_| NetworkNodeHandleError::SendError)?;
        r.await.map_err(|_| NetworkNodeHandleError::RecvError)
    }

    /// Ignore `peers` when pruning
    /// e.g. maintain their connection
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    pub async fn ignore_peers(&self, peers: Vec<PeerId>) -> Result<(), NetworkNodeHandleError> {
        let req = ClientRequest::IgnorePeers(peers);
        self.send_network
            .send_async(req)
            .await
            .map_err(|_| NetworkNodeHandleError::SendError)
    }

    /// Make a direct request to `peer_id` containing `msg`
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    /// - Will return [`NetworkNodeHandleError::SerializationError`] when unable to serialize `msg`
    pub async fn direct_request(
        &self,
        peer_id: PeerId,
        msg: &impl Serialize,
    ) -> Result<(), NetworkNodeHandleError> {
        let serialized_msg = bincode_opts().serialize(msg).context(SerializationSnafu)?;
        let req = ClientRequest::DirectRequest(peer_id, serialized_msg);
        self.send_network
            .send_async(req)
            .await
            .map_err(|_| NetworkNodeHandleError::SendError)
    }

    /// Reply with `msg` to a request over `chan`
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    /// - Will return [`NetworkNodeHandleError::SerializationError`] when unable to serialize `msg`
    pub async fn direct_response(
        &self,
        chan: ResponseChannel<DirectMessageResponse>,
        msg: &impl Serialize,
    ) -> Result<(), NetworkNodeHandleError> {
        let serialized_msg = bincode_opts().serialize(msg).context(SerializationSnafu)?;
        let req = ClientRequest::DirectResponse(chan, serialized_msg);
        self.send_network
            .send_async(req)
            .await
            .map_err(|_| NetworkNodeHandleError::SendError)
    }

    /// Forcefully disconnet from a peer
    /// # Errors
    /// If the channel is closed somehow
    /// Shouldnt' happen.
    /// # Panics
    /// If channel errors out
    /// shouldn't happen.
    pub async fn prune_peer(&self, pid: PeerId) -> Result<(), NetworkNodeHandleError> {
        let req = ClientRequest::Prune(pid);
        self.send_network
            .send_async(req)
            .await
            .map_err(|_| NetworkNodeHandleError::SendError)
    }

    /// Gossip a message to peers
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    /// - Will return [`NetworkNodeHandleError::SerializationError`] when unable to serialize `msg`
    pub async fn gossip(
        &self,
        topic: String,
        msg: &impl Serialize,
    ) -> Result<(), NetworkNodeHandleError> {
        let serialized_msg = bincode_opts().serialize(msg).context(SerializationSnafu)?;
        let req = ClientRequest::GossipMsg(topic, serialized_msg);
        self.send_network
            .send_async(req)
            .await
            .map_err(|_| NetworkNodeHandleError::SendError)
    }

    /// Tell libp2p about known network nodes
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    pub async fn add_known_peers(
        &self,
        known_peers: Vec<(Option<PeerId>, Multiaddr)>,
    ) -> Result<(), NetworkNodeHandleError> {
        info!("ADDING KNOWN PEERS TO {:?}", self.peer_id);
        let req = ClientRequest::AddKnownPeers(known_peers);
        self.send_network
            .send_async(req)
            .await
            .map_err(|_| NetworkNodeHandleError::SendError)
    }

    /// Get a clone of the internal `killed` receiver
    pub fn recv_kill(&self) -> Receiver<()> {
        self.recv_kill.clone()
    }

    /// Get a clone of the internal network receiver
    pub fn recv_network(&self) -> Receiver<NetworkEvent> {
        self.recv_network.clone()
    }

    /// Mark this network as killed
    pub async fn mark_killed(&self) {
        *self.killed.lock().await = true;
    }

    /// Send a client request to the network
    ///
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    async fn send_request(&self, req: ClientRequest) -> Result<(), NetworkNodeHandleError> {
        self.send_network
            .send_async(req)
            .await
            .map_err(|_| NetworkNodeHandleError::SendError)?;
        Ok(())
    }

    /// Returns number of peers this node is connected to
    /// # Errors
    /// If the channel is closed somehow
    /// Shouldnt' happen.
    /// # Panics
    /// If channel errors out
    /// shouldn't happen.
    pub async fn num_connected(&self) -> Result<usize, NetworkNodeHandleError> {
        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::GetConnectedPeerNum(s);
        self.send_request(req).await?;
        Ok(r.await.unwrap())
    }

    /// return hashset of PIDs this node is connected to
    /// # Errors
    /// If the channel is closed somehow
    /// Shouldnt' happen.
    /// # Panics
    /// If channel errors out
    /// shouldn't happen.
    pub async fn connected_pids(&self) -> Result<HashSet<PeerId>, NetworkNodeHandleError> {
        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::GetConnectedPeers(s);
        self.send_request(req).await?;
        Ok(r.await.unwrap())
    }

    /// Get a reference to the network node handle's id.
    pub fn id(&self) -> usize {
        self.id
    }

    /// Get a reference to the network node handle's peer id.
    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    /// Return a reference to the network config
    pub fn config(&self) -> &NetworkNodeConfig {
        &self.network_config
    }

    /// Modify the state. This will automatically call `state_changed` and `notify_webui`
    pub async fn modify_state<F>(&self, cb: F)
    where
        F: FnMut(&mut S),
    {
        self.state.modify(cb).await;
    }

    /// Get a reference to the internal Condvar. This will be triggered whenever a different task calls `modify_state`
    pub fn state_changed(&self) -> &Condvar {
        &self.state_changed
    }

    /// Returns `true` if the network state is killed
    pub async fn is_killed(&self) -> bool {
        *self.killed.lock().await
    }

    /// Register a webui listener
    pub async fn register_webui_listener(&self) -> Receiver<()> {
        let (sender, receiver) = bounded(100);
        let mut lock = self.webui_listeners.lock().await;
        lock.push(sender);
        receiver
    }

    /// Call `wait_timeout_until` on the state's [`SubscribableMutex`]
    /// # Errors
    /// Will throw a [`NetworkNodeHandleError::TimeoutError`] error upon timeout
    pub async fn state_wait_timeout_until<F>(
        &self,
        timeout: Duration,
        f: F,
    ) -> Result<(), NetworkNodeHandleError>
    where
        F: FnMut(&S) -> bool,
    {
        self.state
            .wait_timeout_until(timeout, f)
            .await
            .context(TimeoutSnafu)
    }

    /// Call `wait_timeout_until_with_trigger` on the state's [`SubscribableMutex`]
    pub fn state_wait_timeout_until_with_trigger<'a, F>(
        &'a self,
        timeout: Duration,
        f: F,
    ) -> async_std::stream::Timeout<FuturesOrdered<impl Future<Output = ()> + 'a>>
    where
        F: FnMut(&S) -> bool + 'a,
    {
        self.state.wait_timeout_until_with_trigger(timeout, f)
    }

    /// Call `wait_until` on the state's [`SubscribableMutex`]
    /// # Errors
    /// Will throw a [`NetworkNodeHandleError::TimeoutError`] error upon timeout
    pub async fn state_wait_until<F>(&self, f: F) -> Result<(), NetworkNodeHandleError>
    where
        F: FnMut(&S) -> bool,
    {
        self.state.wait_until(f).await;
        Ok(())
    }
}

impl<S: Clone> NetworkNodeHandle<S> {
    /// Get a clone of the internal state
    pub async fn state(&self) -> S {
        self.state.cloned().await
    }
}

impl From<NetworkNodeHandleError> for HotShotNetworkError {
    fn from(error: NetworkNodeHandleError) -> Self {
        HotShotNetworkError::Other {
            inner: Box::new(error),
        }
    }
}

/// Error wrapper type for interacting with swarm handle
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum NetworkNodeHandleError {
    /// Error generating network
    NetworkError {
        /// source of error
        source: NetworkError,
    },
    /// Failure to serialize a message
    SerializationError {
        /// source of error
        source: Box<bincode::ErrorKind>,
    },
    /// Failure to deserialize a message
    DeserializationError {
        /// source of error
        source: Box<bincode::ErrorKind>,
    },
    /// Error sending request to network
    SendError,
    /// Error receiving message from network
    RecvError,
    /// Error building Node config
    NodeConfigError {
        /// source of error
        source: NetworkNodeConfigBuilderError,
    },
    /// Error waiting for connections
    TimeoutError {
        /// source of error
        source: TimeoutError,
    },
    /// Error in the kademlia DHT
    DHTError {
        /// source of error
        source: DHTError,
    },
    /// The inner [`NetworkNode`] has already been killed
    CantKillTwice {
        /// dummy source
        source: SendError<()>,
    },
}

/// Re-exports of the snafu errors that [`NetworkNodeHandleError`] can throw
pub mod network_node_handle_error {
    pub use super::{
        NetworkSnafu, NodeConfigSnafu, RecvSnafu, SendSnafu, SerializationSnafu, TimeoutSnafu,
    };
}
