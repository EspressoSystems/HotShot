//! A network implementation that attempts to connect to a centralized server.
//!
//! To run the server, see the `./centralized_server/` folder in this repo.

use async_std::{net::TcpStream, sync::RwLock};
use async_trait::async_trait;
use bincode::Options;
use flume::{Receiver, Sender};
use futures::{future::BoxFuture, FutureExt};
use hotshot_centralized_server::{
    FromServer, NetworkConfig, Run, RunResults, TcpStreamUtil, ToServer,
};
use hotshot_types::traits::{
    network::{
        FailedToDeserializeSnafu, FailedToSerializeSnafu, NetworkChange, NetworkError,
        NetworkingImplementation, TestableNetworkingImplementation,
    },
    signature_key::{
        ed25519::{Ed25519Priv, Ed25519Pub},
        SignatureKey, TestableSignatureKey,
    },
};
use hotshot_utils::bincode::bincode_opts;
use serde::{de::DeserializeOwned, Serialize};
use snafu::ResultExt;
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tracing::error;

/// The inner state of the `CentralizedServerNetwork`
#[derive(Debug)]
struct Inner<K: SignatureKey> {
    /// List of all known nodes
    known_nodes: Vec<K>,
    /// `true` if the TCP stream is connected to the server
    connected: AtomicBool,
    /// `true` if the client is still running.
    running: AtomicBool,
    /// A queue of messages to be send to the server. This is emptied by `run_background`. Each message can optionally have a callback sender that will be invoked when the message is send.
    sending: Sender<(ToServer<K>, Option<Sender<()>>)>,
    /// A loopback sender that will send to `receiving`, for broadcasting to self.
    receiving_loopback: Sender<FromServer<K>>,
    /// A queue of messages to be received by this node. This is filled by `run_background`.
    receiving: Receiver<FromServer<K>>,
    /// An internal queue of messages that have been received but not yet processed.
    incoming_queue: RwLock<Vec<FromServer<K>>>,
    /// a sender used to immediately broadcast the amount of clients connected
    request_client_count_sender: RwLock<Vec<Sender<usize>>>,
}

impl<K: SignatureKey> Inner<K> {
    /// Send a broadcast mesasge to the server.
    async fn broadcast(&self, message: Vec<u8>) {
        self.sending
            .send_async((
                ToServer::Broadcast {
                    message: message.clone(),
                },
                None,
            ))
            .await
            .expect("Background thread exited");
        self.receiving_loopback.send_async(FromServer::Broadcast { message }).await.expect("Loopback exited, this should never happen because we have a reference to this receiver ourselves");
    }
    /// Send a direct message to the server.
    async fn direct_message(&self, target: K, message: Vec<u8>) {
        self.sending
            .send_async((ToServer::Direct { target, message }, None))
            .await
            .expect("Background thread exited");
    }

    /// Request the client count from the server
    async fn request_client_count(&self, sender: Sender<usize>) {
        self.request_client_count_sender.write().await.push(sender);
        self.sending
            .send_async((ToServer::RequestClientCount, None))
            .await
            .expect("Background thread exited");
    }

    /// Remove the first message from the internal queue, or the internal receiving channel, if the given `c` method returns `Some(RET)` on that entry.
    ///
    /// This will block this entire `Inner` struct until a message is found.
    async fn remove_next_message_from_queue<F, RET>(&self, c: F) -> RET
    where
        F: Fn(&FromServer<K>) -> Option<RET>,
    {
        let mut lock = self.incoming_queue.write().await;
        // remove all entries that match from `self.incoming_queue`
        let mut i = 0;
        while i < lock.len() {
            if let Some(ret) = c(&lock[i]) {
                lock.remove(i);
                return ret;
            }
            i += 1;
        }
        // pop all messages from the incoming stream, push them onto `result` if they match `c`, else push them onto our `lock`
        loop {
            let msg = self
                .receiving
                .recv_async()
                .await
                .expect("Could not receive message from centralized server queue");
            if let Some(ret) = c(&msg) {
                return ret;
            }
            lock.push(msg);
        }
    }

