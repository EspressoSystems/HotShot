use async_std::{
    io::{ReadExt, WriteExt},
    net::{TcpListener, TcpStream},
    prelude::FutureExt,
};
use bincode::Options;
use flume::{Receiver, Sender};
use futures::FutureExt as _;
use hotshot_types::{traits::signature_key::EncodedPublicKey, HotShotConfig};
use hotshot_types::{traits::signature_key::SignatureKey, ExecutionType};
use hotshot_utils::bincode::bincode_opts;
use snafu::ResultExt;
use std::{
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
    net::{IpAddr, Shutdown, SocketAddr},
    num::NonZeroUsize,
    time::Duration,
};

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug, Clone)]
#[serde(bound(deserialize = ""))]
pub enum ToServer<K: SignatureKey> {
    GetConfig,
    Identify { key: K },
    Broadcast { message: Vec<u8> },
    Direct { target: K, message: Vec<u8> },
    RequestClientCount,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub enum FromServer<K> {
    Config { config: NetworkConfig<K> },
    NodeConnected { key: K },
    NodeDisconnected { key: K },
    Broadcast { message: Vec<u8> },
    Direct { message: Vec<u8> },
    ClientCount(usize),
}

pub struct Server<K: SignatureKey + 'static> {
    listener: TcpListener,
    shutdown: Option<Receiver<()>>,
    config: Option<NetworkConfig<K>>,
    _k: PhantomData<&'static K>,
}

impl<K: SignatureKey + 'static> Server<K> {
    /// Create a new instance of the centralized server.
    pub async fn new(host: IpAddr, port: u16) -> Self {
        let listener = TcpListener::bind((host, port))
            .await
            .expect("Could not bind to address");
        Self {
            listener,
            shutdown: None,
            config: None,
            _k: PhantomData,
        }
    }

    /// Register a signal that can be used to shut down this server
    ///
    /// # Panics
    ///
    /// Will panic if the shutdown signal is already configured.
    pub fn with_shutdown_signal(mut self, shutdown_listener: Receiver<()>) -> Self {
        if self.shutdown.is_some() {
            panic!("A shutdown signal is already registered and can not be registered twice");
        }
        self.shutdown = Some(shutdown_listener);
        self
    }

    /// Get the address that this server is running on
    pub fn addr(&self) -> SocketAddr {
        self.listener.local_addr().unwrap()
    }

    /// Set the network config. Setting this will allow clients to request this config when they connect to the server.
    pub fn with_network_config(mut self, config: NetworkConfig<K>) -> Self {
        self.config = Some(config);
        self
    }

    /// Run the server.
    ///
    /// If `with_shutdown_signal` is called before, this server will stop when that signal is called. Otherwise this server will run forever.
    pub async fn run(self) {
        let (sender, receiver) = flume::unbounded();

        let background_task_handle = async_std::task::spawn({
            async move {
                if let Err(e) = background_task(receiver, self.config).await {
                    eprintln!("Background processing thread encountered an error: {:?}", e);
                }
                eprintln!("Background thread ended");
            }
        });
        let shutdown_future;
        let mut shutdown = if let Some(shutdown) = self.shutdown {
            shutdown_future = shutdown;
            shutdown_future.recv_async().boxed().fuse()
        } else {
            futures::future::pending().boxed().fuse()
        };

        loop {
            futures::select! {
                result = self.listener.accept().fuse() => {
                    match result {
                        Ok((stream, addr)) => {
                            async_std::task::spawn({
                                let sender = sender.clone();
                                async move {
                                    let mut key = None;
                                    if let Err(e) =
                                        spawn_client(addr, stream, &mut key, sender.clone()).await
                                    {
                                        println!("Client from {:?} encountered an error: {:?}", addr, e);
                                    } else {
                                        println!("Client from {:?} shut down", addr);
                                    }
                                    if let Some(key) = key {
                                        let _ = sender
                                            .send_async(ToBackground::ClientDisconnected { key })
                                            .await;
                                    }
                                }
                            });
                        },
                        Err(e) => {
                            eprintln!("Could not accept new client: {:?}", e);
                            break;
                        }
                    }
                }
                _ = shutdown => {
                    eprintln!("Received shutdown signal");
                    break;
                }
            }
        }
        eprintln!("Server shutting down");
        sender
            .send_async(ToBackground::Shutdown)
            .await
            .expect("Could not notify background thread that we're shutting down");
        background_task_handle
            .timeout(Duration::from_secs(5))
            .await
            .expect("Could not join on the background thread");
    }
}

