pub mod config;

mod client;
mod clients;
mod runs;

pub use config::NetworkConfig;

use async_compatibility_layer::{
    art::{async_spawn, async_timeout},
    channel::{bounded, OneShotReceiver, OneShotSender, Receiver, Sender},
};
use async_trait::async_trait;
use bincode::Options;
use clients::Clients;
use config::ClientConfig;
use futures::FutureExt as _;
use hotshot_types::traits::{election::ElectionConfig, signature_key::SignatureKey};
use hotshot_utils::bincode::bincode_opts;
use runs::RoundConfig;
use snafu::ResultExt;
use std::{
    convert::TryInto,
    marker::Send,
    net::{IpAddr, SocketAddr},
    num::NonZeroUsize,
    time::Duration,
};
use tracing::{debug, error};

#[cfg(feature = "async-std-executor")]
mod types {
    pub use async_std::{
        io::{ReadExt, WriteExt},
        net::{TcpListener, TcpStream},
    };
    pub use std::net::Shutdown;
    pub use TcpStream as WriteTcpStream;
    pub use TcpStream as ReadTcpStream;
    pub trait AsyncReadStream: ReadExt + Unpin + Send {}
    pub trait AsyncWriteStream: WriteExt + Unpin + Send {}
    impl<T> AsyncReadStream for T where T: ReadExt + Unpin + Send {}
    impl<T> AsyncWriteStream for T where T: WriteExt + Unpin + Send {}
}

#[cfg(feature = "tokio-executor")]
mod types {
    pub use OwnedReadHalf as ReadTcpStream;
    pub use OwnedWriteHalf as WriteTcpStream;
    pub trait AsyncReadStream: AsyncReadExt + Unpin + Send {}
    pub trait AsyncWriteStream: AsyncWriteExt + Unpin + Send {}
    impl<T> AsyncReadStream for T where T: AsyncReadExt + Unpin + Send {}
    impl<T> AsyncWriteStream for T where T: AsyncWriteExt + Send + Unpin {}

    pub use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{tcp::OwnedReadHalf, tcp::OwnedWriteHalf, TcpListener, TcpStream},
    };
}

#[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}
use types::*;

/// 256KB, assumed to be the kernel receive buffer
/// <https://stackoverflow.com/a/2862176>
pub(crate) const MAX_CHUNK_SIZE: usize = 256 * 1024;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub enum ToServer<K> {
    GetConfig,
    Identify { key: K },
    Broadcast { message_len: u64 },
    Direct { target: K, message_len: u64 },
    RequestClientCount,
    Results(RunResults),
}