    /// Remove all messages from the internal queue, and then the internal receiving channel, if the given `c` method returns `Some(RET)` on that entry.
    ///
    /// This will not block, and will return 0 items if nothing is in the internal queue or channel.
    async fn remove_messages_from_queue<F, RET>(&self, c: F) -> Vec<RET>
    where
        F: Fn(&FromServer<K>) -> Option<RET>,
    {
        let mut lock = self.incoming_queue.write().await;
        let mut result = Vec::new();
        // remove all entries that match from `self.incoming_queue`
        let mut i = 0;
        while i < lock.len() {
            if let Some(ret) = c(&lock[i]) {
                lock.remove(i);
                result.push(ret);
            } else {
                i += 1;
            }
        }
        // pop all messages from the incoming stream, push them onto `result` if they match `c`, else push them onto our `lock`
        while let Ok(msg) = self.receiving.try_recv() {
            if let Some(ret) = c(&msg) {
                result.push(ret);
            } else {
                lock.push(msg);
            }
        }
        result
    }
    /// Get all the incoming broadcast messages received from the server. Returning 0 messages if nothing was received.
    async fn get_broadcasts<M: Serialize + DeserializeOwned + Send + Sync + Clone + 'static>(
        &self,
    ) -> Vec<Result<M, bincode::Error>> {
        self.remove_messages_from_queue(|msg| {
            if let FromServer::Broadcast { message } = msg {
                Some(bincode_opts().deserialize(message))
            } else {
                None
            }
        })
        .await
    }
    /// Get the next incoming broadcast message received from the server. Will lock up this struct internally until a message was received.
    async fn get_next_broadcast<M: Serialize + DeserializeOwned + Send + Sync + Clone + 'static>(
        &self,
    ) -> Result<M, bincode::Error> {
        self.remove_next_message_from_queue(|msg| {
            if let FromServer::Broadcast { message } = msg {
                Some(bincode_opts().deserialize(message))
            } else {
                None
            }
        })
        .await
    }
    /// Get all the incoming direct messages received from the server. Returning 0 messages if nothing was received.
    async fn get_direct_messages<
        M: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    >(
        &self,
    ) -> Vec<Result<M, bincode::Error>> {
        self.remove_messages_from_queue(|msg| {
            if let FromServer::Direct { message } = msg {
                Some(bincode_opts().deserialize(message))
            } else {
                None
            }
        })
        .await
    }
    /// Get the next incoming direct message received from the server. Will lock up this struct internally until a message was received.
    async fn get_next_direct_message<
        M: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    >(
        &self,
    ) -> Result<M, bincode::Error> {
        self.remove_next_message_from_queue(|msg| {
            if let FromServer::Direct { message } = msg {
                Some(bincode_opts().deserialize(message))
            } else {
                None
            }
        })
        .await
    }
    /// Get the current `NetworkChange` messages received from the server. Returning 0 messages if nothing was received.
    async fn get_network_changes(&self) -> Vec<NetworkChange<K>> {
        self.remove_messages_from_queue(|msg| match msg {
            FromServer::NodeConnected { key } => Some(NetworkChange::NodeConnected(key.clone())),
            FromServer::NodeDisconnected { key } => {
                Some(NetworkChange::NodeDisconnected(key.clone()))
            }
            _ => None,
        })
        .await
    }
}

/// Handle for connecting to a centralized server
#[derive(Clone, Debug)]
pub struct CentralizedServerNetwork<K: SignatureKey> {
    /// The inner state
    inner: Arc<Inner<K>>,
    /// An optional shutdown signal. This is only used when this connection is created through the `TestableNetworkingImplementation` API.
    server_shutdown_signal: Option<Arc<Sender<()>>>,
}