#[derive(PartialEq, Eq, Clone)]
struct OrdKey<K: SignatureKey> {
    pub key: K,
    pubkey: EncodedPublicKey,
}

impl<K: SignatureKey> From<K> for OrdKey<K> {
    fn from(key: K) -> Self {
        let pubkey = key.to_bytes();
        Self { key, pubkey }
    }
}

impl<K: SignatureKey> PartialOrd for OrdKey<K> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.pubkey.partial_cmp(&other.pubkey)
    }
}
impl<K: SignatureKey> Ord for OrdKey<K> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.pubkey.cmp(&other.pubkey)
    }
}

async fn background_task<K: SignatureKey>(
    receiver: Receiver<ToBackground<K>>,
    mut config: Option<NetworkConfig<K>>,
) -> Result<(), Error> {
    let mut clients = Clients::new();
    loop {
        let msg = receiver
            .recv_async()
            .await
            .map_err(|_| Error::BackgroundShutdown)?;

        let client_count = clients.len();
        match msg {
            ToBackground::Shutdown => {
                eprintln!("Background thread shutting down");
                break Ok(());
            }
            ToBackground::NewClient { key, sender } => {
                // notify everyone else of the new client
                clients
                    .broadcast(FromServer::NodeConnected { key: key.clone() })
                    .await;
                // add the client
                clients.insert(key.into(), sender);
            }
            ToBackground::ClientDisconnected { key } => {
                // remove the client
                clients.remove(key.clone().into());
                // notify everyone of the client disconnecting
                clients
                    .broadcast(FromServer::NodeDisconnected { key })
                    .await;
            }
            ToBackground::IncomingBroadcast { message, sender } => {
                clients
                    .broadcast_except_self(sender, FromServer::Broadcast { message })
                    .await;
            }
            ToBackground::IncomingDirectMessage { receiver, message } => {
                clients
                    .direct_message(receiver, FromServer::Direct { message })
                    .await;
            }
            ToBackground::RequestClientCount { sender } => {
                let client_count = clients.len();
                clients
                    .direct_message(sender, FromServer::ClientCount(client_count))
                    .await;
            }
            ToBackground::RequestConfig { sender } => {
                match config.as_mut() {
                    Some(config) => {
                        let clone = config.clone();
                        config.node_index += 1;
                        // the client may or may not be identified.
                        // if it is, the next message will also fail, so we won't clean it up here
                        let _ = sender.send_async(clone).await;
                    }
                    None => {
                        eprintln!("Client requested a network config but none was configured");
                    }
                }
            }
        }

        if client_count != clients.len() {
            println!("Clients connected: {}", clients.len());
        }
    }
}

struct Clients<K: SignatureKey>(BTreeMap<OrdKey<K>, Sender<FromServer<K>>>);

impl<K: SignatureKey + PartialEq> Clients<K> {
    fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub async fn broadcast(&mut self, msg: FromServer<K>) {
        let futures = futures::future::join_all(self.0.iter().map(|(id, sender)| {
            sender
                .send_async(msg.clone())
                .map(move |res| (id, res.is_ok()))
        }))
        .await;
        let keys_to_remove = futures
            .into_iter()
            .filter_map(|(id, is_ok)| if !is_ok { Some(id.clone()) } else { None })
            .collect();
        self.prune_nodes(keys_to_remove).await;
    }

