pub mod config;

mod client;
mod clients;
mod runs;

pub use config::NetworkConfig;

cfg_if::cfg_if! {
    if #[cfg(feature = "async-std-executor")] {
        use async_std::{
            io::{ReadExt, WriteExt},
            net::{TcpListener, TcpStream},
        };
        use std::net::Shutdown;
    } else if #[cfg(feature = "tokio-executor")] {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::{tcp::OwnedReadHalf, tcp::OwnedWriteHalf, TcpListener, TcpStream},
        };
    } else {
        std::compile_error!{"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."};
    }
}
use bincode::Options;
use clients::Clients;
use config::ClientConfig;
use flume::{Receiver, Sender};
use futures::FutureExt as _;
use hotshot_types::traits::signature_key::SignatureKey;
use hotshot_utils::bincode::bincode_opts;
use runs::RoundConfig;
use hotshot_types::{traits::signature_key::EncodedPublicKey, HotShotConfig};
use hotshot_types::{traits::signature_key::SignatureKey, ExecutionType};
use hotshot_utils::{
    art::{async_spawn, async_timeout},
    bincode::bincode_opts,
};
use snafu::ResultExt;
use std::{
    marker::PhantomData,
    net::{IpAddr, SocketAddr, ShutDown},
    num::NonZeroUsize,
    time::Duration,
};
use tracing::{debug, error};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub enum ToServer<K> {
    GetConfig,
    Identify { key: K },
    Broadcast { message: Vec<u8> },
    Direct { target: K, message: Vec<u8> },
    RequestClientCount,
    Results(RunResults),
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct RunResults {
    pub run: Run,
    pub node_index: u64,

    pub transactions_submitted: usize,
    pub transactions_rejected: usize,
    pub transaction_size_bytes: usize,

    pub rounds_succeeded: u64,
    pub rounds_timed_out: u64,
    pub total_time_in_seconds: f64,
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug, Clone, Copy, Default)]
pub struct Run(pub usize);

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum FromServer<K> {
    Config { config: NetworkConfig<K>, run: Run },
    NodeConnected { key: K },
    NodeDisconnected { key: K },
    Broadcast { message: Vec<u8> },
    Direct { message: Vec<u8> },
    ClientCount(usize),
    Start,
}

pub struct Server<K: SignatureKey + 'static> {
    listener: TcpListener,
    shutdown: Option<Receiver<()>>,
    config: Option<RoundConfig<K>>,
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
    pub fn with_round_config(mut self, config: RoundConfig<K>) -> Self {
        self.config = Some(config);
        self
    }

    /// Run the server.
    ///
    /// If `with_shutdown_signal` is called before, this server will stop when that signal is called. Otherwise this server will run forever.
    pub async fn run(self) {
        let (sender, receiver) = flume::unbounded();

        let background_task_handle = async_spawn({
            let sender = sender.clone();
            async move {
                if let Err(e) = background_task(sender, receiver, self.config).await {
                    error!("Background processing thread encountered an error: {:?}", e);
                }
                debug!("Background thread ended");
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
                            async_spawn(client::spawn(addr, stream, sender.clone()));
                            // TODO most likely don't want this but double check to be sure.
                            // async_spawn({
                            //     let sender = sender.clone();
                            //     async move {
                            //         let mut key = None;
                            //         if let Err(e) =
                            //             spawn_client(addr, stream, &mut key, sender.clone()).await
                            //         {
                            //             debug!("Client from {:?} encountered an error: {:?}", addr, e);
                            //         } else {
                            //             debug!("Client from {:?} shut down", addr);
                            //         }
                            //         if let Some(key) = key {
                            //             let _ = sender
                            //                 .send_async(ToBackground::ClientDisconnected { key })
                            //                 .await;
                            //         }
                            //     }
                            // });
                        },
                        Err(e) => {
                            error!("Could not accept new client: {:?}", e);
                            break;
                        }
                    }
                }
                _ = shutdown => {
                    debug!("Received shutdown signal");
                    break;
                }
            }
        }
        debug!("Server shutting down");
        sender
            .send_async(ToBackground::Shutdown)
            .await
            .expect("Could not notify background thread that we're shutting down");
        let timeout = async_timeout(Duration::from_secs(5), background_task_handle).await;
        cfg_if::cfg_if! {
            if #[cfg(feature = "async-std-executor")] {
                timeout
                    .expect("Could not join on the background thread");
            } else if #[cfg(feature = "tokio-executor")] {
                timeout
                    .expect("background task timed_out")
                    .expect("Could not join on the background thread");
            } else {
                std::compile_error!{"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."};
            }
        }
    }
}

