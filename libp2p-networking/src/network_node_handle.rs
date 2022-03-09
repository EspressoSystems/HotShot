use crate::{
    network::{NetworkNode, NetworkNodeConfig, NetworkNodeConfigBuilderError},
    network_node::{gen_multiaddr, ClientRequest, ConnectionData, NetworkError, NetworkEvent},
};
use async_std::{
    future::{timeout, TimeoutError},
    sync::{Condvar, Mutex},
    task::spawn,
};
use flume::{Receiver, RecvError, SendError, Sender};
use futures::{select, Future, FutureExt};
use libp2p::{Multiaddr, PeerId};
use rand::{seq::IteratorRandom, thread_rng};
use snafu::{ResultExt, Snafu};
use std::{fmt::Debug, sync::Arc, time::Duration};
use tracing::{info, info_span, instrument, Instrument};

/// A handle containing:
/// - A reference to the state
/// - Controls for the swarm
#[derive(Debug)]
pub struct NetworkNodeHandle<S> {
    /// network configuration
    pub network_config: NetworkNodeConfig,
    /// notifies that a state change has occurred
    pub state_changed: Condvar,
    /// the state of the replica
    pub state: Arc<Mutex<S>>,
    /// send an action to the networkbehaviour
    pub send_network: Sender<ClientRequest>,
    /// receive an action from the networkbehaviour
    pub recv_network: Receiver<NetworkEvent>,
    /// whether or not the handle has been killed
    pub killed: Arc<Mutex<bool>>,
    /// kill the event handler for events from the swarm
    pub kill_switch: Sender<()>,
    /// receiving end of `kill_switch`
    pub recv_kill: Receiver<()>,
    /// the local address we're listening on
    pub listen_addr: Multiaddr,
    /// the peer id of the networkbehaviour
    pub peer_id: PeerId,
    /// the connection metadata associated with the networkbehaviour
    pub connection_state: Arc<Mutex<ConnectionData>>,
    /// human readable id
    pub id: usize,

    /// A list of webui listeners that are listening for changes on this node
    // TODO: Replace the following fields with `SubscribableMutex` (see https://github.com/EspressoSystems/phaselock/pull/33)
    // - `state: Arc<Mutex<S>>`
    // - `connection_state: Arc<Mutex<ConnectionData>>`
    pub webui_listeners: Arc<Mutex<Vec<Sender<()>>>>,
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
        let peer_id = network.peer_id;
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
            state: Arc::new(Mutex::new(S::default())),
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
    pub async fn kill(&self) -> Result<(), NetworkError> {
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
                    node.connection_state.lock().await.connected_peers = pids.clone();
                    connected_ok = pids.len() >= num_peers;
                    node.notify_webui().await;
                }
                NetworkEvent::UpdateKnownPeers(pids) => {
                    node.connection_state.lock().await.known_peers = pids.clone();
                    known_ok = pids.len() >= num_peers;
                    node.notify_webui().await;
                }
                _ => {}
            }
        }
        Ok(())
    }

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
}

/// Glue function that listens for events from the Swarm corresponding to `handle`
/// and calls `event_handler` when an event is observed.
/// The idea is that this function can be used independent of the actual state
/// we use
#[allow(clippy::panic)]
#[instrument(skip(event_handler))]
pub async fn spawn_handler<S: 'static + Send + Default + Debug, Fut>(
    handle: Arc<NetworkNodeHandle<S>>,
    event_handler: impl (Fn(NetworkEvent, Arc<NetworkNodeHandle<S>>) -> Fut)
        + std::marker::Sync
        + std::marker::Send
        + 'static,
) where
    Fut: Future<Output = Result<(), NetworkNodeHandleError>>
        + std::marker::Send
        + 'static
        + std::marker::Sync,
{
    let recv_kill = handle.recv_kill.clone();
    let recv_event = handle.recv_network.clone();
    spawn(
        async move {
            loop {
                select!(
                    _ = recv_kill.recv_async().fuse() => {
                        *handle.killed.lock().await = true;
                        break;
                    },
                    event = recv_event.recv_async().fuse() => {
                        event_handler(event.context(RecvSnafu)?, handle.clone()).await?;
                    },
                );
            }
            Ok::<(), NetworkNodeHandleError>(())
        }
        .instrument(info_span!("Libp2p Counter Handler")),
    );
}

/// a single node, connects them to each other
/// and waits for connections to propagate to all nodes.
#[instrument]
pub async fn spin_up_swarm<S: std::fmt::Debug + Default>(
    timeout_len: Duration,
    known_nodes: Vec<(Option<PeerId>, Multiaddr)>,
    config: NetworkNodeConfig,
    idx: usize,
    handle: &Arc<NetworkNodeHandle<S>>,
) -> Result<(), NetworkNodeHandleError> {
    info!("known_nodes{:?}", known_nodes);
    handle
        .send_network
        .send_async(ClientRequest::AddKnownPeers(known_nodes))
        .await
        .context(SendSnafu)?;

    timeout(
        timeout_len,
        NetworkNodeHandle::wait_to_connect(
            handle.clone(),
            config.max_num_peers,
            handle.recv_network.clone(),
            idx,
        ),
    )
    .await
    .context(TimeoutSnafu)??;
    handle
        .send_network
        .send_async(ClientRequest::Subscribe("global".to_string()))
        .await
        .context(SendSnafu)?;

    Ok(())
}

/// Given a slice of handles assumed to be larger than 0,
/// chooses one
/// # Panics
/// panics if handles is of length 0
pub fn get_random_handle<S>(handles: &[Arc<NetworkNodeHandle<S>>]) -> Arc<NetworkNodeHandle<S>> {
    handles.iter().choose(&mut thread_rng()).unwrap().clone()
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