impl<K> ToServer<K> {
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

#[test]
fn run_results_can_be_serialized() {
    // validate that `RunResults` can be serialized
    let results = RunResults {
        run: Run(5),
        node_index: 6,
        transactions_submitted: 7,
        transactions_rejected: 8,
        transaction_size_bytes: 9,
        rounds_succeeded: 10,
        rounds_timed_out: 11,
        total_time_in_seconds: 12.13,
    };
    serde_json::to_string_pretty(&results).unwrap();
    toml::to_string_pretty(&results).unwrap();
}

/// the run of execution we are on
#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug, Clone, Copy, Default)]
pub struct Run(pub usize);

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum FromServer<K, ELECTIONCONFIG> {
    Config {
        config: Box<NetworkConfig<K, ELECTIONCONFIG>>,
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

impl<K, E> FromServer<K, E> {
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
pub struct FromBackground<K, E> {
    header: FromServer<K, E>,
    payload: Option<Vec<u8>>,
}

impl<K, E> FromBackground<K, E> {
    pub fn config(config: NetworkConfig<K, E>, run: Run) -> FromBackground<K, E> {
        FromBackground {
            header: FromServer::Config {
                config: Box::new(config),
                run,
            },
            payload: None,
        }
    }
    pub fn node_connected(key: K) -> FromBackground<K, E> {
        FromBackground {
            header: FromServer::NodeConnected { key },
            payload: None,
        }
    }
    pub fn node_disconnected(key: K) -> FromBackground<K, E> {
        FromBackground {
            header: FromServer::NodeDisconnected { key },
            payload: None,
        }
    }
    pub fn broadcast(
        source: K,
        message_len: u64,
        payload: Option<Vec<u8>>,
    ) -> FromBackground<K, E> {
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
    pub fn broadcast_payload(source: K, payload: Vec<u8>) -> FromBackground<K, E> {
        FromBackground {
            header: FromServer::BroadcastPayload {
                source,
                payload_len: payload.len() as u64,
            },
            payload: Some(payload),
        }
    }
    pub fn direct(source: K, message_len: u64, payload: Option<Vec<u8>>) -> FromBackground<K, E> {
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
    pub fn direct_payload(source: K, payload: Vec<u8>) -> FromBackground<K, E> {
        FromBackground {
            header: FromServer::DirectPayload {
                source,
                payload_len: payload.len() as u64,
            },
            payload: Some(payload),
        }
    }
    pub fn client_count(client_count: u32) -> FromBackground<K, E> {
        FromBackground {
            header: FromServer::ClientCount(client_count),
            payload: None,
        }
    }
    pub fn start() -> FromBackground<K, E> {
        FromBackground {
            header: FromServer::Start,
            payload: None,
        }
    }
}

pub struct Server<K: SignatureKey + 'static, E: ElectionConfig + 'static> {
    listener: TcpListener,
    shutdown: Option<OneShotReceiver<()>>,
    config: Option<RoundConfig<K, E>>,
}

impl<K: SignatureKey + 'static, E: ElectionConfig + 'static> Server<K, E> {
    /// Create a new instance of the centralized server.
    pub async fn new(host: IpAddr, port: u16) -> Self {
        let listener = TcpListener::bind((host, port))
            .await
            .expect("Could not bind to address");
        Self {
            listener,
            shutdown: None,
            config: None,
        }
    }

    /// Register a signal that can be used to shut down this server
    ///
    /// # Panics
    ///
    /// Will panic if the shutdown signal is already configured.
    pub fn with_shutdown_signal(mut self, shutdown_listener: OneShotReceiver<()>) -> Self {
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
    pub fn with_round_config(mut self, config: RoundConfig<K, E>) -> Self {
        self.config = Some(config);
        self
    }

    /// Run the server.
    ///
    /// If `with_shutdown_signal` is called before, this server will stop when that signal is called. Otherwise this server will run forever.
    pub async fn run(self) {
        let (sender, receiver) = bounded(10);

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
            shutdown_future.recv().boxed().fuse()
        } else {
            futures::future::pending().boxed().fuse()
        };

        let mut listener_fuse = self.listener.accept().boxed().fuse();
        loop {
            futures::select! {
                result = listener_fuse => {
                    match result {
                        Ok((stream, addr)) => {
                            async_spawn(client::spawn::<K, E>(addr, stream, sender.clone()));
                        },
                        Err(e) => {
                            error!("Could not accept new client: {:?}", e);
                            break;
                        }
                    }
                    listener_fuse = self.listener.accept().boxed().fuse();
                }
                _ = shutdown => {
                    debug!("Received shutdown signal");
                    break;
                }
            }
        }
        debug!("Server shutting down");
        sender
            .send(ToBackground::Shutdown)
            .await
            .expect("Could not notify background thread that we're shutting down");
        let timeout = async_timeout(Duration::from_secs(5), background_task_handle).await;

        #[cfg(feature = "async-std-executor")]
        timeout.expect("Could not join on the background thread");
        #[cfg(feature = "tokio-executor")]
        timeout
            .expect("background task timed_out")
            .expect("Could not join on the background thread");
        #[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
        compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}
    }
}

/// The "background" task. This is a central task that will wait for messages send by clients. See `src/client.rs` for the client part.
///
/// This task's purpose is to:
/// - React to incoming messages,
/// - Keep track of the clients connected,
/// - Send direct/broadcast messages to clients
async fn background_task<K: SignatureKey + 'static, E: ElectionConfig + 'static>(
    self_sender: Sender<ToBackground<K, E>>,
    mut receiver: Receiver<ToBackground<K, E>>,
    mut config: Option<RoundConfig<K, E>>,
) -> Result<(), Error> {
    let mut clients = Clients::new();
    loop {
        let msg = receiver
            .recv()
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
                    sender.send(ClientConfig::default());
                }
            }
            ToBackground::StartRun(run) => {
                error!("Starting run {run:?} ({} nodes)", clients.len(run));
                clients.broadcast(run, FromBackground::start()).await;
            }
        }
    }
}

#[derive(Debug)]
pub enum ToBackground<K, E> {
    Shutdown,
    StartRun(Run),
    NewClient {
        run: Run,
        key: K,
        sender: Sender<FromBackground<K, E>>,
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
        sender: OneShotSender<ClientConfig<K, E>>,
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
            #[cfg(feature = "async-std-executor")]
            let _ = self.stream.shutdown(Shutdown::Write);
            #[cfg(feature = "tokio-executor")]
            let _ = async_compatibility_layer::art::async_block_on(self.stream.shutdown());
            #[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
            compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}
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
            #[cfg(feature = "async-std-executor")]
            let _ = self.stream.shutdown(Shutdown::Read);
            #[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
            compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}
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
            #[cfg(feature = "async-std-executor")]
            let _ = self.stream.shutdown(Shutdown::Both);
            #[cfg(feature = "tokio-executor")]
            let _ = async_compatibility_layer::art::async_block_on(self.stream.shutdown());
            #[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
            compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}
        }
    }
}
