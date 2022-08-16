use async_std::{
    io::{ReadExt, WriteExt},
    net::{TcpListener, TcpStream},
    prelude::FutureExt,
};
use bincode::Options;
use flume::{Receiver, Sender};
use futures::FutureExt as _;
use hotshot_types::traits::signature_key::EncodedPublicKey;
use snafu::ResultExt;
use std::{
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
    net::{IpAddr, Shutdown, SocketAddr},
    time::Duration,
};

use bincode::config::*;
use hotshot_types::traits::signature_key::SignatureKey;

/// For the wire format, we use bincode with the following options:
///   - No upper size limit
///   - Litte endian encoding
///   - Varint encoding
///   - Reject trailing bytes
#[allow(clippy::type_complexity)]
pub fn bincode_opts() -> WithOtherTrailing<
    WithOtherIntEncoding<
        WithOtherEndian<WithOtherLimit<DefaultOptions, bincode::config::Infinite>, LittleEndian>,
        VarintEncoding,
    >,
    RejectTrailing,
> {
    bincode::DefaultOptions::new()
        .with_no_limit()
        .with_little_endian()
        .with_varint_encoding()
        .reject_trailing_bytes()
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug)]
#[serde(bound(deserialize = ""))]
pub enum ToServer<K: SignatureKey> {
    Identify { key: K },
    Broadcast { message: Vec<u8> },
    Direct { target: K, message: Vec<u8> },
    RequestClientCount,
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug)]
#[serde(bound(deserialize = ""))]
pub enum FromServer<K: SignatureKey> {
    NodeConnected { key: K },
    NodeDisconnected { key: K },
    Broadcast { message: Vec<u8> },
    Direct { message: Vec<u8> },
    ClientCount(usize),
}

pub struct Server<K: SignatureKey + 'static> {
    listener: TcpListener,
    shutdown: Option<Receiver<()>>,
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
            _k: PhantomData,
        }
    }

    /// Register a signal that can be used to shut down this server
    pub fn with_shutdown_signal(mut self, shutdown_listener: Receiver<()>) -> Self {
        self.shutdown = Some(shutdown_listener);
        self
    }

    /// Get the address that this server is running on
    pub fn addr(&self) -> SocketAddr {
        self.listener.local_addr().unwrap()
    }

    /// Run the server.
    ///
    /// If `with_shutdown_signal` is called before, this server will stop when that signal is called. Otherwise this server will run forever.
    pub async fn run(self) {
        let (sender, receiver) = flume::unbounded();

        let background_task_handle = async_std::task::spawn({
            async move {
                if let Err(e) = background_task(receiver).await {
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
                                        spawn_client::<K>(addr, stream, &mut key, sender.clone()).await
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
) -> Result<(), Error> {
    let mut clients = BTreeMap::<OrdKey<K>, Sender<FromServer<K>>>::new();
    loop {
        let msg = receiver
            .recv_async()
            .await
            .map_err(|_| Error::BackgroundShutdown)?;

        // if we fail to delivery a message to other clients, we store them in this vec
        // at the end we'll clean up the clients that we failed to `.send_async` to
        let mut clients_with_error = BTreeSet::<OrdKey<K>>::new();
        let client_count = clients.len();
        match msg {
            ToBackground::Shutdown => {
                eprintln!("Background thread shutting down");
                break Ok(());
            }
            ToBackground::NewClient { key, sender } => {
                // notify everyone else of the new client
                for (id, sender) in &mut clients {
                    if sender
                        .send_async(FromServer::NodeConnected { key: key.clone() })
                        .await
                        .is_err()
                    {
                        clients_with_error.insert(id.clone());
                    }
                }
                // add the client
                clients.insert(key.into(), sender);
            }
            ToBackground::ClientDisconnected { key } => {
                // remove the client
                clients.remove(&OrdKey::from(key.clone()));
                // notify everyone of the client disconnecting
                for (id, sender) in &mut clients {
                    if sender
                        .send_async(FromServer::NodeDisconnected { key: key.clone() })
                        .await
                        .is_err()
                    {
                        clients_with_error.insert(id.clone());
                    }
                }
            }
            ToBackground::IncomingBroadcast { message, sender } => {
                // Notify everyone but ourself of this message
                for (id, sender) in clients.iter_mut().filter(|(key, _)| key.key != sender) {
                    if sender
                        .send_async(FromServer::Broadcast {
                            message: message.clone(),
                        })
                        .await
                        .is_err()
                    {
                        clients_with_error.insert(id.clone());
                    }
                }
            }
            ToBackground::IncomingDirectMessage { receiver, message } => {
                let receiver = OrdKey::from(receiver);
                if let Some(channel) = clients.get_mut(&receiver) {
                    if channel
                        .send_async(FromServer::Direct { message })
                        .await
                        .is_err()
                    {
                        clients_with_error.insert(receiver);
                    }
                }
            }
            ToBackground::RequestClientCount { sender } => {
                let sender = OrdKey::from(sender);
                let client_count = clients.len();
                if let Some(channel) = clients.get_mut(&sender) {
                    if channel
                        .send_async(FromServer::ClientCount(client_count))
                        .await
                        .is_err()
                    {
                        clients_with_error.insert(sender);
                    }
                }
            }
        }

        // While notifying the clients of other clients disconnecting, those clients can be disconnected too
        // we solve this by looping over this until we've removed all failing nodes and have successfully notified everyone else.
        while !clients_with_error.is_empty() {
            let clients_to_remove = std::mem::take(&mut clients_with_error);
            for client in &clients_to_remove {
                eprintln!("Background task could not deliver message to client thread {:?}, removing them", client.pubkey);
                clients.remove(client);
            }
            for client in clients_to_remove {
                for (id, sender) in clients.iter_mut() {
                    if sender
                        .send_async(FromServer::NodeDisconnected {
                            key: client.key.clone(),
                        })
                        .await
                        .is_err()
                    {
                        // note: push to `clients_with_error`, NOT `clients_to_remove`
                        // clients_with_error will be attempted to be purged next loop
                        clients_with_error.insert(id.clone());
                    }
                }
            }
        }
        if client_count != clients.len() {
            println!("Clients connected: {}", clients.len());
        }
    }
}

async fn spawn_client<K: SignatureKey + 'static>(
    address: SocketAddr,
    stream: TcpStream,
    parent_key: &mut Option<K>,
    to_background: Sender<ToBackground<K>>,
) -> Result<(), Error> {
    let (sender, receiver) = flume::unbounded();
    let mut sender = Some(sender);
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
            (ToServer::Identify { key }, false) if sender.is_some() => {
                *parent_key = Some(key.clone());
                let sender = sender.take().unwrap();
                to_background
                    .send_async(ToBackground::NewClient { key, sender })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            (ToServer::Identify { .. }, true) => {
                eprintln!("{:?} tried to identify twice", address);
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