impl CentralizedServerNetwork<Ed25519Pub> {
    /// Connect with the server running at `addr` and retrieve the config from the server.
    ///
    /// The config is returned along with the current run index and the running `CentralizedServerNetwork`
    pub async fn connect_with_server_config(
        addr: SocketAddr,
    ) -> (NetworkConfig<Ed25519Pub>, Run, Self) {
        let (stream, run, config) = loop {
            let mut stream = match TcpStream::connect(addr).await {
                Ok(stream) => TcpStreamUtil::new(stream),
                Err(e) => {
                    error!("Could not connect to server: {:?}", e);
                    error!("Trying again in 5 seconds");
                    async_std::task::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };
            if let Err(e) = stream.send(ToServer::<Ed25519Pub>::GetConfig).await {
                error!("Could not request config from server: {e:?}");
                error!("Trying again in 5 seconds");
                async_std::task::sleep(Duration::from_secs(5)).await;
                continue;
            }
            match stream.recv().await {
                Ok(FromServer::Config { config, run }) => break (stream, run, config),
                x => {
                    error!("Expected config from server, got {:?}", x);
                    error!("Trying again in 5 seconds");
                    async_std::task::sleep(Duration::from_secs(5)).await;
                }
            }
        };

        let key = Ed25519Priv::generated_from_seed_indexed(config.seed, config.node_index);
        let key = Ed25519Pub::from_private(&key);
        let known_nodes = config.config.known_nodes.clone();

        let mut stream = Some(stream);

        let result = Self::create(
            known_nodes,
            move || {
                let stream = stream.take();
                async move {
                    if let Some(stream) = stream {
                        stream
                    } else {
                        Self::connect_to(addr).await
                    }
                }
                .boxed()
            },
            key,
        );
        (config, run, result)
    }

    /// Send the results for this run to the server
    pub async fn send_results(&self, results: RunResults) {
        let (sender, receiver) = flume::bounded(1);
        let _ = self
            .inner
            .sending
            .send_async((ToServer::Results(results), Some(sender)))
            .await;
        // Wait until it's successfully send before shutting down
        let _ = receiver.recv_async().await;
    }
}

impl<K: SignatureKey + 'static> CentralizedServerNetwork<K> {
    /// Connect to a given socket address. Will loop and try to connect every 5 seconds if the server is unreachable.
    fn connect_to(addr: SocketAddr) -> BoxFuture<'static, TcpStreamUtil> {
        async move {
            loop {
                match TcpStream::connect(addr).await {
                    Ok(stream) => break TcpStreamUtil::new(stream),
                    Err(e) => {
                        error!("Could not connect to server: {:?}", e);
                        error!("Trying again in 5 seconds");
                        async_std::task::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                }
            }
        }
        .boxed()
    }
    /// Connect to a centralized server
    pub fn connect(known_nodes: Vec<K>, addr: SocketAddr, key: K) -> Self {
        Self::create(known_nodes, move || Self::connect_to(addr), key)
    }

    /// Create a `CentralizedServerNetwork`. Every time a new TCP connection is needed, `create_connection` is called.
    ///
    /// This will auto-reconnect when the network loses connection to the server.
    fn create<F>(known_nodes: Vec<K>, mut create_connection: F, key: K) -> Self
    where
        F: FnMut() -> BoxFuture<'static, TcpStreamUtil> + Send + 'static,
    {
        let (to_background_sender, to_background) = flume::unbounded();
        let (from_background_sender, from_background) = flume::unbounded();
        let receiving_loopback = from_background_sender.clone();

        let inner = Arc::new(Inner {
            connected: AtomicBool::new(false),
            running: AtomicBool::new(true),
            known_nodes,
            sending: to_background_sender,
            receiving_loopback,
            receiving: from_background,
            incoming_queue: RwLock::default(),
            request_client_count_sender: RwLock::default(),
        });
        async_std::task::spawn({
            let inner = Arc::clone(&inner);
            async move {
                while inner.running.load(Ordering::Relaxed) {
                    let stream = create_connection().await;

                    if let Err(e) = run_background(
                        stream,
                        key.clone(),
                        to_background.clone(),
                        from_background_sender.clone(),
                        Arc::clone(&inner),
                    )
                    .await
                    {
                        error!(?key, ?e, "background thread exited");
                    }
                    inner.connected.store(false, Ordering::Relaxed);
                }
            }
        });
        Self {
            inner,
            server_shutdown_signal: None,
        }
    }

