//! Websockets based prototyping networking implementation
//!
//! This module provides a websockets based networking implementation, where each node in the
//! network forms a websocket connection to every other node.
//!
//! This implementation is useful for testing, due to its simplicity, but is not production grade.

use super::NetworkingImplementation;
use crate::traits::{
    networking::{
        ChannelDisconnectedSnafu, CouldNotDeliverSnafu, ExecutorSnafu, FailedToBindListenerSnafu,
        NoSocketsSnafu, SocketDecodeSnafu, WebSocketSnafu,
    },
    NetworkError,
};
use async_compatibility_layer::{
    art::{async_block_on, async_sleep, async_spawn, async_timeout},
    channel::{bounded, unbounded, Receiver, Sender, UnboundedReceiver, UnboundedSender},
};
use async_lock::{Mutex, RwLock};
use async_trait::async_trait;
use async_tungstenite::{
    accept_async, client_async,
    tungstenite::{error::Error as WsError, Message},
    WebSocketStream,
};
use bincode::Options;
use dashmap::DashMap;
use futures::{channel::oneshot, future::BoxFuture, prelude::*};
use hotshot_types::{
    data::{LeafType, ProposalType, ValidatingLeaf, ValidatingProposal},
    message::Message as HotShotMessage,
    traits::{
        election::Election,
        network::{NetworkChange, TestableNetworkingImplementation},
        node_implementation::NodeTypes,
        signature_key::{SignatureKey, TestableSignatureKey},
        state::ValidatingConsensus,
    },
};
use hotshot_utils::bincode::bincode_opts;
use rand::prelude::ThreadRng;
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt};
use std::{
    fmt::Debug,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tracing::{debug, error, info, info_span, instrument, trace, warn, Instrument};
use tracing_unwrap::ResultExt as RXT;

#[cfg(feature = "async-std-executor")]
use async_std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
#[cfg(feature = "tokio-executor")]
use std::net::SocketAddr;
#[cfg(feature = "tokio-executor")]
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};
#[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
std::compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(bound(deserialize = ""))]
/// Inter-node protocol level message types
pub enum Command<
    TYPES: NodeTypes,
    LEAF: LeafType<NodeType = TYPES>,
    PROPOSAL: ProposalType<NodeTypes = TYPES>,
> {
    /// A message that was broadcast to all nodes
    Broadcast {
        /// Message being sent
        inner: HotShotMessage<TYPES, LEAF, PROPOSAL>,
        /// Who is sending it
        from: TYPES::SignatureKey,
        /// Message ID
        id: u64,
    },
    /// A message that was sent directly to this node
    Direct {
        /// Message being sent
        inner: HotShotMessage<TYPES, LEAF, PROPOSAL>,
        /// Who is sending it
        from: TYPES::SignatureKey,
        /// Who its being sent to
        to: TYPES::SignatureKey,
        /// Message ID
        id: u64,
    },
    /// A message identifying the sending node
    Identify {
        /// Who the message is from
        from: TYPES::SignatureKey,
        /// Message ID
        id: u64,
    },
    /// Ping keepalive message
    Ping {
        /// Message ID
        id: u64,
    },
    /// Acknowledge
    Ack {
        /// Message being acknowledged
        ack_id: u64,
        /// Message ID
        id: u64,
    },
}

impl<
        TYPES: NodeTypes,
        LEAF: LeafType<NodeType = TYPES>,
        PROPOSAL: ProposalType<NodeTypes = TYPES>,
    > Command<TYPES, LEAF, PROPOSAL>
{
    /// Returns the id of this `Command`
    pub fn id(&self) -> u64 {
        match self {
            Command::Broadcast { id, .. }
            | Command::Direct { id, .. }
            | Command::Identify { id, .. }
            | Command::Ping { id, .. }
            | Command::Ack { id, .. } => *id,
        }
    }
}

/// The handle used for interacting with a [`WNetwork`] connection
#[derive(Clone)]
struct Handle<
    TYPES: NodeTypes,
    LEAF: LeafType<NodeType = TYPES>,
    PROPOSAL: ProposalType<NodeTypes = TYPES>,
> {
    /// Messages to be sent by this node
    outbound: Sender<Command<TYPES, LEAF, PROPOSAL>>,
    /// The address of the remote
    remote_socket: SocketAddr,
    /// Indicate that the handle should be closed
    shutdown: Arc<RwLock<bool>>,
    /// The last time the remote sent us a message
    last_message: Arc<Mutex<Instant>>,
}

/// The inner shared state of a [`WNetwork`] instance
struct WNetworkInner<
    TYPES: NodeTypes,
    LEAF: LeafType<NodeType = TYPES>,
    PROPOSAL: ProposalType<NodeTypes = TYPES>,
> {
    /// Whether or not the network is connected
    is_ready: Arc<AtomicBool>,
    /// The handles for each known public [`SignatureKey`]
    handles: DashMap<TYPES::SignatureKey, Handle<TYPES, LEAF, PROPOSAL>>,
    /// The public [`SignatureKey`] of this node
    pub_key: TYPES::SignatureKey,
    /// The global message counter
    counter: Arc<AtomicU64>,
    /// The `SocketAddr` that this [`WNetwork`] listens on
    socket: SocketAddr,
    /// The currently pending `Waiters`
    waiters: Waiters,
    /// The inputs to the internal queues
    inputs: Inputs<HotShotMessage<TYPES, LEAF, PROPOSAL>>,
    /// The outputs to the internal queues
    outputs: Outputs<HotShotMessage<TYPES, LEAF, PROPOSAL>>,
    /// Keeps track of if the tasks have been started
    tasks_started: AtomicBool,
    /// Holds onto to a TCP socket between binding and task start
    socket_holder: Mutex<Option<TcpListener>>,
    /// Duration in between keepalive pings
    keep_alive_duration: Duration,
    /// Sender for changes in the network
    network_change_input: UnboundedSender<NetworkChange<TYPES::SignatureKey>>,
    /// Receiver for changes in the network
    network_change_output: UnboundedReceiver<NetworkChange<TYPES::SignatureKey>>,
}

/// Shared waiting state for a [`WNetwork`] instance
struct Waiters {
    /// Waiting on a message to be delivered
    delivered: DashMap<u64, oneshot::Sender<()>>,
    /// Waiting on a message to be acked
    acked: DashMap<u64, oneshot::Sender<()>>,
}