    async fn broadcast_except_self(&mut self, sender_key: K, message: FromServer<K>) {
        let futures = futures::future::join_all(self.0.iter().filter_map(|(id, sender)| {
            if id.key != sender_key {
                Some(
                    sender
                        .send_async(message.clone())
                        .map(move |res| (id, res.is_ok())),
                )
            } else {
                None
            }
        }))
        .await;
        let keys_to_remove = futures
            .into_iter()
            .filter_map(|(id, is_ok)| if !is_ok { Some(id.clone()) } else { None })
            .collect();
        self.prune_nodes(keys_to_remove).await;
    }

    async fn direct_message(&mut self, receiver: K, msg: FromServer<K>) {
        let receiver = OrdKey::from(receiver);
        if let Some(sender) = self.0.get_mut(&receiver) {
            if sender.send_async(msg).await.is_err() {
                let mut tree = BTreeSet::new();
                tree.insert(receiver);
                self.prune_nodes(tree).await;
            }
        }
    }

    async fn prune_nodes(&mut self, mut clients_with_error: BTreeSet<OrdKey<K>>) {
        // While notifying the clients of other clients disconnecting, those clients can be disconnected too
        // we solve this by looping over this until we've removed all failing nodes and have successfully notified everyone else.
        while !clients_with_error.is_empty() {
            let clients_to_remove = std::mem::take(&mut clients_with_error);
            for client in &clients_to_remove {
                eprintln!("Background task could not deliver message to client thread {:?}, removing them", client.pubkey);
                self.0.remove(client);
            }
            let mut futures = Vec::with_capacity(self.0.len() * clients_to_remove.len());
            for client in clients_to_remove {
                let message = FromServer::NodeDisconnected { key: client.key };
                futures.extend(self.0.iter().map(|(id, sender)| {
                    sender
                        .send_async(message.clone())
                        .map(move |result| (id, result.is_ok()))
                }));
            }
            let results = futures::future::join_all(futures).await;
            clients_with_error = results
                .into_iter()
                .filter_map(|(id, is_ok)| if !is_ok { Some(id.clone()) } else { None })
                .collect();
        }
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn insert(&mut self, key: OrdKey<K>, sender: Sender<FromServer<K>>) {
        self.0.insert(key, sender);
    }

    fn remove(&mut self, key: OrdKey<K>) {
        self.0.remove(&key);
    }
}

async fn spawn_client<K: SignatureKey + 'static>(
    address: SocketAddr,
    stream: TcpStream,
    parent_key: &mut Option<K>,
    to_background: Sender<ToBackground<K>>,
) -> Result<(), Error> {
    let (sender, receiver) = flume::unbounded();
    async_std::task::spawn({
        let mut stream = TcpStreamUtil::new(stream.clone());
        async move {
            while let Ok(msg) = receiver.recv_async().await {
                if let Err(e) = stream.send(msg).await {
                    eprintln!("Lost connection to {:?}: {:?}", address, e);
                    break;
                }
            }
        }
    });
    let mut stream = TcpStreamUtil::new(stream);
    loop {
        let msg = stream.recv::<ToServer<K>>().await?;
        let parent_key_is_some = parent_key.is_some();
        match (msg, parent_key_is_some) {
            (ToServer::Identify { key }, false) => {
                *parent_key = Some(key.clone());
                let sender = sender.clone();
                to_background
                    .send_async(ToBackground::NewClient { key, sender })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            (ToServer::Identify { .. }, true) => {
                eprintln!("{:?} tried to identify twice", address);
            }
            (ToServer::GetConfig, _) => {
                let config = {
                    let (sender, receiver) = flume::bounded(1);
                    to_background
                        .send_async(ToBackground::RequestConfig { sender })
                        .await
                        .map_err(|_| Error::BackgroundShutdown)?;
                    receiver
                        .recv_async()
                        .await
                        .expect("Failed to retrieve config from background")
                };
                sender
                    .send_async(FromServer::Config { config })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            (_, false) => {
                eprintln!("{:?} received message but is not identified yet", address);
            }
            (ToServer::Broadcast { message }, true) => {
                let sender = parent_key.clone().unwrap();
                to_background
                    .send_async(ToBackground::IncomingBroadcast { sender, message })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            (ToServer::Direct { message, target }, true) => {
                to_background
                    .send_async(ToBackground::IncomingDirectMessage {
                        receiver: target,
                        message,
                    })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            (ToServer::RequestClientCount, true) => {
                to_background
                    .send_async(ToBackground::RequestClientCount {
                        sender: parent_key.clone().unwrap(),
                    })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
        }
    }
}

enum ToBackground<K: SignatureKey> {
    Shutdown,
    NewClient {
        key: K,
        sender: Sender<FromServer<K>>,
    },
    ClientDisconnected {
        key: K,
    },
    IncomingBroadcast {
        sender: K,
        message: Vec<u8>,
    },
    IncomingDirectMessage {
        receiver: K,
        message: Vec<u8>,
    },
    RequestClientCount {
        sender: K,
    },
    RequestConfig {
        sender: Sender<NetworkConfig<K>>,
    },
}

#[derive(snafu::Snafu, Debug)]
pub enum Error {
    Io { source: std::io::Error },
    BackgroundShutdown,
    Disconnected,
    Decode { source: bincode::Error },
}

/// Utility struct that wraps a `TcpStream` and prefixes all messages with a length
pub struct TcpStreamUtil {
    stream: TcpStream,
    buffer: Vec<u8>,
}

impl TcpStreamUtil {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            buffer: Vec::new(),
        }
    }
    pub async fn connect(addr: SocketAddr) -> std::io::Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        Ok(Self::new(stream))
    }

    pub async fn recv<M: serde::de::DeserializeOwned>(&mut self) -> Result<M, Error> {
        loop {
            if self.buffer.len() > 4 {
                let mut bytes = [0u8; 4];
                bytes.copy_from_slice(&self.buffer[..4]);
                let len = u32::from_le_bytes(bytes) as usize;
                if self.buffer.len() >= 4 + len {
                    let bytes: Vec<u8> = self.buffer.drain(..len + 4).skip(4).collect();
                    return bincode_opts().deserialize::<M>(&bytes).context(DecodeSnafu);
                }
            }
            let mut buffer = [0u8; 1024];
            let n = self.stream.read(&mut buffer).await.context(IoSnafu)?;
            if n == 0 {
                return Err(Error::Disconnected);
            }
            self.buffer.extend(&buffer[..n]);
        }
    }

    pub async fn send<M: serde::Serialize>(&mut self, m: M) -> Result<(), Error> {
        let bytes = bincode_opts()
            .serialize(&m)
            .expect("Could not serialize message");
        let len_bytes = (bytes.len() as u32).to_le_bytes();
        self.stream.write_all(&len_bytes).await.context(IoSnafu)?;
        self.stream.write_all(&bytes).await.context(IoSnafu)?;
        Ok(())
    }
}

impl Drop for TcpStreamUtil {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = self.stream.shutdown(Shutdown::Both);
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct NetworkConfig<K> {
    #[serde(default = "default_rounds")]
    pub rounds: usize,
    #[serde(default = "default_transactions_per_round")]
    pub transactions_per_round: usize,
    #[serde(default)]
    pub node_index: u64,
    #[serde(default)]
    pub seed: [u8; 32],
    #[serde(default = "default_config")]
    pub config: HotShotConfig<K>,
}

// This is hacky, blame serde for not having something like `default_value = "10"`

fn default_rounds() -> usize {
    10
}
fn default_transactions_per_round() -> usize {
    10
}
fn default_config<K>() -> HotShotConfig<K> {
    HotShotConfig {
        execution_type: ExecutionType::Continuous,
        total_nodes: NonZeroUsize::new(10).unwrap(),
        threshold: NonZeroUsize::new(7).unwrap(),
        max_transactions: NonZeroUsize::new(100).unwrap(),
        known_nodes: vec![],
        next_view_timeout: 10000,
        timeout_ratio: (11, 10),
        round_start_delay: 1,
        start_delay: 1,
        propose_min_round_time: Duration::from_secs(0),
        propose_max_round_time: Duration::from_secs(10),
        num_bootstrap: 7,
    }
}
