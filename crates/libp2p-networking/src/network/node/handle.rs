use crate::network::{
    behaviours::request_response::{Request, Response},
    error::{CancelledRequestSnafu, DHTError},
    gen_multiaddr, ClientRequest, NetworkError, NetworkEvent, NetworkNode, NetworkNodeConfig,
    NetworkNodeConfigBuilderError,
};
use async_compatibility_layer::{
    art::{async_sleep, async_timeout, future::to},
    channel::{Receiver, SendError, UnboundedReceiver, UnboundedRecvError, UnboundedSender},
};
use futures::channel::oneshot;

use hotshot_types::traits::network::NetworkError as HotshotNetworkError;
use libp2p::{request_response::ResponseChannel, Multiaddr};
use libp2p_identity::PeerId;
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};
use std::{
    collections::HashSet,
    fmt::Debug,
    time::{Duration, Instant},
};
use tracing::{debug, info, instrument};
use versioned_binary_serialization::{version::StaticVersionType, BinarySerializer, Serializer};

/// A handle containing:
/// - A reference to the state
/// - Controls for the swarm
#[derive(Debug, Clone)]
pub struct NetworkNodeHandle {
    /// network configuration
    network_config: NetworkNodeConfig,

    /// send an action to the networkbehaviour
    send_network: UnboundedSender<ClientRequest>,

    /// the local address we're listening on
    listen_addr: Multiaddr,

    /// the peer id of the networkbehaviour
    peer_id: PeerId,

    /// human readable id
    id: usize,
}

/// internal network node receiver
#[derive(Debug)]
pub struct NetworkNodeReceiver {
    /// the receiver
    receiver: UnboundedReceiver<NetworkEvent>,

    ///kill switch
    recv_kill: Option<Receiver<()>>,
}

impl NetworkNodeReceiver {
    /// recv a network event
    /// # Errors
    /// Errors if the receiver channel is closed
    pub async fn recv(&self) -> Result<NetworkEvent, NetworkNodeHandleError> {
        self.receiver.recv().await.context(ReceiverEndedSnafu)
    }
    /// Add a kill switch to the receiver
    pub fn set_kill_switch(&mut self, kill_switch: Receiver<()>) {
        self.recv_kill = Some(kill_switch);
    }

    /// Take the kill switch to allow killing the receiver task
    pub fn take_kill_switch(&mut self) -> Option<Receiver<()>> {
        self.recv_kill.take()
    }
}

/// Spawn a network node task task and return the handle and the receiver for it
/// # Errors
/// Errors if spawning the task fails
pub async fn spawn_network_node(
    config: NetworkNodeConfig,
    id: usize,
) -> Result<(NetworkNodeReceiver, NetworkNodeHandle), NetworkNodeHandleError> {
    let mut network = NetworkNode::new(config.clone())
        .await
        .context(NetworkSnafu)?;
    // randomly assigned port
    let listen_addr = config
        .bound_addr
        .clone()
        .unwrap_or_else(|| gen_multiaddr(0));
    let peer_id = network.peer_id();
    let listen_addr = network
        .start_listen(listen_addr)
        .await
        .context(NetworkSnafu)?;
    // pin here to force the future onto the heap since it can be large
    // in the case of flume
    let (send_chan, recv_chan) = Box::pin(network.spawn_listeners())
        .await
        .context(NetworkSnafu)?;
    let receiver = NetworkNodeReceiver {
        receiver: recv_chan,
        recv_kill: None,
    };

    info!("LISTEN ADDRESS IS {:?}", listen_addr);

    let handle = NetworkNodeHandle {
        network_config: config,
        send_network: send_chan,
        listen_addr,
        peer_id,
        id,
    };
    Ok((receiver, handle))
}

impl NetworkNodeHandle {
    /// Cleanly shuts down a swarm node
    /// This is done by sending a message to
    /// the swarm itself to spin down
    #[instrument]
    pub async fn shutdown(&self) -> Result<(), NetworkNodeHandleError> {
        self.send_request(ClientRequest::Shutdown).await?;
        Ok(())
    }
    /// Notify the network to begin the bootstrap process
    /// # Errors
    /// If unable to send via `send_network`. This should only happen
    /// if the network is shut down.
    pub async fn begin_bootstrap(&self) -> Result<(), NetworkNodeHandleError> {
        let req = ClientRequest::BeginBootstrap;
        self.send_request(req).await
    }