/// Holds onto the input queues for a [`WNetwork`]
#[derive(Clone)]
struct Inputs<T> {
    /// Input to broadcast queue
    broadcast: Sender<T>,
    /// Input to direct queue
    direct: Sender<T>,
}

/// Holds onto the output queues for a [`WNetwork`]
#[derive(Clone)]
struct Outputs<T> {
    /// Output from broadcast queue
    broadcast: Arc<Mutex<Receiver<T>>>,
    /// Output from direct queue
    direct: Arc<Mutex<Receiver<T>>>,
}

/// Internal enum for combining message and command streams
enum Combo<
    TYPES: NodeTypes,
    LEAF: LeafType<NodeType = TYPES>,
    PROPOSAL: ProposalType<NodeTypes = TYPES>,
> {
    /// Inbound message
    Message(Message),
    /// Outbound command
    Command(Command<TYPES, LEAF, PROPOSAL>),
    /// Error
    Error(WsError),
}

#[derive(Clone)]
/// Handle to the underlying networking implementation
pub struct WNetwork<
    TYPES: NodeTypes,
    LEAF: LeafType<NodeType = TYPES>,
    PROPOSAL: ProposalType<NodeTypes = TYPES>,
> {
    /// Pointer to the internal state of this [`WNetwork`]
    inner: Arc<WNetworkInner<TYPES, LEAF, PROPOSAL>>,
}

impl<
        TYPES: NodeTypes,
        LEAF: LeafType<NodeType = TYPES>,
        PROPOSAL: ProposalType<NodeTypes = TYPES>,
    > Debug for WNetwork<TYPES, LEAF, PROPOSAL>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WNetwork").field("inner", &"inner").finish()
    }
}

impl<
        TYPES: NodeTypes,
        LEAF: LeafType<NodeType = TYPES>,
        PROPOSAL: ProposalType<NodeTypes = TYPES>,
    > TestableNetworkingImplementation<TYPES, LEAF, PROPOSAL> for WNetwork<TYPES, LEAF, PROPOSAL>
where
    TYPES::SignatureKey: TestableSignatureKey,
{
    fn generator(
        expected_node_count: usize,
        _num_bootstrap: usize,
    ) -> Box<dyn Fn(u64) -> Self + 'static> {
        let map = Arc::new(DashMap::<u64, u16>::new());

        Box::new(move |node_id| {
            let mut rng = rand::thread_rng();
            let priv_key = TYPES::SignatureKey::generate_test_key(node_id);
            let pub_key = TYPES::SignatureKey::from_private(&priv_key);
            let (network, port, _) = {
                async_block_on(async move {
                    get_networking::<TYPES, LEAF, PROPOSAL, ThreadRng>(
                        pub_key, "0.0.0.0", node_id, &mut rng,
                    )
                    .await
                })
            };

            network.inner.is_ready.swap(false, Ordering::Relaxed);

            map.insert(node_id, port);

            async_spawn({
                let n = network.clone();
                let map = map.clone();
                async move {
                    for i in node_id + 1..(expected_node_count as u64) {
                        let priv_key_2 = TYPES::SignatureKey::generate_test_key(i);
                        let key2: TYPES::SignatureKey =
                            TYPES::SignatureKey::from_private(&priv_key_2);
                        let port: u16 = loop {
                            let port = map.get(&i).map(|x| *x);
                            if let Some(port_a) = port {
                                break port_a;
                            }
                            // periodically check if we have enough information to connect
                            async_sleep(Duration::from_millis(2)).await;
                        };
                        let addr = format!("localhost:{}", port);
                        n.connect_to(key2, &addr)
                            .await
                            .expect("Failed to connect nodes");
                    }
                    n.inner.is_ready.swap(true, Ordering::Relaxed);
                }
            });
            network
        })
    }

    fn in_flight_message_count(&self) -> Option<usize> {
        None
    }
}

