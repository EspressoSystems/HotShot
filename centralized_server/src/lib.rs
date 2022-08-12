use async_std::{
    io::{ReadExt, WriteExt},
    net::{TcpListener, TcpStream},
    prelude::FutureExt,
};
use bincode::Options;
use flume::{Receiver, Sender};
use futures::FutureExt as _;
use hotshot::types::SignatureKey;
use hotshot_centralized_server_shared::{FromServer, ToServer};
use hotshot_types::traits::signature_key::EncodedPublicKey;
use snafu::ResultExt;
use std::{
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
    net::{IpAddr, SocketAddr},
    time::Duration,
};

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
    }
}

async fn spawn_client<K: SignatureKey + 'static>(
    address: SocketAddr,
    mut stream: TcpStream,
    parent_key: &mut Option<K>,
    to_background: Sender<ToBackground<K>>,
) -> Result<(), Error> {
    let (sender, receiver) = flume::unbounded();
    let mut sender = Some(sender);
    async_std::task::spawn({
        let mut stream = stream.clone();
        async move {
            while let Ok(msg) = receiver.recv_async().await {
                let bytes = hotshot_centralized_server_shared::bincode_opts()
                    .serialize(&msg)
                    .expect("Could not serialize message");
                if let Err(e) = stream.write_all(&bytes).await {
                    eprintln!("Lost connection to {:?}: {:?}", address, e);
                    break;
                }
            }
        }
    });
    loop {
        let mut buffer = [0u8; 1024];
        let n = stream.read(&mut buffer).await.context(IoSnafu)?;
        if n == 0 {
            break; // disconnected
        }
        match hotshot_centralized_server_shared::bincode_opts()
            .deserialize::<ToServer<K>>(&buffer[..n])
        {
            Ok(ToServer::Identify { key }) if parent_key.is_none() && sender.is_some() => {
                *parent_key = Some(key.clone());
                let sender = sender.take().unwrap();
                to_background
                    .send_async(ToBackground::NewClient { key, sender })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            Ok(ToServer::Identify { .. }) => {
                eprintln!("{:?} tried to identify twice", address);
            }
            Ok(ToServer::Broadcast { message }) if parent_key.is_some() => {
                let sender = parent_key.clone().unwrap();
                to_background
                    .send_async(ToBackground::IncomingBroadcast { sender, message })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            Ok(ToServer::Direct { message, target }) if parent_key.is_some() => {
                to_background
                    .send_async(ToBackground::IncomingDirectMessage {
                        receiver: target,
                        message,
                    })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            Ok(_) => {
                eprintln!("{:?} received message but is not identified yet", address);
            }
            Err(e) => {
                eprintln!("{:?} send invalid data: {:?}", address, e);
            }
        }
    }
    Ok(())
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
}

#[derive(snafu::Snafu, Debug)]
pub enum Error {
    Io { source: std::io::Error },
    BackgroundShutdown,
}
