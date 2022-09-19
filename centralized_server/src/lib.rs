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
        std::compile_error!{"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}
    }
}
use async_trait::async_trait;
use bincode::Options;
use clients::Clients;
use config::ClientConfig;
use flume::{Receiver, Sender};
use futures::FutureExt as _;
use hotshot_types::traits::signature_key::SignatureKey;
use hotshot_utils::{
    art::{async_spawn, async_timeout},
    bincode::bincode_opts,
};
use runs::RoundConfig;
use snafu::ResultExt;
use std::{
    convert::TryInto,
    marker::{PhantomData, Send},
    net::{IpAddr, SocketAddr},
    num::NonZeroUsize,
    time::Duration,
};
use tracing::{debug, error};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub enum ToServer<K> {
    GetConfig,
    Identify { key: K },
    Broadcast { message_len: u64 },
    Direct { target: K, message_len: u64 },
    RequestClientCount,
    Results(RunResults),
}

impl<K: SignatureKey> ToServer<K> {
    pub fn payload_len(&self) -> Option<NonZeroUsize> {
        match self {
            Self::GetConfig
            | Self::Identify { .. }
            | Self::RequestClientCount
            | Self::Results(..) => None,
            Self::Broadcast { message_len } | Self::Direct { message_len, .. } => {
                (*message_len as usize).try_into().ok()
            }
        }
    }
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
    Config {
        config: NetworkConfig<K>,
        run: Run,
    },
    NodeConnected {
        key: K,
    },
    NodeDisconnected {
        key: K,
    },
    Broadcast {
        source: K,
        message_len: u64,
        payload_len: u64,
    },
    BroadcastPayload {
        source: K,
        payload_len: u64,
    },
    Direct {
        source: K,
        message_len: u64,
        payload_len: u64,
    },
    DirectPayload {
        source: K,
        payload_len: u64,
    },
    ClientCount(u32),
    Start,
}