impl<
        TYPES: NodeTypes,
        LEAF: LeafType<NodeType = TYPES>,
        PROPOSAL: ProposalType<NodeTypes = TYPES>,
    > WNetwork<TYPES, LEAF, PROPOSAL>
{
    /// Processes an individual `Command`
    #[instrument(name = "WNetworking::process_command", skip(self, inputs))]
    async fn process_command(
        &self,
        command: Command<TYPES, LEAF, PROPOSAL>,
        inputs: &Inputs<HotShotMessage<TYPES, LEAF, PROPOSAL>>,
    ) -> Result<Option<Command<TYPES, LEAF, PROPOSAL>>, NetworkError> {
        trace!("Processing command");
        match command {
            Command::Broadcast { inner, .. } => {
                debug!(?inner, "Broadcast");
                let res = inputs.broadcast.send(inner).await;
                match res {
                    Ok(_) => Ok(None),
                    Err(_) => Err(NetworkError::ChannelSend),
                }
            }
            Command::Direct { inner, .. } => {
                debug!(?inner, "Broadcast");
                let res = inputs.direct.send(inner).await;
                match res {
                    Ok(_) => Ok(None),
                    Err(_) => Err(NetworkError::ChannelSend),
                }
            }
            Command::Ack { ack_id, .. } => {
                debug!(?ack_id, "Got an ack");
                let waiter = &self.inner.waiters.acked;
                let waiter = waiter.remove(&ack_id);
                match waiter {
                    Some(c) => {
                        trace!("Signaling waiter for ack");
                        let _res = c.1.send(());
                        Ok(None)
                    }
                    None => Ok(None),
                }
            }
            // Identify and Ping commands require special handling inside the task, since they
            // require an ack, and an identify command requires piping the information back out
            m => Ok(Some(m)),
        }
    }

    /// Atomically increments the message counter and returns the previous value
    fn get_next_message_id(&self) -> u64 {
        self.inner.counter.fetch_add(1, Ordering::SeqCst)
    }

    /// Spawns the task for handling a connection to a node
    #[allow(clippy::too_many_lines)]
    #[instrument(name = "WNetwork::spawn_task", skip(self, stream))]
    async fn spawn_task(
        &self,
        key: Option<TYPES::SignatureKey>,
        mut stream: WebSocketStream<TcpStream>,
        remote_socket: SocketAddr,
    ) -> Result<(TYPES::SignatureKey, Handle<TYPES, LEAF, PROPOSAL>), NetworkError> {
        info!("Spawning task to handle connection");
        let (s_outbound, r_outbound) = bounded(128);
        trace!("Opened channels");
        let shutdown = Arc::new(RwLock::new(false));
        let last_message = Arc::new(Mutex::new(Instant::now()));
        let handle = Handle {
            outbound: s_outbound,
            remote_socket,
            shutdown: shutdown.clone(),
            last_message: last_message.clone(),
        };
        let w = self.clone();
        let inputs = w.inner.inputs.clone();
        let (pk_s, pk_r) = oneshot::channel();
        let mut pk_s = Some(pk_s);
        // Identify before spawning task
        let waiter_ident = if key.is_some() {
            debug!("Identifying");
            let ident_id = w.get_next_message_id();
            let (s, r) = oneshot::channel();
            if let Some(other_waiter) = self.inner.waiters.acked.insert(ident_id, s) {
                warn!("Signaling other waiter for ack");
                let _ = other_waiter.send(());
            }
            let command = Command::<TYPES, LEAF, PROPOSAL>::Identify {
                from: w.inner.pub_key.clone(),
                id: ident_id,
            };
            // Unwrap is safe, as this serialization can't fail
            let bytes = bincode_opts().serialize(&command).unwrap();
            let res = stream.send(Message::Binary(bytes)).await;
            if res.is_err() {
                error!("Failed to ident, closing stream");
                *shutdown.write().await = true;
                return Err(NetworkError::IdentityHandshake);
            }
            trace!("Ident successful");
            Some(r)
        } else {
            None
        };
        async_spawn(async move {
            trace!("Entering setup");
            #[cfg(feature="async-std-executor")]
            let (mut ws_sink, ws_stream) = stream.split();
            #[cfg(feature="tokio-executor")]
            let (mut ws_sink, ws_stream) = stream.into_split();

            #[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
            compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}

            let ws_stream = ws_stream.map(|x| match x {
                Ok(x) => Combo::Message(x),
                Err(x) => Combo::Error(x),
            });
            let ob_stream =  r_outbound
                .into_stream()
                .map(Combo::Command);
            let mut combined_stream = futures::stream::select(ws_stream,ob_stream);
            debug!("Entering processing loop");
            while let Some(m) = combined_stream.next().await {
                // Check for shutdown signal
                if *shutdown.read().await {
                    info!("Received shutdown");
                    break;
                }
                match m {
                    Combo::Message(m) => {
                        trace!(?m, "Incoming websockets message");
                        // Update the message timer
                        // Do this inside a block to make sure the lock doesn't leak
                        {
                            let mut lock = last_message.lock().await;
                            *lock = Instant::now();
                        }
                        // Attempt to decode the message
                        match m {
                            Message::Binary(vec) => {
                                trace!(?vec, "Attempting to decode binary message");
                                let res: Result<Command<TYPES,LEAF,PROPOSAL>, _> = bincode_opts().deserialize(&vec);
                                match res {
                                    Ok(command) => {
                                        match w.process_command(command, &inputs).await {
                                            Ok(Some(command)) => match command {
                                                Command::Identify { from, id } => {
                                                    debug!("Identity received");
                                                    // Identifying twice isn't an error, but
                                                    // repeated identifies are ignored
                                                    let pk_s = pk_s.take();
                                                    if let Some(pk_s) = pk_s {
                                                        if pk_s.send(from).is_err() {
                                                            error!("Listener is gone, closing stream");
                                                            *shutdown.write().await = true;
                                                            break;
                                                        }
                                                    }
                                                    trace!("Acking identify");
                                                    let command =
                                                        Command::<TYPES,LEAF,PROPOSAL>::Ack{
                                                            ack_id: id,
                                                            id: w.get_next_message_id()
                                                        };
                                                    // Unwrap is safe, as this serialization can't
                                                    // fail
                                                    let bytes = bincode_opts()
                                                        .serialize(&command)
                                                        .unwrap();
                                                    let res = ws_sink.send(Message::Binary(bytes)).await;
                                                    if res.is_err() {
                                                        error!("Failed to ack, closing stream");
                                                        *shutdown.write().await = true;
                                                        break;
                                                    }
                                                },
                                                Command::Ping { id } => {
                                                    debug!("Received ping, acking");
                                                    let command =
                                                        Command::<TYPES,LEAF,PROPOSAL>::Ack{
                                                            ack_id: id,
                                                            id: w.get_next_message_id()
                                                        };
                                                    // Unwrap is safe, as this serialization can't
                                                    // fail
                                                    let bytes = bincode_opts()
                                                        .serialize(&command)
                                                        .unwrap();
                                                    let res = ws_sink.send(Message::Binary(bytes)).await;
                                                    if res.is_err() {
                                                        error!("Failed to ack, closing stream");
                                                        *shutdown.write().await = true;
                                                        break;
                                                    }
                                                },
                                                _ => {
                                                    error!("Command was invalidly passed to us");
                                                    error!("In an invalid state, closing stream.");
                                                    *shutdown.write().await = true;
                                                    break;
                                                }
                                            },
                                            Ok(None) => trace!("Processed command"),
                                            Err(e) => warn!(?e, "Error processing command, skipping"),
                                        }
                                    },
                                    Err(e) => warn!(?vec,?e, "Error deserializing message, skipping"),
                                }
                            },
                            Message::Close(c) => {
                                // Log and close
                                info!(?c, "Received close message, closing stream.");
                                *shutdown.write().await = true;
                                break;
                            },
                            m => warn!(?m, "Received unsupported message type, ignoring")
                        }
                    },
                    Combo::Command(c) => {
                        trace!(?c, "Sending command");
                        // serializing
                        let bytes = bincode_opts()
                            .serialize(&c)
                            .expect_or_log("Failed to serialize a command. Having types that can fail serialization is not supported.");
                        // Sending down the pipe
                        trace!("Sending serialized command");
                        let res = ws_sink.send(Message::Binary(bytes)).await;
                        match res {
                            Ok(_) => {
                                // Log and notify the water if there is any
                                trace!("Message fed to stream");
                                let waiter = &w.inner.waiters.delivered;
                                if waiter.contains_key(&c.id()) {
                                    // Unwrap is safe, as we just verified the key exists
                                    let (_, oneshot) = waiter.remove(&c.id()).unwrap();
                                    let res = oneshot.send(());
                                    if res.is_err() {
                                        warn!("Failed to message waiter for message {}", c.id());
                                    }
                                }
                            },
                            Err(e) => {
                                // log error and shutdown
                                error!(?e, "Error sending message to remote, closing stream.");
                                *shutdown.write().await = true;
                                break;
                            },
                        }
                    },
                    Combo::Error(e) => {
                        // log the error and close the stream
                        error!(?e, "A websockets error occurred! Closing stream.");
                        // Note the shutdown status and break
                        *shutdown.write().await = true;
                        break;
                    },
                }
            }
        }.instrument(tracing::info_span!("Background Stream Handler",
                                      self.socket = ?self.inner.socket,
                                      other.node_id = ?key,
                                         other.socket = ?remote_socket)));
        trace!("Task spawned");

        if let Some(pk) = key {
            if let Some(waiter_ident) = waiter_ident {
                trace!("Waiting for remote to ack the ident");
                waiter_ident.await.unwrap();
                trace!("Remote acked");
            }
            Ok((pk, handle))
        } else {
            let pk = pk_r.await.map_err(|_| NetworkError::IdentityHandshake)?;
            Ok((pk, handle))
        }
    }
    /// Creates a connection to the given node.
    ///
    /// If the connection does not succeed immediately, pause and retry. Use
    /// `connection_table_size()` to get the number of connected nodes.
    ///
    /// # Errors
    ///
    /// Will error if an underlying networking error occurs
    #[instrument(name = "WNetwork::connect_to", skip(self), err)]
    pub async fn connect_to(
        &self,
        key: TYPES::SignatureKey,
        addr: impl ToSocketAddrs + Debug,
    ) -> Result<(), NetworkError> {
        // First check to see if we have the node in the map
        if self.inner.handles.contains_key(&key) {
            debug!(?key, "Already have a connection to node");
            Ok(())
        } else {
            let socket = TcpStream::connect(addr).await.context(ExecutorSnafu)?;
            let addr = socket.peer_addr().context(SocketDecodeSnafu {
                input: "connect_to",
            })?;
            info!(?addr, "Connecting to remote with decoded address");
            let url = format!("ws://{}", addr);
            trace!(?url);
            let (web_socket, _) = client_async(url, socket).await.context(WebSocketSnafu)?;
            trace!("Websocket connection created");
            let (pub_key, handle) = self.spawn_task(Some(key), web_socket, addr).await?;
            trace!("Task created");
            self.inner.handles.insert(pub_key, handle);
            trace!("Handle noted");
            Ok(())
        }
    }
    /// Sends a raw message to the specified node
    ///
    /// # Errors
    ///
    /// Will error if an underlying network error occurs
    #[instrument(level = "trace", name = "WNetwork::send_raw_message", err, skip(self))]
    #[allow(dead_code)]
    async fn send_raw_message(
        &self,
        node: &TYPES::SignatureKey,
        message: Command<TYPES, LEAF, PROPOSAL>,
    ) -> Result<(), NetworkError> {
        let handle = &self.inner.handles.get(node);
        if let Some(handle) = handle {
            let res = handle.outbound.send(message).await;
            match res {
                Ok(_) => Ok(()),
                Err(_) => Err(NetworkError::CouldNotDeliver),
            }
        } else {
            Err(NetworkError::NoSuchNode)
        }
    }

    /// Creates a new [`WNetwork`] preloaded with connections to the nodes in `node_list`
    ///
    /// # Errors
    ///
    /// Will error if an underlying networking error occurs
    #[instrument(level = "trace", name = "WNetwork::new_from_strings", err)]
    pub async fn new(
        own_key: TYPES::SignatureKey,
        listen_addr: &str,
        port: u16,
        keep_alive_duration: Option<Duration>,
    ) -> Result<Self, NetworkError> {
        let (s_direct, r_direct) = bounded(128);
        let (s_broadcast, r_broadcast) = bounded(128);
        let keep_alive_duration = keep_alive_duration.unwrap_or_else(|| Duration::from_millis(500));
        trace!("Created queues");
        let s_string = format!("{}:{}", listen_addr, port);
        let s_addr = match s_string.to_socket_addrs().await {
            Ok(mut x) => x.next().context(NoSocketsSnafu { input: s_string })?,
            Err(e) => {
                return Err(NetworkError::SocketDecodeError {
                    input: s_string,
                    source: e,
                })
            }
        };
        info!(?s_addr, "Binding socket");
        let listener = TcpListener::bind(&s_addr)
            .await
            .context(FailedToBindListenerSnafu)?;
        debug!("Successfully bound socket");

        let (network_change_input, network_change_output) = unbounded();

        let inner = WNetworkInner {
            is_ready: Arc::new(AtomicBool::new(true)),
            handles: DashMap::new(),
            pub_key: own_key,
            counter: Arc::new(AtomicU64::new(0)),
            socket: s_addr,
            waiters: Waiters {
                delivered: DashMap::new(),
                acked: DashMap::new(),
            },
            inputs: Inputs {
                broadcast: s_broadcast,
                direct: s_direct,
            },
            outputs: Outputs {
                broadcast: Arc::new(Mutex::new(r_broadcast)),
                direct: Arc::new(Mutex::new(r_direct)),
            },
            tasks_started: AtomicBool::new(false),
            socket_holder: Mutex::new(Some(listener)),
            keep_alive_duration,
            network_change_input,
            network_change_output,
        };
        let w = Self {
            inner: Arc::new(inner),
        };
        trace!("Self constructed");
        Ok(w)
    }

    /// Generates the background processing task
    ///
    /// Will only generate the task once, subsequent calls will return `None`
    ///
    /// # Panics
    ///
    /// Will panic if the
    #[instrument(skip(self, sync))]
    pub fn generate_task(
        &self,
        sync: oneshot::Sender<()>,
    ) -> Option<Vec<BoxFuture<'static, Result<(), NetworkError>>>> {
        let generated = self
            .inner
            .tasks_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .unwrap_or(true);
        if generated {
            warn!("Task already generated, returning nothing");
            None
        } else {
            trace!("Creating tasks");
            let w = self.clone();
            /*
            Create the listener background task

            This task is responsible for accepting incoming connections.
            */
            let listener_future = async move {
                debug!("Launching server");
                // Unwrap is safe due to atomic guard
                let listener: TcpListener = w.inner.socket_holder.lock().await.take().unwrap();
                trace!("Acquired socket");
                let mut incoming = listener.incoming();
                // Port is open, send signal
                sync.send(())
                    .expect_or_log("Failed to send port alive sync signal");
                // Loop over inbound connections and open tasks for them
                while let Some(stream) = incoming.next().await {
                    debug!("Processing incoming connection");
                    match stream {
                        Ok(stream) => {
                            let addr = stream.peer_addr().unwrap();
                            trace!(?addr, "Connected incoming stream");
                            let ws_stream = accept_async(stream).await;
                            match ws_stream {
                                Ok(ws_stream) => {
                                    trace!(?addr, "stream accepted");
                                    let res: Result<
                                        (TYPES::SignatureKey, Handle<TYPES, LEAF, PROPOSAL>),
                                        _,
                                    > = w.spawn_task(None, ws_stream, addr).await;
                                    match res {
                                        Ok((pub_key, handle)) => {
                                            trace!(?addr, "Spawned task for stream");
                                            w.inner.handles.insert(pub_key.clone(), handle);
                                            w.inner
                                                .network_change_input
                                                .send(NetworkChange::NodeConnected(pub_key))
                                                .await
                                                .unwrap();
                                            trace!(?addr, "Stored handle for stream");
                                        }
                                        Err(e) => error!(
                                            ?e,
                                            ?addr,
                                            "Error spawning task for incoming stream"
                                        ),
                                    }
                                }
                                Err(e) => warn!(
                                    ?e,
                                    ?addr,
                                    "Error accepting incoming connection, ignoring."
                                ),
                            }
                        }
                        Err(e) => warn!(?e, "Failed to connect incoming stream, ignoring"),
                    }
                }
                unreachable!()
            };
            let w = self.clone();
            // Create the patrol background task
            //
            // This task is responsible for checking each task to make sure that the timeout is not
            // exceeded, sending a ping, and removing the task from the pool if no response is
            // received.
            let patrol_future = async move {
                let sleep_dur = w.inner.keep_alive_duration;
                loop {
                    trace!("going to sleep");
                    // Sleep for timeout duration.
                    // We don't bother checking if we have slept the correct amount of time, since
                    // it doesn't really matter in this case. Patrolling for stale nodes _too_
                    // frequently won't really hurt.
                    async_sleep(sleep_dur).await;
                    debug!("Patrol task woken up");
                    // Get a copy of all the handles
                    let handles: Vec<_> = w
                        .inner
                        .handles
                        .iter()
                        .map(|x| (x.key().clone(), x.value().clone()))
                        .collect();
                    trace!("Handles collected");
                    // Get current instant
                    let now = Instant::now();
                    trace!(?now);
                    // Loop through the handles
                    for (pub_key, handle) in handles {
                        trace!("Checking handle {:?}", handle.remote_socket);
                        // Get the last message time inside a block, to make sure we don't hold the
                        // lock for longer than needed
                        let last_message_time = { *handle.last_message.lock().await };
                        let duration = now.checked_duration_since(last_message_time);
                        if let Some(duration) = duration {
                            trace!(?handle.remote_socket, "Grabbed duration");
                            if duration >= sleep_dur {
                                debug!(?handle.remote_socket, ?duration, "Remote has gone stale, pinging");
                                let w = w.clone();
                                async_spawn(async move {
                                    w.ping_remote(pub_key, handle).await;
                                });
                            } else {
                                trace!(?handle.remote_socket, ?duration, "Remote has recent message");
                            }
                        } else {
                            trace!(?handle.remote_socket, "Last message was after we started patrol");
                        }
                    }
                }
            };
            Some(vec![
                listener_future
                    .instrument(info_span!("WNetwork Server",
                                        addr = ?self.inner.socket))
                    .boxed(),
                patrol_future
                    .instrument(info_span!("WNetwork Patrol",
                                           addr = ?self.inner.socket
                    ))
                    .boxed(),
            ])
        }
    }
    /// Returns the size of the internal connection table
    pub async fn connection_table_size(&self) -> usize {
        self.inner.handles.len()
    }
    /// Pings a remote, removing the remote from the handles table if the ping fails
    #[instrument(skip(self, handle))]
    async fn ping_remote(
        &self,
        remote: TYPES::SignatureKey,
        handle: Handle<TYPES, LEAF, PROPOSAL>,
    ) {
        let _ = &handle;
        trace!("Packing up ping command");
        let id = self.get_next_message_id();
        let command = Command::Ping { id };
        trace!("Registering ack waiter");
        let (send, recv) = oneshot::channel();
        if let Some(other_waiter) = self.inner.waiters.acked.insert(id, send) {
            warn!("Signaling other waiter for ack");
            let _ = other_waiter.send(());
        }
        trace!("Waiter inserted");
        let res = handle.outbound.send(command).await;
        if res.is_ok() {
            debug!("Ping sent to remote");
            let duration = self.inner.keep_alive_duration;
            if let Ok(Ok(_)) = async_timeout(duration, recv).await {
                debug!("Received ping from remote");
            } else {
                error!("Remote did not respond in time! Removing from node map");
                self.inner.handles.remove(&remote);
                self.inner
                    .network_change_input
                    .send(NetworkChange::NodeDisconnected(remote))
                    .await
                    .unwrap();
            }
        } else {
            error!("Handle has been shutdown! Removing from node map");
            self.inner.handles.remove(&remote);
            self.inner
                .network_change_input
                .send(NetworkChange::NodeDisconnected(remote))
                .await
                .unwrap();
        }
    }
}

