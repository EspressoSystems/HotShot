use crate::network::{
    gen_multiaddr, ClientRequest, ConnectionData, NetworkError, NetworkEvent, NetworkNode,
    NetworkNodeConfig, NetworkNodeConfigBuilderError,
};
use async_std::{
    future::TimeoutError,
    sync::{Condvar, Mutex},
};
use flume::{bounded, Receiver, RecvError, SendError, Sender};
use libp2p::{Multiaddr, PeerId};
use phaselock_utils::subscribable_mutex::SubscribableMutex;
use snafu::{ResultExt, Snafu};
use std::{collections::HashSet, fmt::Debug, sync::Arc, time::Duration};
use tracing::{info, instrument};

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
    /// the connection metadata associated with the networkbehaviour
    connection_state: Arc<SubscribableMutex<ConnectionData>>,
    /// human readable id
    id: usize,

    /// A list of webui listeners that are listening for changes on this node
    // TODO: Replace the following fields with `SubscribableMutex` (see https://github.com/EspressoSystems/phaselock/pull/33)
    // - `state: Arc<Mutex<S>>`
    // - `connection_state: Arc<Mutex<ConnectionData>>`
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
        // TODO separate this into a separate function so you can make everyone know about everyone
        let listen_addr = network
            .start_listen(listen_addr)
            .await
            .context(NetworkSnafu)?;
        let (send_chan, recv_chan) = network.spawn_listeners().await.context(NetworkSnafu)?;
        let (kill_switch, recv_kill) = flume::bounded(1);

        Ok(NetworkNodeHandle {
            network_config: config,
            state_changed: Condvar::new(),
            state: Arc::new(SubscribableMutex::new(S::default())),
            send_network: send_chan,
            recv_network: recv_chan,
            killed: Arc::new(Mutex::new(false)),
            kill_switch,
            recv_kill,
            listen_addr,
            peer_id,
            connection_state: Arc::default(),
            id,
            webui_listeners: Arc::default(),
        })
    }

    /// Cleanly shuts down a swarm node
    /// This is done by sending a message to
    /// the swarm event handler to stop handling events
    /// and a message to the swarm itself to spin down
    #[instrument]
    pub async fn shutdown(&self) -> Result<(), NetworkError> {
        self.send_network
            .send_async(ClientRequest::Shutdown)
            .await
            .map_err(|_e| NetworkError::StreamClosed)?;
        self.kill_switch
            .send_async(())
            .await
            .map_err(|_e| NetworkError::StreamClosed)?;
        Ok(())
    }

    /// Wait for a node to connect to other nodes
    #[instrument]
    pub async fn wait_to_connect(
        node: Arc<NetworkNodeHandle<S>>,
        num_peers: usize,
        chan: Receiver<NetworkEvent>,
        node_idx: usize,
    ) -> Result<(), NetworkNodeHandleError> {
        info!("waiting to connect!");
        let mut connected_ok = false;
        let mut known_ok = false;
        while !(known_ok && connected_ok) {
            match chan.recv_async().await.context(RecvSnafu)? {
                NetworkEvent::UpdateConnectedPeers(pids) => {
                    info!(
                        "updating connected peers to: {}, waiting on {}",
                        pids.len(),
                        num_peers
                    );
                    node.connection_state
                        .modify(|state| {
                            state.connected_peers = pids.clone();
                        })
                        .await;
                    connected_ok = pids.len() >= num_peers;
                    node.notify_webui().await;
                }
                NetworkEvent::UpdateKnownPeers(pids) => {
                    node.connection_state
                        .modify(|state| {
                            state.known_peers = pids.clone();
                        })
                        .await;
                    known_ok = pids.len() >= num_peers;
                    node.notify_webui().await;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Get a reference to the network node handle's listen addr.
    pub fn listen_addr(&self) -> Multiaddr {
        self.listen_addr.clone()
    }
}

impl<S> NetworkNodeHandle<S> {
    /// Notify the webui that either the `state` or `connection_state` has changed.
    ///
    /// If the webui is not started, this will do nothing.
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
    ///
    /// Will throw a [`SendSnafu`] error if all receivers are dropped.
    pub async fn send_request(&self, req: ClientRequest) -> Result<(), NetworkNodeHandleError> {
        self.send_network.send_async(req).await.context(SendSnafu)?;
        Ok(())
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

    /// Get a clone of the internal connection state
    pub async fn connection_state(&self) -> ConnectionData {
        self.connection_state.cloned().await
    }

    /// Get a clone of the known peers list
    pub async fn known_peers(&self) -> HashSet<PeerId> {
        self.connection_state.cloned().await.known_peers
    }

    /// Get a clone of the connected peers list
    pub async fn connected_peers(&self) -> HashSet<PeerId> {
        self.connection_state.cloned().await.connected_peers
    }

    /// Get a clone of the ignored peers list
    pub async fn ignored_peers(&self) -> HashSet<PeerId> {
        self.connection_state.cloned().await.ignored_peers
    }

    /// Modify the state. This will automatically call `state_changed` and `notify_webui`
    pub async fn modify_state<F>(&self, cb: F)
    where
        F: FnMut(&mut S),
    {
        self.state.modify(cb).await;
        self.state_changed.notify_all();
        self.notify_webui().await;
    }

    /// Set the connected peers list
    pub async fn set_connected_peers(&self, peers: HashSet<PeerId>) {
        self.connection_state
            .modify(|s| {
                s.connected_peers = peers;
            })
            .await;
    }

    /// Set the known peers list
    pub async fn set_known_peers(&self, peers: HashSet<PeerId>) {
        self.connection_state
            .modify(|s| {
                s.known_peers = peers;
            })
            .await;
    }

    /// Add a peer to the ignored peers list
    pub async fn add_ignored_peer(&self, peer: PeerId) {
        self.connection_state
            .modify(|s| {
                s.ignored_peers.insert(peer);
            })
            .await;
    }

    /// Get a reference to the internal Condvar. This will be triggered whenever a different task calls [`modify_state`]
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
    /// will notify the caller through `chan` when listening for event
    /// # Errors
    /// Will throw a [`TimeoutSnafu`] error upon timeout
    pub async fn state_wait_timeout_until<F>(
        &self,
        timeout: Duration,
        f: F,
        chan: Sender<bool>,
    ) -> Result<(), async_std::future::TimeoutError>
    where
        F: FnMut(&S) -> bool,
    {
        self.state.wait_timeout_until(timeout, f, chan).await
    }
}

impl<S: Clone> NetworkNodeHandle<S> {
    /// Get a clone of the internal state
    pub async fn state(&self) -> S {
        self.state.cloned().await
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
    DeserializationError {},
    /// Error sending request to network
    SendError {
        /// source of error
        source: SendError<ClientRequest>,
    },
    /// Error receiving message from network
    RecvError {
        /// source of error
        source: RecvError,
    },
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
}

/// Re-exports of the snafu errors that [`NetworkNodeHandleError`] can throw
pub mod network_node_handle_error {
    pub use super::{
        NetworkSnafu, NodeConfigSnafu, RecvSnafu, SendSnafu, SerializationSnafu, TimeoutSnafu,
    };
}