    /// Get a reference to the network node handle's listen addr.
    #[must_use]
    pub fn listen_addr(&self) -> Multiaddr {
        self.listen_addr.clone()
    }

    /// Print out the routing table used by kademlia
    /// NOTE: only for debugging purposes currently
    /// # Errors
    /// if the client has stopped listening for a response
    pub async fn print_routing_table(&self) -> Result<(), NetworkNodeHandleError> {
        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::GetRoutingTable(s);
        self.send_request(req).await?;
        r.await.map_err(|_| NetworkNodeHandleError::RecvError)
    }
    /// Wait until at least `num_peers` have connected, or until `timeout` time has passed.
    ///
    /// # Errors
    ///
    /// Will return any networking error encountered, or `ConnectTimeout` if the `timeout` has elapsed.
    pub async fn wait_to_connect(
        &self,
        num_peers: usize,
        node_id: usize,
        timeout: Duration,
    ) -> Result<(), NetworkNodeHandleError> {
        let start = Instant::now();
        self.begin_bootstrap().await?;
        let mut connected_ok = false;
        while !connected_ok {
            if start.elapsed() >= timeout {
                return Err(NetworkNodeHandleError::ConnectTimeout);
            }
            async_sleep(Duration::from_secs(1)).await;
            let num_connected = self.num_connected().await?;
            info!(
                "WAITING TO CONNECT, connected to {} / {} peers ON NODE {}",
                num_connected, num_peers, node_id
            );
            connected_ok = num_connected >= num_peers;
        }
        Ok(())
    }

    /// Request another peer for some data we want.  Returns the id of the request
    ///
    /// # Errors
    ///
    /// Will retrun a networking error if the channel closes before the result
    /// can be sent back
    pub async fn request_data<VER: StaticVersionType>(
        &self,
        request: &impl Serialize,
        peer: PeerId,
        _: VER,
    ) -> Result<Option<Response>, NetworkNodeHandleError> {
        let (tx, rx) = oneshot::channel();
        let serialized_msg = Serializer::<VER>::serialize(request).context(SerializationSnafu)?;
        let req = ClientRequest::DataRequest {
            request: Request(serialized_msg),
            peer,
            chan: tx,
        };

        self.send_request(req).await?;

        rx.await.map_err(|_| NetworkNodeHandleError::RecvError)
    }

    /// Send a response to a request with the response channel
    /// # Errors
    /// Will error if the client request channel is closed, or serialization fails.
    pub async fn respond_data<VER: StaticVersionType>(
        &self,
        response: &impl Serialize,
        chan: ResponseChannel<Response>,
        _: VER,
    ) -> Result<(), NetworkNodeHandleError> {
        let serialized_msg = Serializer::<VER>::serialize(response).context(SerializationSnafu)?;
        let req = ClientRequest::DataResponse {
            response: Response(serialized_msg),
            chan,
        };
        self.send_request(req).await
    }

    /// Look up a peer's addresses in kademlia
    /// NOTE: this should always be called before any `request_response` is initiated
    /// # Errors
    /// if the client has stopped listening for a response
    pub async fn lookup_pid(&self, peer_id: PeerId) -> Result<(), NetworkNodeHandleError> {
        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::LookupPeer(peer_id, s);
        self.send_request(req).await?;
        r.await.map_err(|_| NetworkNodeHandleError::RecvError)
    }

    /// Looks up a node's `PeerId` and attempts to validate routing
    /// # Errors
    /// if the peer was unable to be looked up (did not provide a response, DNE)
    pub async fn lookup_node<V: for<'a> Deserialize<'a> + Serialize, VER: StaticVersionType>(
        &self,
        key: V,
        dht_timeout: Duration,
        bind_version: VER,
    ) -> Result<PeerId, NetworkNodeHandleError> {
        // get record (from DHT)
        let pid = self
            .get_record_timeout::<PeerId, VER>(&key, dht_timeout, bind_version)
            .await?;

        // pid lookup for routing
        // self.lookup_pid(pid).await?;

        Ok(pid)
    }