    /// Get the amount of clients that are connected
    pub async fn get_connected_client_count(&self) -> usize {
        let (sender, receiver) = flume::bounded(1);
        self.inner.request_client_count(sender).await;
        receiver
            .recv_async()
            .await
            .expect("Could not request client count from server")
    }
}

/// Connect to a TCP stream on address `addr`. On connection, this will send an identify with key `key`.
///
/// - All messages send to the sender of `to_background` will be send to the server.
/// - All messages received from the TCP stream, will be send to `from_background_sender`.
async fn run_background<K: SignatureKey>(
    mut stream: TcpStreamUtil,
    key: K,
    to_background: Receiver<(ToServer<K>, Option<Sender<()>>)>,
    from_background_sender: Sender<FromServer<K>>,
    connection: Arc<Inner<K>>,
) -> Result<(), Error> {
    // let mut stream = TcpStreamUtil::new(TcpStream::connect(addr).await.context(StreamSnafu)?);

    // send identify
    stream.send(ToServer::Identify { key: key.clone() }).await?;
    connection.connected.store(true, Ordering::Relaxed);

    // If we were in the middle of requesting # of clients, re-send that request
    if !connection
        .request_client_count_sender
        .read()
        .await
        .is_empty()
    {
        stream.send(ToServer::<K>::RequestClientCount).await?;
    }

    loop {
        futures::select! {
            res = stream.recv().fuse() => {
                let msg = res?;
                match msg {
                    x @ (FromServer:: NodeConnected { .. } | FromServer:: NodeDisconnected { .. } | FromServer:: Broadcast { .. } | FromServer:: Direct { .. }) => {
                        from_background_sender.send_async(x).await.map_err(|_| Error::FailedToReceive)?;
                    },

                    FromServer::ClientCount(count) => {
                        let senders = std::mem::take(&mut *connection.request_client_count_sender.write().await);
                        for sender in senders {
                            let _ = sender.try_send(count);
                        }
                    },

                    FromServer::Config { .. } => {
                        tracing::warn!("Received config from server but we're already running");
                    }
                }
            },
            result = to_background.recv_async().fuse() => {
                let (msg, confirm) = result.map_err(|_| Error::FailedToSend)?;
                stream.send(msg).await?;
                if let Some(confirm) = confirm {
                    let _ = confirm.send_async(()).await;
                }
            }
        }
    }
}

/// Inner error type for the `run_background` function.
#[derive(snafu::Snafu, Debug)]
enum Error {
    /// Generic error occured with the TCP stream
    Stream {
        /// The inner error
        source: std::io::Error,
    },
    /// Failed to receive a message on the background task
    FailedToReceive,
    /// Failed to send a message from the background task to the receiver.
    FailedToSend,
    /// Could not deserialize a message
    CouldNotDeserialize {
        /// The inner error
        source: bincode::Error,
    },
    /// We lost connection to the server
    Disconnected,
}

impl From<hotshot_centralized_server::Error> for Error {
    fn from(e: hotshot_centralized_server::Error) -> Self {
        match e {
            hotshot_centralized_server::Error::Io { source } => Self::Stream { source },
            hotshot_centralized_server::Error::Decode { source } => {
                Self::CouldNotDeserialize { source }
            }
            hotshot_centralized_server::Error::Disconnected => Self::Disconnected,
            hotshot_centralized_server::Error::BackgroundShutdown => unreachable!(), // should never be reached
        }
    }
}