/// The "background" task. This is a central task that will wait for messages send by clients. See `src/client.rs` for the client part.
///
/// This task's purpose is to:
/// - React to incoming messages,
/// - Keep track of the clients connected,
/// - Send direct/broadcast messages to clients
async fn background_task<K: SignatureKey + 'static>(
    self_sender: Sender<ToBackground<K>>,
    receiver: Receiver<ToBackground<K>>,
    mut config: Option<RoundConfig<K>>,
) -> Result<(), Error> {
    let mut clients = Clients::new();
    loop {
        let msg = receiver
            .recv_async()
            .await
            .map_err(|_| Error::BackgroundShutdown)?;

        match msg {
            ToBackground::Shutdown => {
                debug!("Background thread shutting down");
                break Ok(());
            }
            ToBackground::NewClient { run, key, sender } => {
                // notify everyone else of the new client
                clients
                    .broadcast(run, FromServer::NodeConnected { key: key.clone() })
                    .await;
                // add the client
                clients.insert(run, key, sender);
            }
            ToBackground::ClientDisconnected { run, key } => {
                // remove the client
                clients.remove(run, key.clone());
                // notify everyone of the client disconnecting
                clients
                    .broadcast(run, FromServer::NodeDisconnected { key })
                    .await;
            }
            ToBackground::IncomingBroadcast {
                run,
                message,
                sender,
            } => {
                clients
                    .broadcast_except_self(run, sender, FromServer::Broadcast { message })
                    .await;
            }
            ToBackground::IncomingDirectMessage {
                run,
                receiver,
                message,
            } => {
                clients
                    .direct_message(run, receiver, FromServer::Direct { message })
                    .await;
            }
            ToBackground::RequestClientCount { run, sender } => {
                let client_count = clients.len(run);
                clients
                    .direct_message(run, sender, FromServer::ClientCount(client_count))
                    .await;
            }
            ToBackground::Results { results } => {
                if let Some(config) = &mut config {
                    if let Err(e) = config.add_result(results).await {
                        error!("Could not export node's config: {e:?}");
                    }
                }
            }
            ToBackground::ClientConnected { addr, sender } => {
                // This will assign the client a `NetworkConfig` and a `Run`.
                // if we have no `config`, or we're out of runs, the client will be assigned a default config and `Run(0)`
                if let Some(round_config) = &mut config {
                    round_config
                        .get_next_config(addr.ip(), sender, self_sender.clone())
                        .await;
                } else {
                    let _ = sender.send_async(ClientConfig::default()).await;
                }
            }
            ToBackground::StartRun(run) => {
                error!("Starting run {run:?} ({} nodes)", clients.len(run));
                clients.broadcast(run, FromServer::Start).await;
            }
        }
    }
}

pub enum ToBackground<K> {
    Shutdown,
    StartRun(Run),
    NewClient {
        run: Run,
        key: K,
        sender: Sender<FromServer<K>>,
    },
    ClientDisconnected {
        run: Run,
        key: K,
    },
    IncomingBroadcast {
        run: Run,
        sender: K,
        message: Vec<u8>,
    },
    IncomingDirectMessage {
        run: Run,
        receiver: K,
        message: Vec<u8>,
    },
    RequestClientCount {
        run: Run,
        sender: K,
    },
    Results {
        results: RunResults,
    },
    ClientConnected {
        addr: SocketAddr,
        sender: Sender<ClientConfig<K>>,
    },
}

#[derive(snafu::Snafu, Debug)]
pub enum Error {
    Io { source: std::io::Error },
    BackgroundShutdown,
    Disconnected,
    Decode { source: bincode::Error },
}

cfg_if::cfg_if! {
    if #[cfg(feature = "async-std-executor")] {
        use TcpStream as WriteTcpStream;
        use TcpStream as ReadTcpStream;
    } else if #[cfg(feature = "tokio-executor")] {
        use OwnedWriteHalf as WriteTcpStream;
        use OwnedReadHalf as ReadTcpStream;
    } else {
        std::compile_error!{"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."};
    }
}

/// Utility struct that wraps a `TcpStream` and prefixes all messages with a length
pub struct TcpStreamSendUtil {
    stream: WriteTcpStream,
}

/// Utility struct that wraps a `TcpStream` and expects a length before each messages
pub struct TcpStreamRecvUtil {
    stream: ReadTcpStream,
    buffer: Vec<u8>,
}

/// Utility struct that wraps a `TcpStream` and handles length prefixes bidirectionally
pub struct TcpStreamUtil {
    stream: TcpStream,
    buffer: Vec<u8>,
}

impl TcpStreamSendUtil {
    pub fn new(stream: WriteTcpStream) -> Self {
        Self { stream }
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

impl Drop for TcpStreamSendUtil {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            cfg_if::cfg_if! {
                if #[cfg(feature = "async-std-executor")] {
                    let _ = self.stream.shutdown(Shutdown::Write);
                } else if #[cfg(feature = "tokio-executor")] {
                    let _ = self.stream.shutdown();
                } else {
                    std::compile_error!{"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."};
                }
            }
        }
    }
}

impl TcpStreamRecvUtil {
    pub fn new(stream: ReadTcpStream) -> Self {
        Self {
            stream,
            buffer: Vec::new(),
        }
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
}

impl Drop for TcpStreamRecvUtil {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            cfg_if::cfg_if! {
                if #[cfg(feature = "async-std-executor")] {
                    let _ = self.stream.shutdown(Shutdown::Read);
                }
            }
        }
    }
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
            cfg_if::cfg_if! {
                if #[cfg(feature = "async-std-executor")] {
                    let _ = self.stream.shutdown(Shutdown::Both);
                } else if #[cfg(feature = "tokio-executor")] {
                    let _ = self.stream.shutdown();
                } else {
                    std::compile_error!{"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."};
                }
            }
        }
    }
}