impl<K> FromServer<K> {
    pub fn payload_len(&self) -> Option<NonZeroUsize> {
        match self {
            Self::Config { .. }
            | Self::NodeConnected { .. }
            | Self::NodeDisconnected { .. }
            | Self::ClientCount(..)
            | Self::Start => None,
            Self::Broadcast { payload_len, .. }
            | Self::BroadcastPayload { payload_len, .. }
            | Self::Direct { payload_len, .. }
            | Self::DirectPayload { payload_len, .. } => (*payload_len as usize).try_into().ok(),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct FromBackground<K> {
    header: FromServer<K>,
    payload: Option<Vec<u8>>,
}

impl<K> FromBackground<K> {
    pub fn config(config: NetworkConfig<K>, run: Run) -> FromBackground<K> {
        FromBackground {
            header: FromServer::Config { config, run },
            payload: None,
        }
    }
    pub fn node_connected(key: K) -> FromBackground<K> {
        FromBackground {
            header: FromServer::NodeConnected { key },
            payload: None,
        }
    }
    pub fn node_disconnected(key: K) -> FromBackground<K> {
        FromBackground {
            header: FromServer::NodeDisconnected { key },
            payload: None,
        }
    }
    pub fn broadcast(source: K, message_len: u64, payload: Option<Vec<u8>>) -> FromBackground<K> {
        FromBackground {
            header: FromServer::Broadcast {
                source,
                message_len,
                payload_len: if let Some(p) = &payload {
                    p.len() as u64
                } else {
                    0
                },
            },
            payload,
        }
    }
    pub fn broadcast_payload(source: K, payload: Vec<u8>) -> FromBackground<K> {
        FromBackground {
            header: FromServer::BroadcastPayload {
                source,
                payload_len: payload.len() as u64,
            },
            payload: Some(payload),
        }
    }
    pub fn direct(source: K, message_len: u64, payload: Option<Vec<u8>>) -> FromBackground<K> {
        FromBackground {
            header: FromServer::Direct {
                source,
                message_len,
                payload_len: if let Some(p) = &payload {
                    p.len() as u64
                } else {
                    0
                },
            },
            payload,
        }
    }
    pub fn direct_payload(source: K, payload: Vec<u8>) -> FromBackground<K> {
        FromBackground {
            header: FromServer::DirectPayload {
                source,
                payload_len: payload.len() as u64,
            },
            payload: Some(payload),
        }
    }
    pub fn client_count(client_count: u32) -> FromBackground<K> {
        FromBackground {
            header: FromServer::ClientCount(client_count),
            payload: None,
        }
    }
    pub fn start() -> FromBackground<K> {
        FromBackground {
            header: FromServer::Start,
            payload: None,
        }
    }
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
                std::compile_error!{"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}
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
                    .broadcast(run, FromBackground::node_connected(key.clone()))
                    .await;
                // add the client
                clients.insert(run, key, sender);
            }
            ToBackground::ClientDisconnected { run, key } => {
                // remove the client
                clients.remove(run, key.clone());
                // notify everyone of the client disconnecting
                clients
                    .broadcast(run, FromBackground::node_disconnected(key))
                    .await;
            }
            ToBackground::IncomingBroadcast {
                run,
                sender,
                message_len,
            } => {
                clients
                    .broadcast_except_self(
                        run,
                        sender.clone(),
                        FromBackground::broadcast(sender, message_len, None),
                    )
                    .await;
            }
            ToBackground::IncomingBroadcastChunk {
                run,
                sender,
                message_chunk,
            } => {
                clients
                    .broadcast_except_self(
                        run,
                        sender.clone(),
                        FromBackground::broadcast_payload(sender, message_chunk),
                    )
                    .await;
            }
            ToBackground::IncomingDirectMessage {
                run,
                sender,
                receiver,
                message_len,
            } => {
                clients
                    .direct_message(
                        run,
                        receiver,
                        FromBackground::direct(sender, message_len, None),
                    )
                    .await;
            }
            ToBackground::IncomingDirectMessageChunk {
                run,
                sender,
                receiver,
                message_chunk,
            } => {
                clients
                    .direct_message(
                        run,
                        receiver,
                        FromBackground::direct_payload(sender, message_chunk),
                    )
                    .await;
            }
            ToBackground::RequestClientCount { run, sender } => {
                let client_count = clients.len(run);
                clients
                    .direct_message(
                        run,
                        sender,
                        FromBackground::client_count(client_count as u32),
                    )
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
                clients.broadcast(run, FromBackground::start()).await;
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
        sender: Sender<FromBackground<K>>,
    },
    ClientDisconnected {
        run: Run,
        key: K,
    },
    IncomingBroadcast {
        run: Run,
        sender: K,
        message_len: u64,
    },
    IncomingDirectMessage {
        run: Run,
        sender: K,
        receiver: K,
        message_len: u64,
    },
    IncomingBroadcastChunk {
        run: Run,
        sender: K,
        message_chunk: Vec<u8>,
    },
    IncomingDirectMessageChunk {
        run: Run,
        sender: K,
        receiver: K,
        message_chunk: Vec<u8>,
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
    Io {
        source: std::io::Error,
    },
    BackgroundShutdown,
    Disconnected,
    Decode {
        source: bincode::Error,
    },
    #[snafu(display("Slice of size {slice_len} expected size {passed_len}"))]
    SizeMismatch {
        slice_len: usize,
        passed_len: usize,
    },
    #[snafu(display("Could not convert Vec of size {source_len} to array of size {target_len}"))]
    VecToArray {
        source_len: usize,
        target_len: usize,
    },
}

cfg_if::cfg_if! {
    if #[cfg(feature = "async-std-executor")] {
        use TcpStream as WriteTcpStream;
        use TcpStream as ReadTcpStream;
    } else if #[cfg(feature = "tokio-executor")] {
        use OwnedWriteHalf as WriteTcpStream;
        use OwnedReadHalf as ReadTcpStream;
    } else {
        std::compile_error!{"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}
    }
}

cfg_if::cfg_if! {
    if #[cfg(feature = "async-std-executor")] {
        fn split_stream(stream: TcpStream) -> (TcpStream, TcpStream) {
            (stream.clone(), stream)
        }
    } else if #[cfg(feature = "tokio-executor")] {
        fn split_stream(stream: TcpStream) -> (OwnedReadHalf, OwnedWriteHalf) {
            stream.into_split()
        }
    } else {
        std::compile_error!{"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}
    }
}

cfg_if::cfg_if! {
    if #[cfg(feature = "async-std-executor")] {
        pub trait AsyncReadStream: ReadExt + Unpin + Send {}
        pub trait AsyncWriteStream: WriteExt + Unpin + Send {}
        impl<T> AsyncReadStream for T where T: ReadExt + Unpin + Send {}
        impl<T> AsyncWriteStream for T where T: WriteExt + Unpin + Send {}
    } else if #[cfg(feature = "tokio-executor")] {
        pub trait AsyncReadStream: AsyncReadExt + Unpin + Send {}
        pub trait AsyncWriteStream: AsyncWriteExt + Unpin + Send {}
        impl<T> AsyncReadStream for T where T: AsyncReadExt + Unpin + Send {}
        impl<T> AsyncWriteStream for T where T: AsyncWriteExt + Send + Unpin {}
    } else {
        std::compile_error!{"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."};
    }
}

#[async_trait]
pub trait TcpStreamUtilWithRecv {
    type ReadStream: AsyncReadStream;
    fn read_stream(&mut self) -> &mut Self::ReadStream;
    // impl the recv functions against read_stream

