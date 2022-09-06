mod client;
mod clients;

use async_std::{
    fs,
    io::{ReadExt, WriteExt},
    net::{TcpListener, TcpStream},
    path::Path,
    prelude::FutureExt,
};
use bincode::Options;
use clients::Clients;
use flume::{Receiver, Sender};
use futures::FutureExt as _;
use hotshot_types::HotShotConfig;
use hotshot_types::{traits::signature_key::SignatureKey, ExecutionType};
use hotshot_utils::bincode::bincode_opts;
use snafu::ResultExt;
use std::{
    marker::PhantomData,
    net::{IpAddr, Shutdown, SocketAddr},
    num::NonZeroUsize,
    time::Duration,
};
use tracing::{debug, error};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(bound(deserialize = ""))]
pub enum ToServer<K: SignatureKey> {
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

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
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

        let background_task_handle = async_std::task::spawn({
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
                            async_std::task::spawn(client::spawn(addr, stream, sender.clone()));
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
        background_task_handle
            .timeout(Duration::from_secs(5))
            .await
            .expect("Could not join on the background thread");
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
            ToBackground::ClientConnected(sender) => {
                // This will assign the client a `NetworkConfig` and a `Run`.
                // if we have no `config`, or we're out of runs, the client will be assigned a default config and `Run(0)`
                if let Some(round_config) = &mut config {
                    let (config, run) = round_config.get_next_config().unwrap_or_default();
                    let _ = sender.send_async(ClientConfig { config, run }).await;

                    if round_config.current_run_full() {
                        // We have enough nodes to start this run, start it in 5 seconds
                        // This will allow the new client time to register itself with the server, and set up stuff
                        error!("Run {} is full, starting round in 5 seconds", run.0 + 1);
                        async_std::task::spawn({
                            let sender = self_sender.clone();
                            async move {
                                async_std::task::sleep(Duration::from_secs(5)).await;
                                let _ = sender.send_async(ToBackground::StartRun(run)).await;
                            }
                        });
                    }
                } else {
                    let _ = sender
                        .send_async(ClientConfig {
                            config: NetworkConfig::default(),
                            run: Run(0),
                        })
                        .await;
                }
            }
            ToBackground::StartRun(run) => {
                clients.broadcast(run, FromServer::Start).await;
            }
        }
    }
}

/// Contains information about the current round
pub struct RoundConfig<K> {
    configs: Vec<NetworkConfig<K>>,
    current_run: usize,
    next_node_index: usize,
}

impl<K> RoundConfig<K> {
    pub fn new(configs: Vec<NetworkConfig<K>>) -> Self {
        Self {
            configs,
            current_run: 0,
            next_node_index: 0,
        }
    }

    pub fn current_round_client_count(&self) -> usize {
        self.configs
            .get(self.current_run)
            .map(|config| config.config.total_nodes.get())
            .unwrap_or(0)
    }

    /// Will write the results for this node to `<run>/<node_index>.toml`.
    ///
    /// If the folder `<run>/` does not exist, it will be created and the config for that run will be stored in `<run>/config.toml`
    ///
    /// # Panics
    ///
    /// Will panic if serialization to TOML fails
    pub async fn add_result(&mut self, result: RunResults) -> std::io::Result<()>
    where
        K: serde::Serialize,
    {
        let run = result.run.0;
        let folder = run.to_string();
        let folder = Path::new(&folder);
        if !folder.exists().await {
            // folder does not exist, create it and copy over the network config to `config.toml`
            let config = &self.configs[run];
            fs::create_dir_all(folder).await?;
            fs::write(
                format!("{}/config.toml", run),
                toml::to_string_pretty(config).expect("Could not serialize"),
            )
            .await?;
        }
        fs::write(
            format!("{}/{}.toml", run, result.node_index),
            toml::to_string_pretty(&result).expect("Could not serialize"),
        )
        .await?;

        Ok(())
    }

    pub fn get_next_config(&mut self) -> Option<(NetworkConfig<K>, Run)>
    where
        K: Clone,
    {
        let mut config: NetworkConfig<K> = self.configs.get(self.current_run)?.clone();

        if self.next_node_index >= config.config.total_nodes.get() {
            self.next_node_index = 0;
            self.current_run += 1;

            println!(
                "Starting run {} / {}",
                self.current_run + 1,
                self.configs.len()
            );

            config = self.configs.get(self.current_run)?.clone();
        } else if self.next_node_index == 0 && self.current_run == 0 {
            println!("Starting run 1 / {}", self.configs.len());
        }
        let index = self.next_node_index;

        self.next_node_index += 1;

        config.node_index = index as u64;
        Some((config, Run(self.current_run)))
    }

    fn current_run_full(&self) -> bool {
        if let Some(config) = self.configs.get(self.current_run) {
            println!(
                "  clients connected: {} / {}",
                self.next_node_index,
                config.config.total_nodes.get()
            );
            self.next_node_index == config.config.total_nodes.get()
        } else {
            false
        }
    }
}

enum ToBackground<K: SignatureKey> {
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
    ClientConnected(Sender<ClientConfig<K>>),
}

struct ClientConfig<K> {
    pub run: Run,
    pub config: NetworkConfig<K>,
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
    #[serde(default = "default_padding")]
    pub padding: usize,
    #[serde(default = "default_config")]
    pub config: HotShotConfig<K>,
}

impl<K> Default for NetworkConfig<K> {
    fn default() -> Self {
        Self {
            rounds: default_rounds(),
            transactions_per_round: default_transactions_per_round(),
            node_index: 0,
            seed: [0u8; 32],
            padding: default_padding(),
            config: default_config(),
        }
    }
}

// This is hacky, blame serde for not having something like `default_value = "10"`

fn default_rounds() -> usize {
    10
}
fn default_transactions_per_round() -> usize {
    10
}
fn default_padding() -> usize {
    100
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