    /// Insert a record into the kademlia DHT
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::DHTError`] when encountering an error putting to DHT
    /// - Will return [`NetworkNodeHandleError::SerializationError`] when unable to serialize the key or value
    pub async fn put_record<VER: StaticVersionType>(
        &self,
        key: &impl Serialize,
        value: &impl Serialize,
        _: VER,
    ) -> Result<(), NetworkNodeHandleError> {
        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::PutDHT {
            key: Serializer::<VER>::serialize(key).context(SerializationSnafu)?,
            value: Serializer::<VER>::serialize(value).context(SerializationSnafu)?,
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
    pub async fn get_record<V: for<'a> Deserialize<'a>, VER: StaticVersionType>(
        &self,
        key: &impl Serialize,
        retry_count: u8,
        _: VER,
    ) -> Result<V, NetworkNodeHandleError> {
        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::GetDHT {
            key: Serializer::<VER>::serialize(key).context(SerializationSnafu)?,
            notify: s,
            retry_count,
        };
        self.send_request(req).await?;

        match r.await.context(CancelledRequestSnafu) {
            Ok(result) => Serializer::<VER>::deserialize(&result).context(DeserializationSnafu),
            Err(e) => Err(e).context(DHTSnafu),
        }
    }

    /// Get a record from the kademlia DHT with a timeout
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::DHTError`] when encountering an error putting to DHT
    /// - Will return [`NetworkNodeHandleError::TimeoutError`] when times out
    /// - Will return [`NetworkNodeHandleError::SerializationError`] when unable to serialize the key
    /// - Will return [`NetworkNodeHandleError::DeserializationError`] when unable to deserialize the returned value
    pub async fn get_record_timeout<V: for<'a> Deserialize<'a>, VER: StaticVersionType>(
        &self,
        key: &impl Serialize,
        timeout: Duration,
        bind_version: VER,
    ) -> Result<V, NetworkNodeHandleError> {
        let result = async_timeout(timeout, self.get_record(key, 3, bind_version)).await;
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
    pub async fn put_record_timeout<VER: StaticVersionType>(
        &self,
        key: &impl Serialize,
        value: &impl Serialize,
        timeout: Duration,
        bind_version: VER,
    ) -> Result<(), NetworkNodeHandleError> {
        let result = async_timeout(timeout, self.put_record(key, value, bind_version)).await;
        match result {
            Err(e) => Err(e).context(TimeoutSnafu),
            Ok(r) => r,
        }
    }

    /// Subscribe to a topic
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    pub async fn subscribe(&self, topic: String) -> Result<(), NetworkNodeHandleError> {
        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::Subscribe(topic, Some(s));
        self.send_request(req).await?;
        r.await.map_err(|_| NetworkNodeHandleError::RecvError)
    }

    /// Unsubscribe from a topic
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    pub async fn unsubscribe(&self, topic: String) -> Result<(), NetworkNodeHandleError> {
        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::Unsubscribe(topic, Some(s));
        self.send_request(req).await?;
        r.await.map_err(|_| NetworkNodeHandleError::RecvError)
    }

    /// Ignore `peers` when pruning
    /// e.g. maintain their connection
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    pub async fn ignore_peers(&self, peers: Vec<PeerId>) -> Result<(), NetworkNodeHandleError> {
        let req = ClientRequest::IgnorePeers(peers);
        self.send_request(req).await
    }

    /// Make a direct request to `peer_id` containing `msg`
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    /// - Will return [`NetworkNodeHandleError::SerializationError`] when unable to serialize `msg`
    pub async fn direct_request<VER: StaticVersionType>(
        &self,
        pid: PeerId,
        msg: &impl Serialize,
        _: VER,
    ) -> Result<(), NetworkNodeHandleError> {
        let serialized_msg = Serializer::<VER>::serialize(msg).context(SerializationSnafu)?;
        self.direct_request_no_serialize(pid, serialized_msg).await
    }

    /// Make a direct request to `peer_id` containing `msg` without serializing
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    /// - Will return [`NetworkNodeHandleError::SerializationError`] when unable to serialize `msg`
    pub async fn direct_request_no_serialize(
        &self,
        pid: PeerId,
        contents: Vec<u8>,
    ) -> Result<(), NetworkNodeHandleError> {
        let req = ClientRequest::DirectRequest {
            pid,
            contents,
            retry_count: 1,
        };
        self.send_request(req).await
    }

    /// Reply with `msg` to a request over `chan`
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    /// - Will return [`NetworkNodeHandleError::SerializationError`] when unable to serialize `msg`
    pub async fn direct_response<VER: StaticVersionType>(
        &self,
        chan: ResponseChannel<Vec<u8>>,
        msg: &impl Serialize,
        _: VER,
    ) -> Result<(), NetworkNodeHandleError> {
        let serialized_msg = Serializer::<VER>::serialize(msg).context(SerializationSnafu)?;
        let req = ClientRequest::DirectResponse(chan, serialized_msg);
        self.send_request(req).await
    }

    /// Forcefully disconnect from a peer
    /// # Errors
    /// If the channel is closed somehow
    /// Shouldnt' happen.
    /// # Panics
    /// If channel errors out
    /// shouldn't happen.
    pub async fn prune_peer(&self, pid: PeerId) -> Result<(), NetworkNodeHandleError> {
        let req = ClientRequest::Prune(pid);
        self.send_request(req).await
    }

    /// Gossip a message to peers
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    /// - Will return [`NetworkNodeHandleError::SerializationError`] when unable to serialize `msg`
    pub async fn gossip<VER: StaticVersionType>(
        &self,
        topic: String,
        msg: &impl Serialize,
        _: VER,
    ) -> Result<(), NetworkNodeHandleError> {
        let serialized_msg = Serializer::<VER>::serialize(msg).context(SerializationSnafu)?;
        self.gossip_no_serialize(topic, serialized_msg).await
    }

    /// Gossip a message to peers without serializing
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    /// - Will return [`NetworkNodeHandleError::SerializationError`] when unable to serialize `msg`
    pub async fn gossip_no_serialize(
        &self,
        topic: String,
        msg: Vec<u8>,
    ) -> Result<(), NetworkNodeHandleError> {
        let req = ClientRequest::GossipMsg(topic, msg);
        self.send_request(req).await
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
        self.send_request(req).await
    }

    /// Send a client request to the network
    ///
    /// # Errors
    /// - Will return [`NetworkNodeHandleError::SendError`] when underlying `NetworkNode` has been killed
    async fn send_request(&self, req: ClientRequest) -> Result<(), NetworkNodeHandleError> {
        debug!("peerid {:?}\t\tsending message {:?}", self.peer_id, req);
        self.send_network
            .send(req)
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
    #[must_use]
    pub fn id(&self) -> usize {
        self.id
    }

    /// Get a reference to the network node handle's peer id.
    #[must_use]
    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    /// Return a reference to the network config
    #[must_use]
    pub fn config(&self) -> &NetworkNodeConfig {
        &self.network_config
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
        source: anyhow::Error,
    },
    /// Failure to deserialize a message
    DeserializationError {
        /// source of error
        source: anyhow::Error,
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
        source: to::TimeoutError,
    },
    /// Could not connect to the network in time
    ConnectTimeout,
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
    /// The network node has been killed
    Killed,
    /// The receiver was unable to receive a new message
    ReceiverEnded {
        /// source of error
        source: UnboundedRecvError,
    },
    /// no known topic matches the hashset of keys
    NoSuchTopic,
}

impl From<NetworkNodeHandleError> for HotshotNetworkError {
    fn from(error: NetworkNodeHandleError) -> Self {
        match error {
            NetworkNodeHandleError::SerializationError { source } => {
                HotshotNetworkError::FailedToSerialize { source }
            }
            NetworkNodeHandleError::DeserializationError { source } => {
                HotshotNetworkError::FailedToDeserialize { source }
            }
            NetworkNodeHandleError::TimeoutError { source } => {
                HotshotNetworkError::Timeout { source }
            }
            NetworkNodeHandleError::Killed => HotshotNetworkError::ShutDown,
            source => HotshotNetworkError::Libp2p {
                source: Box::new(source),
            },
        }
    }
}

/// Re-exports of the snafu errors that [`NetworkNodeHandleError`] can throw
pub mod network_node_handle_error {
    pub use super::{
        NetworkSnafu, NodeConfigSnafu, RecvSnafu, SendSnafu, SerializationSnafu, TimeoutSnafu,
    };
}