    async fn recv<M: serde::de::DeserializeOwned>(&mut self) -> Result<M, Error> {
        // very important, usize will break if we mix native sizes.
        let len_buffer = self.recv_raw_all(std::mem::size_of::<u32>()).await?;
        let len_buffer =
            TryInto::<[u8; 4]>::try_into(len_buffer).map_err(|v| Error::VecToArray {
                source_len: v.len(),
                target_len: 4,
            })?;
        let len = u32::from_le_bytes(len_buffer) as usize;
        let bincode_buffer = self.recv_raw_all(len).await?;
        bincode_opts()
            .deserialize::<M>(&bincode_buffer)
            .context(DecodeSnafu)
    }

    async fn recv_raw_impl(&mut self, remaining: usize, recv_all: bool) -> Result<Vec<u8>, Error> {
        let mut remaining = remaining;
        let mut result = Vec::new();
        result.reserve(remaining);
        while remaining > 0 {
            let mut buffer = [0u8; 1024];
            let read_len = std::cmp::min(remaining, 1024);
            let received = self
                .read_stream()
                .read(&mut buffer[..read_len])
                .await
                .context(IoSnafu)?;
            if received == 0 {
                return Err(Error::Disconnected);
            }
            result.append(&mut buffer[..received].to_vec());
            if !recv_all && received < read_len {
                break;
            }
            remaining -= received;
        }
        Ok(result)
    }

    async fn recv_raw(&mut self, remaining: usize) -> Result<Vec<u8>, Error> {
        self.recv_raw_impl(remaining, false).await
    }

    async fn recv_raw_all(&mut self, remaining: usize) -> Result<Vec<u8>, Error> {
        self.recv_raw_impl(remaining, true).await
    }
}

#[async_trait]
pub trait TcpStreamUtilWithSend {
    type WriteStream: AsyncWriteStream;
    fn write_stream(&mut self) -> &mut Self::WriteStream;
    // impl the send functions against write_stream

    async fn send<M: serde::Serialize + Send>(&mut self, m: M) -> Result<(), Error> {
        let bytes = bincode_opts()
            .serialize(&m)
            .expect("Could not serialize message");
        let len_bytes = (bytes.len() as u32).to_le_bytes();
        self.write_stream()
            .write_all(&len_bytes)
            .await
            .context(IoSnafu)?;
        self.write_stream()
            .write_all(&bytes)
            .await
            .context(IoSnafu)?;
        Ok(())
    }

    async fn send_raw(&mut self, slice: &[u8], passed_len: usize) -> Result<(), Error> {
        if slice.len() != passed_len {
            return Err(Error::SizeMismatch {
                slice_len: slice.len(),
                passed_len,
            });
        }
        self.write_stream()
            .write_all(slice)
            .await
            .context(IoSnafu)?;
        Ok(())
    }
}

/// Utility struct that wraps a `TcpStream` and prefixes all messages with a length
pub struct TcpStreamSendUtil {
    stream: WriteTcpStream,
}

/// Utility struct that wraps a `TcpStream` and expects a length before each messages
pub struct TcpStreamRecvUtil {
    stream: ReadTcpStream,
}

/// Utility struct that wraps a `TcpStream` and handles length prefixes bidirectionally
pub struct TcpStreamUtil {
    stream: TcpStream,
}

#[async_trait]
impl TcpStreamUtilWithRecv for TcpStreamUtil {
    type ReadStream = TcpStream;
    fn read_stream(&mut self) -> &mut Self::ReadStream {
        &mut self.stream
    }
}

#[async_trait]
impl TcpStreamUtilWithSend for TcpStreamUtil {
    type WriteStream = TcpStream;
    fn write_stream(&mut self) -> &mut Self::WriteStream {
        &mut self.stream
    }
}

#[async_trait]
impl TcpStreamUtilWithRecv for TcpStreamRecvUtil {
    type ReadStream = ReadTcpStream;
    fn read_stream(&mut self) -> &mut Self::ReadStream {
        &mut self.stream
    }
}

#[async_trait]
impl TcpStreamUtilWithSend for TcpStreamSendUtil {
    type WriteStream = WriteTcpStream;
    fn write_stream(&mut self) -> &mut Self::WriteStream {
        &mut self.stream
    }
}

impl TcpStreamSendUtil {
    pub fn new(stream: WriteTcpStream) -> Self {
        Self { stream }
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
                    std::compile_error!{"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}
                }
            }
        }
    }
}

impl TcpStreamRecvUtil {
    pub fn new(stream: ReadTcpStream) -> Self {
        Self { stream }
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
        Self { stream }
    }
    pub async fn connect(addr: SocketAddr) -> std::io::Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        Ok(Self::new(stream))
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
                    std::compile_error!{"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}
                }
            }
        }
    }
}