#[async_trait]
impl<
        TYPES: NodeTypes,
        LEAF: LeafType<NodeType = TYPES>,
        PROPOSAL: ProposalType<NodeTypes = TYPES>,
    > NetworkingImplementation<TYPES, LEAF, PROPOSAL> for WNetwork<TYPES, LEAF, PROPOSAL>
{
    #[instrument(name = "WNetwork::ready")]
    async fn ready(&self) -> bool {
        while !self.inner.is_ready.load(Ordering::Relaxed) {
            async_sleep(Duration::from_millis(1)).await;
        }
        true
    }

    #[instrument(name = "WNetwork::broadcast_message")]
    async fn broadcast_message(
        &self,
        message: HotShotMessage<TYPES, LEAF, PROPOSAL>,
    ) -> Result<(), super::NetworkError> {
        debug!(?message, "Broadcasting message");
        // As a stop gap solution to be able to simulate network faults, this method will
        // collect all the erros encountered during execution, completing all the completeable
        // requests, before returning an error
        let mut errors = vec![];
        // Visit each handle in the map
        for x in self.inner.handles.iter() {
            // "Destruct" the RefMulti
            let (key, handle) = x.pair();
            trace!(?key, "Attempting to message remote");
            // Flag an error if this handle has shut down
            if *handle.shutdown.read().await {
                warn!(?key, "Handle to remote node shut down");
                errors.push(NetworkError::CouldNotDeliver);
            }
            // Pack up the message into a command
            let id = self.get_next_message_id();
            let command = Command::Broadcast {
                inner: message.clone(),
                from: self.inner.pub_key.clone(),
                id,
            };
            trace!(?command, "Packed up command");
            // send message down pipe
            let network_result = handle.outbound.send(command).await;
            if let Err(e) = network_result {
                warn!(?e, "Failed to message remote node");
            } else {
                trace!("Command sent to task");
            }
        }
        // Return the first error, if any
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.remove(0))
        }
    }

    #[instrument(name = "WNetwork::message_node")]
    async fn message_node(
        &self,
        message: HotShotMessage<TYPES, LEAF, PROPOSAL>,
        recipient: TYPES::SignatureKey,
    ) -> Result<(), super::NetworkError> {
        debug!(?message, "Messaging node");
        // Attempt to locate node
        if let Some(h) = self.inner.handles.get(&recipient) {
            trace!("Handle found");
            let handle = h.value();
            // Flag an error if this handle was shut down
            if *handle.shutdown.read().await {
                error!(?recipient, "Handle to remote node shut down");
                return Err(NetworkError::CouldNotDeliver);
            }
            // Pack up the message into a command
            let id = self.get_next_message_id();
            let command = Command::Direct {
                inner: message,
                from: self.inner.pub_key.clone(),
                to: recipient,
                id,
            };
            trace!(?command, "Packed up command");
            // Send the message down the pipe
            handle
                .outbound
                .send(command)
                .await
                .ok()
                .context(CouldNotDeliverSnafu)?;
            trace!("Command sent to task");
            Ok(())
        } else {
            error!(?message, ?recipient, "Node did not exist");
            Err(NetworkError::NoSuchNode)
        }
    }

    #[instrument(name = "WNetwork::broadcast_queue")]
    async fn broadcast_queue(
        &self,
    ) -> Result<Vec<HotShotMessage<TYPES, LEAF, PROPOSAL>>, super::NetworkError> {
        self.inner
            .outputs
            .broadcast
            .lock()
            .await
            .drain_at_least_one()
            .await
            .context(ChannelDisconnectedSnafu)
    }

    #[instrument(name = "WNetwork::next_broadcast")]
    async fn next_broadcast(
        &self,
    ) -> Result<HotShotMessage<TYPES, LEAF, PROPOSAL>, super::NetworkError> {
        self.inner
            .outputs
            .broadcast
            .lock()
            .await
            .recv()
            .await
            .map_err(|_| NetworkError::ShutDown)
    }

    #[instrument(name = "WNetwork::direct_queue")]
    async fn direct_queue(
        &self,
    ) -> Result<Vec<HotShotMessage<TYPES, LEAF, PROPOSAL>>, super::NetworkError> {
        self.inner
            .outputs
            .direct
            .lock()
            .await
            .drain_at_least_one()
            .await
            .context(ChannelDisconnectedSnafu)
    }

    #[instrument(name = "WNetwork::next_direct")]
    async fn next_direct(
        &self,
    ) -> Result<HotShotMessage<TYPES, LEAF, PROPOSAL>, super::NetworkError> {
        self.inner
            .outputs
            .direct
            .lock()
            .await
            .recv()
            .await
            .map_err(|_| NetworkError::ShutDown)
    }

    async fn known_nodes(&self) -> Vec<TYPES::SignatureKey> {
        self.inner.handles.iter().map(|x| x.key().clone()).collect()
    }

    #[instrument(name = "WNetwork::direct_queue")]
    async fn network_changes(
        &self,
    ) -> Result<Vec<NetworkChange<TYPES::SignatureKey>>, NetworkError> {
        self.inner
            .network_change_output
            .drain_at_least_one()
            .await
            .context(ChannelDisconnectedSnafu)
    }

    async fn shut_down(&self) {
        // TODO (vko):  I think shutting down the `TcpListener` will shut down this network, but I'm not sure
        // I'll need some proper test cases
        unimplemented!("WNetwork: shut_down not supported");
    }

    async fn put_record(
        &self,
        _key: impl Serialize + Send + Sync + 'static,
        _value: impl Serialize + Send + Sync + 'static,
    ) -> Result<(), NetworkError> {
        unimplemented!("WNetwork: put_record not supported");
    }

    async fn get_record<V: for<'a> Deserialize<'a>>(
        &self,
        _key: impl Serialize + Send + Sync + 'static,
    ) -> Result<V, NetworkError> {
        unimplemented!("WNetwork: get_record not supported");
    }

    async fn notify_of_subsequent_leader(
        &self,
        _pk: TYPES::SignatureKey,
        _is_cancelled: Arc<AtomicBool>,
    ) {
        // do nothing
    }
}