#[async_trait]
impl<M, P> NetworkingImplementation<M, P> for CentralizedServerNetwork<P>
where
    M: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    P: SignatureKey + 'static,
{
    async fn ready(&self) -> bool {
        while !self.inner.connected.load(Ordering::Relaxed) {
            async_std::task::sleep(Duration::from_secs(1)).await;
        }
        true
    }

    async fn broadcast_message(&self, message: M) -> Result<(), NetworkError> {
        self.inner
            .broadcast(
                bincode_opts()
                    .serialize(&message)
                    .context(FailedToSerializeSnafu)?,
            )
            .await;
        Ok(())
    }

    async fn message_node(&self, message: M, recipient: P) -> Result<(), NetworkError> {
        self.inner
            .direct_message(
                recipient,
                bincode_opts()
                    .serialize(&message)
                    .context(FailedToSerializeSnafu)?,
            )
            .await;
        Ok(())
    }

    async fn broadcast_queue(&self) -> Result<Vec<M>, NetworkError> {
        self.inner
            .get_broadcasts()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .context(FailedToDeserializeSnafu)
    }

    async fn next_broadcast(&self) -> Result<M, NetworkError> {
        self.inner
            .get_next_broadcast()
            .await
            .context(FailedToDeserializeSnafu)
    }

    async fn direct_queue(&self) -> Result<Vec<M>, NetworkError> {
        self.inner
            .get_direct_messages()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .context(FailedToDeserializeSnafu)
    }

    async fn next_direct(&self) -> Result<M, NetworkError> {
        self.inner
            .get_next_direct_message()
            .await
            .context(FailedToDeserializeSnafu)
    }

    async fn known_nodes(&self) -> Vec<P> {
        self.inner.known_nodes.clone()
    }

    async fn network_changes(&self) -> Result<Vec<NetworkChange<P>>, NetworkError> {
        Ok(self.inner.get_network_changes().await)
    }

    async fn shut_down(&self) {
        self.inner.running.store(false, Ordering::Relaxed);
    }

    async fn put_record(
        &self,
        _key: impl serde::Serialize + Send + Sync + 'static,
        _value: impl serde::Serialize + Send + Sync + 'static,
    ) -> Result<(), NetworkError> {
        Err(NetworkError::DHTError)
    }

    async fn get_record<V: for<'a> serde::Deserialize<'a>>(
        &self,
        _key: impl serde::Serialize + Send + Sync + 'static,
    ) -> Result<V, NetworkError> {
        Err(NetworkError::DHTError)
    }
}

impl<M, P> TestableNetworkingImplementation<M, P> for CentralizedServerNetwork<P>
where
    M: Serialize + DeserializeOwned + Sync + Send + Clone + 'static,
    P: TestableSignatureKey + 'static,
{
    fn generator(
        expected_node_count: usize,
        _num_bootstrap: usize,
    ) -> Box<dyn Fn(u64) -> Self + 'static> {
        let (server_shutdown_sender, server_shutdown) = flume::bounded(1);
        let sender = Arc::new(server_shutdown_sender);

        let server = async_std::task::block_on(hotshot_centralized_server::Server::<P>::new(
            Ipv4Addr::LOCALHOST.into(),
            0,
        ))
        .with_shutdown_signal(server_shutdown);
        let addr = server.addr();
        async_std::task::spawn(server.run());

        let known_nodes = (0..expected_node_count as u64)
            .map(|id| P::from_private(&P::generate_test_key(id)))
            .collect::<Vec<_>>();

        Box::new(move |id| {
            let sender = Arc::clone(&sender);
            let mut network = CentralizedServerNetwork::connect(
                known_nodes.clone(),
                addr,
                known_nodes[id as usize].clone(),
            );
            network.server_shutdown_signal = Some(sender);
            network
        })
    }

    fn in_flight_message_count(&self) -> Option<usize> {
        None
    }
}

impl<P: SignatureKey> Drop for CentralizedServerNetwork<P> {
    fn drop(&mut self) {
        if let Some(shutdown) = self.server_shutdown_signal.take() {
            // we try to unwrap this Arc. If we're the last one with a reference to this arc, we'll be able to unwrap this
            // if we're the last one with a reference, we should send a message on this channel as it'll gracefully shut down the server
            if let Ok(sender) = Arc::try_unwrap(shutdown) {
                if let Err(e) = sender.send(()) {
                    error!("Could not notify server to shut down: {:?}", e);
                }
            }
        }
    }
}