/// Tries to get a networking implementation with the given id
///
/// also starts the background task
/// # Panics
/// panics if unable to generate tasks
#[allow(clippy::panic)]
#[instrument(skip(rng))]
async fn get_networking<
    TYPES: NodeTypes,
    LEAF: LeafType<NodeType = TYPES>,
    PROPOSAL: ProposalType<NodeTypes = TYPES>,
    R: rand::Rng,
>(
    pub_key: TYPES::SignatureKey,
    listen_addr: &str,
    node_id: u64,
    rng: &mut R,
) -> (WNetwork<TYPES, LEAF, PROPOSAL>, u16, TYPES::SignatureKey) {
    debug!(?pub_key);
    for _attempt in 0..50 {
        let port: u16 = rng.gen_range(10_000..50_000);
        let x = WNetwork::new(pub_key.clone(), listen_addr, port, None).await;
        if let Ok(x) = x {
            let (c, sync) = futures::channel::oneshot::channel();
            match x.generate_task(c) {
                Some(task) => {
                    for task in task {
                        async_spawn(task);
                    }
                    sync.await.expect("sync.await failed");
                }
                None => {
                    panic!("Failed to launch networking task");
                }
            }
            return (x, port, pub_key);
        }
    }
    panic!("Failed to open a port");
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use std::marker::PhantomData;

    use super::*;
    use crate::{
        demos::dentry::{DEntryBlock, DEntryState, DEntryTransaction},
        traits::election::static_committee::{StaticElectionConfig, StaticVoteToken},
    };
    use async_compatibility_layer::{art::async_sleep, logging::setup_logging};
    use hotshot_types::{
        data::ViewNumber,
        traits::{
            node_implementation::ApplicationMetadata,
            signature_key::ed25519::{Ed25519Priv, Ed25519Pub},
        },
    };
    use rand::Rng;

    /// application metadata stub
    #[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
    pub struct TestMetaData {}

    impl ApplicationMetadata for TestMetaData {}

    #[derive(
        Copy,
        Clone,
        Debug,
        Default,
        Hash,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        serde::Serialize,
        serde::Deserialize,
    )]
    struct Test {
        message: u64,
    }
    impl NodeTypes for Test {
        // TODO (da) can this be SequencingConsensus?
        type ConsensusType = ValidatingConsensus;
        type Time = ViewNumber;
        type BlockType = DEntryBlock;
        type SignatureKey = Ed25519Pub;
        type VoteTokenType = StaticVoteToken<Ed25519Pub>;
        type Transaction = DEntryTransaction;
        type ElectionConfigType = StaticElectionConfig;
        type StateType = DEntryState;
        type ApplicationMetadataType = TestMetaData;
    }

    #[instrument]
    async fn get_wnetwork<
        LEAF: LeafType<NodeType = Test>,
        PROPOSAL: ProposalType<NodeTypes = Test>,
    >() -> (Ed25519Pub, WNetwork<Test, LEAF, PROPOSAL>, u16) {
        let mut rng = rand::thread_rng();
        let nonce: u64 = rng.gen();
        debug!(?nonce, "Generating PubKey with id");
        let priv_key = Ed25519Priv::generate();
        let pub_key = Ed25519Pub::from_private(&priv_key);
        for _ in 0..10 {
            let port: u16 = rng.gen_range(3000..8000);
            debug!(?port, "Attempting port");
            let res = WNetwork::new(pub_key, "localhost", port, None).await;
            if let Ok(n) = res {
                return (pub_key, n, port);
            }
            warn!(?port, "Port opening failed");
        }
        panic!("Failed to generate a connection");
    }

    #[instrument]
    async fn get_wnetwork_timeout<
        LEAF: LeafType<NodeType = Test>,
        PROPOSAL: ProposalType<NodeTypes = Test>,
    >(
        timeout: u64,
    ) -> (Ed25519Pub, WNetwork<Test, LEAF, PROPOSAL>, u16) {
        let timeout = Duration::from_millis(timeout);
        let mut rng = rand::thread_rng();
        let nonce: u64 = rng.gen();
        debug!(?nonce, "Generating PubKey with id");
        let priv_key = Ed25519Priv::generate();
        let pub_key = Ed25519Pub::from_private(&priv_key);
        for _ in 0..10 {
            let port: u16 = rng.gen_range(3000..8000);
            debug!(?port, "Attempting port");
            let res = WNetwork::new(pub_key, "localhost", port, Some(timeout)).await;
            if let Ok(n) = res {
                return (pub_key, n, port);
            }
            warn!(?port, "Port opening failed");
        }
        panic!("Failed to generate a connection");
    }

    struct WNetworkTask<ELECTION: Election<Test>> {
        pd: PhantomData<ELECTION>,
    }

    // TODO (da) async-std-executor attribute was moved to fix compiler error.
    impl<ELECTION: Election<Test>> WNetworkTask<ELECTION> {
        // Generating the tasks should once and only once
        #[cfg_attr(
            feature = "tokio-executor",
            tokio::test(flavor = "multi_thread", worker_threads = 2)
        )]
        async fn task_only_once() {
            setup_logging();
            let (_key, network, _port) =
                get_wnetwork::<ValidatingLeaf<Test>, ValidatingProposal<Test, ELECTION>>().await;
            let (sync, _r) = oneshot::channel();
            let x = network.generate_task(sync);
            let (sync, _r) = oneshot::channel();
            let y = network.generate_task(sync);
            assert!(x.is_some());
            assert!(y.is_none());
        }

        // Spawning a single WNetwork and starting the task should produce no errors
        #[cfg_attr(
            feature = "tokio-executor",
            tokio::test(flavor = "multi_thread", worker_threads = 2)
        )]
        async fn spawn_single() {
            setup_logging();
            let (_key, network, _port) =
                get_wnetwork::<ValidatingLeaf<Test>, ValidatingProposal<Test, ELECTION>>().await;
            let (sync, r) = oneshot::channel();
            let x = network
                .generate_task(sync)
                .expect("Failed to generate task");
            for x in x {
                async_spawn(x);
            }
            r.await.unwrap();
        }

        // Spawning two WNetworks and connecting them should produce no errors
        #[cfg_attr(
            feature = "tokio-executor",
            tokio::test(flavor = "multi_thread", worker_threads = 2)
        )]
        async fn spawn_double() {
            setup_logging();
            // Spawn first wnetwork
            let (_key1, network1, _port1) =
                get_wnetwork::<ValidatingLeaf<Test>, ValidatingProposal<Test, ELECTION>>().await;
            let (sync, r) = oneshot::channel();
            let x = network1
                .generate_task(sync)
                .expect("Failed to generate task");
            for x in x {
                async_spawn(x);
            }
            r.await.unwrap();
            // Spawn second wnetwork
            let (key2, network2, port2) =
                get_wnetwork::<ValidatingLeaf<Test>, ValidatingProposal<Test, ELECTION>>().await;
            let (sync, r) = oneshot::channel();
            let x = network2
                .generate_task(sync)
                .expect("Failed to generate task");
            for x in x {
                async_spawn(x);
            }
            r.await.unwrap();
            // Connect 1 to 2
            let addr = format!("localhost:{}", port2);
            network1
                .connect_to(key2, &addr)
                .await
                .expect("Failed to connect nodes");
        }

        // // Check to make sure direct queue works
        // #[cfg_attr(
        //     feature = "tokio-executor",
        //     tokio::test(flavor = "multi_thread", worker_threads = 2)
        // )]
        // #[cfg_attr(feature = "async-std-executor" )]
        // async fn direct_queue() {
        //     setup_logging();
        //     // Create some dummy messages
        //     let messages: Vec<Test> = (0..5).map(|x| Test { message: x }).collect();

        //     // Spawn first wnetwork
        //     let (key1, network1, _port1) = get_wnetwork().await;
        //     let (sync, r) = oneshot::channel();
        //     let x = network1
        //         .generate_task(sync)
        //         .expect("Failed to generate task");
        //     for x in x {
        //         async_spawn(x);
        //     }
        //     r.await.unwrap();
        //     // Spawn second wnetwork
        //     let (key2, network2, port2) = get_wnetwork().await;
        //     let (sync, r) = oneshot::channel();
        //     let x = network2
        //         .generate_task(sync)
        //         .expect("Failed to generate task");
        //     for x in x {
        //         async_spawn(x);
        //     }
        //     r.await.unwrap();
        //     // Connect 1 to 2
        //     let addr = format!("localhost:{}", port2);
        //     network1
        //         .connect_to(key2, &addr)
        //         .await
        //         .expect("Failed to connect nodes");

        //     // Test 1 -> 2
        //     // Send messages
        //     for message in &messages {
        //         network1
        //             .message_node(message.clone(), key2)
        //             .await
        //             .expect("Failed to message node");
        //     }
        //     let mut output = Vec::new();
        //     while output.len() < messages.len() {
        //         let message = network2
        //             .next_direct()
        //             .await
        //             .expect("Failed to receive message");
        //         output.push(message);
        //     }
        //     output.sort();
        //     // Check for equality
        //     assert_eq!(output, messages);

        //     // Test 2 -> 1
        //     // Send messages
        //     for message in &messages {
        //         network2
        //             .message_node(message.clone(), key1)
        //             .await
        //             .expect("Failed to message node");
        //     }
        //     let mut output = Vec::new();
        //     while output.len() < messages.len() {
        //         let message = network1
        //             .next_direct()
        //             .await
        //             .expect("Failed to receive message");
        //         output.push(message);
        //     }
        //     output.sort();
        //     // Check for equality
        //     assert_eq!(output, messages);
        // }

        // // Check to make sure broadcast queue works
        // #[cfg_attr(
        //     feature = "tokio-executor",
        //     tokio::test(flavor = "multi_thread", worker_threads = 2)
        // )]
        // #[cfg_attr(feature = "async-std-executor" )]
        // async fn broadcast_queue() {
        //     setup_logging();
        //     // Create some dummy messages
        //     let messages: Vec<Test> = (0..5).map(|x| Test { message: x }).collect();

        //     // Spawn first wnetwork
        //     let (_key1, network1, _port1) = get_wnetwork().await;
        //     let (sync, r) = oneshot::channel();
        //     let x = network1
        //         .generate_task(sync)
        //         .expect("Failed to generate task");
        //     for x in x {
        //         async_spawn(x);
        //     }
        //     r.await.unwrap();
        //     // Spawn second wnetwork
        //     let (key2, network2, port2) = get_wnetwork().await;
        //     let (sync, r) = oneshot::channel();
        //     let x = network2
        //         .generate_task(sync)
        //         .expect("Failed to generate task");
        //     for x in x {
        //         async_spawn(x);
        //     }
        //     r.await.unwrap();
        //     // Connect 1 to 2
        //     let addr = format!("localhost:{}", port2);
        //     network1
        //         .connect_to(key2, &addr)
        //         .await
        //         .expect("Failed to connect nodes");

        //     // Test 1 -> 2
        //     // Send messages
        //     for message in &messages {
        //         network1
        //             .broadcast_message(message.clone())
        //             .await
        //             .expect("Failed to message node");
        //     }
        //     let mut output = Vec::new();
        //     while output.len() < messages.len() {
        //         let message = network2
        //             .next_broadcast()
        //             .await
        //             .expect("Failed to receive message");
        //         output.push(message);
        //     }
        //     output.sort();
        //     // Check for equality
        //     assert_eq!(output, messages);

        //     // Test 2 -> 1
        //     // Send messages
        //     for message in &messages {
        //         network2
        //             .broadcast_message(message.clone())
        //             .await
        //             .expect("Failed to message node");
        //     }
        //     let mut output = Vec::new();
        //     while output.len() < messages.len() {
        //         let message = network1
        //             .next_broadcast()
        //             .await
        //             .expect("Failed to receive message");
        //         output.push(message);
        //     }
        //     output.sort();
        //     // Check for equality
        //     assert_eq!(output, messages);
        // }

        // Check to make sure the patrol task doesn't crash anything
        #[cfg_attr(
            feature = "tokio-executor",
            tokio::test(flavor = "multi_thread", worker_threads = 2)
        )]
        async fn patrol_task() {
            setup_logging();
            // Spawn two w_networks with a timeout of 25ms
            // Spawn first wnetwork
            let (_key1, network1, _port1) = get_wnetwork_timeout::<
                ValidatingLeaf<Test>,
                ValidatingProposal<Test, ELECTION>,
            >(25)
            .await;
            let (sync, r) = oneshot::channel();
            let x = network1
                .generate_task(sync)
                .expect("Failed to generate task");
            for x in x {
                async_spawn(x);
            }
            r.await.unwrap();
            // Spawn second wnetwork
            let (key2, network2, port2) = get_wnetwork_timeout::<
                ValidatingLeaf<Test>,
                ValidatingProposal<Test, ELECTION>,
            >(25)
            .await;
            let (sync, r) = oneshot::channel();
            let x = network2
                .generate_task(sync)
                .expect("Failed to generate task");
            for x in x {
                async_spawn(x);
            }
            r.await.unwrap();
            // Connect 1 to 2
            let addr = format!("localhost:{}", port2);
            network1
                .connect_to(key2, &addr)
                .await
                .expect("Failed to connect nodes");
            // Wait 100ms to make sure that nothing crashes
            // Currently, the log output needs to be inspected to make sure that nothing bad happened
            async_sleep(Duration::from_millis(100)).await;
        }
    }
}
