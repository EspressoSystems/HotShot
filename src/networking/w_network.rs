use async_std::{
    net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
    task::{sleep, spawn},
};
use async_tungstenite::{accept_async, client_async, tungstenite::protocol, WebSocketStream};
use futures::channel::oneshot;
use futures::stream::SplitSink;
use futures::{pin_mut, prelude::*, select, stream::SplitStream};
use futures_lite::future;
use futures_locks::RwLock;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use snafu::{OptionExt, ResultExt};

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use crate::networking::{
    ExecutorError, FailedToBindListener, FailedToSerialize, NetworkError, NetworkingImplementation,
    NoSocketsError, NoSuchNode, SocketDecodeError, WError,
};
use crate::PubKey;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum Command<T> {
    /// A message that was broadcast to all nodes
    Broadcast {
        inner: T,
        from: PubKey,
    },
    /// A message that was sent directly to this node
    Direct {
        inner: T,
        from: PubKey,
        to: PubKey,
    },
    /// A message identifying the sending node
    Identify {
        from: PubKey,
    },
    Ping,
    Pong,
}

struct WNetworkInner<T> {
    own_key: PubKey,
    broadcast_queue: flume::Receiver<T>,
    direct_queue: flume::Receiver<T>,
    nodes: RwLock<HashMap<PubKey, SocketAddr>>,
    outgoing_connections: RwLock<
        HashMap<SocketAddr, RwLock<SplitSink<WebSocketStream<TcpStream>, protocol::Message>>>,
    >,
}

impl<T: Clone + Serialize + DeserializeOwned + Send + std::fmt::Debug + 'static> WNetworkInner<T> {
    fn new(
        own_key: PubKey,
        node_list: impl IntoIterator<Item = (PubKey, SocketAddr)>,
        broadcast: flume::Receiver<T>,
        direct: flume::Receiver<T>,
    ) -> Self {
        Self {
            own_key,
            broadcast_queue: broadcast,
            direct_queue: direct,
            nodes: RwLock::new(node_list.into_iter().collect()),
            outgoing_connections: RwLock::new(HashMap::new()),
        }
    }

    async fn new_from_strings(
        own_key: PubKey,
        node_list: impl IntoIterator<Item = (PubKey, String)>,
        broadcast: flume::Receiver<T>,
        direct: flume::Receiver<T>,
    ) -> Result<Self, NetworkError> {
        let mut node_map = HashMap::new();
        for (k, v) in node_list {
            let addr = v
                .to_socket_addrs()
                .await
                .context(SocketDecodeError { input: v.clone() })?
                .into_iter()
                .next()
                .context(NoSocketsError { input: v.clone() })?;
            node_map.insert(k, addr);
        }
        Ok(Self {
            own_key,
            broadcast_queue: broadcast,
            direct_queue: direct,
            nodes: RwLock::new(node_map),
            outgoing_connections: RwLock::new(HashMap::new()),
        })
    }
}

#[derive(Clone)]
pub struct WNetwork<T> {
    inner: Arc<WNetworkInner<T>>,
    tasks_generated: Arc<AtomicBool>,
    port: Arc<u16>,
    /// Keepalive timer duration
    keep_alive_duration: Duration,
    /// Keepalive round trips, used for debugging
    ping_count: Arc<AtomicU64>,
    /// Keepalive round trips, used for debugging
    pong_count: Arc<AtomicU64>,
    /// Holds onto the broadcast channel
    broadcast: flume::Sender<T>,
    /// Holds onto the direct channel
    direct: flume::Sender<T>,
}

impl<T: Clone + Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + 'static>
    WNetwork<T>
{
    pub async fn connect_to(
        &self,
        key: PubKey,
        addr: impl ToSocketAddrs,
    ) -> Result<(), NetworkError> {
        let mut outgoing_connections = self.inner.outgoing_connections.write().await;
        let socket = TcpStream::connect(addr).await.context(ExecutorError)?;
        let addr = socket.peer_addr().unwrap();
        let url = format!("ws://{}", addr);
        // Bincode up an identification command
        let ident = protocol::Message::Binary(
            bincode::serialize(&Command::<T>::Identify {
                from: self.inner.own_key.clone(),
            })
            .unwrap(),
        );
        // Get the socket
        let (web_socket, _) = client_async(url, socket).await.context(WError)?;
        // split the socket
        let (mut outgoing, incoming) = web_socket.split();
        // Identify ourselves
        outgoing.feed(ident).await.context(WError)?;
        // slot the new connection into the internal map
        outgoing_connections.insert(addr, RwLock::new(outgoing));
        // Register the new inbound connection
        self.register_incoming_connection(addr, incoming).await;
        // Load into the socket map
        let mut nodes = self.inner.nodes.write().await;
        nodes.insert(key, addr);
        Ok(())
    }
    async fn send_raw_message(
        &self,
        node: &PubKey,
        message: Command<T>,
    ) -> Result<(), NetworkError> {
        // Check to see if we have the node
        let addr = self
            .inner
            .nodes
            .read()
            .await
            .get(node)
            .cloned()
            .context(NoSuchNode)?;
        /*
        Bincode up the command
        */
        let binary = bincode::serialize(&message).context(FailedToSerialize)?;
        let w_message = protocol::Message::Binary(binary);
        // Check to see if we have a connection
        let outgoing_connections = self.inner.outgoing_connections.read().await;
        let connection_lock = outgoing_connections.get(&addr);
        if let Some(connection_lock) = connection_lock {
            let mut connection = connection_lock.write().await;
            // Use the existing connection, if one exists
            connection.feed(w_message).await.context(WError)?;
            Ok(())
        } else {
            // Drop outgoing_connections so that connect_to can do its thing to it
            std::mem::drop(outgoing_connections);
            // Open a new connection
            self.connect_to(node.clone(), addr).await?;
            // Grab the connection
            let map = self.inner.outgoing_connections.read().await;
            let connection = map.get(&addr).expect("Newly opened connection missing");
            connection
                .write()
                .await
                .feed(w_message)
                .await
                .context(WError)?;
            Ok(())
        }
    }

    pub async fn new_from_strings(
        own_key: PubKey,
        node_list: impl IntoIterator<Item = (PubKey, String)>,
        port: u16,
        keep_alive_duration: Option<Duration>,
    ) -> Result<Self, NetworkError> {
        // TODO: For now use small bounds on the flume channel to make sure that they block early.
        // Investigate proper limits.
        let (broadcast_s, broadcast_r) = flume::bounded(16);
        let (direct_s, direct_r) = flume::bounded(16);
        let inner: WNetworkInner<T> =
            WNetworkInner::new_from_strings(own_key, node_list, broadcast_r, direct_r).await?;
        let inner = Arc::new(inner);
        let tasks_generated = Arc::new(AtomicBool::new(false));
        // Default the duration to 100ms for now
        let keep_alive_duration =
            keep_alive_duration.unwrap_or_else(|| Duration::from_millis(1000));
        let ping_count = Arc::new(AtomicU64::new(0));
        let pong_count = Arc::new(AtomicU64::new(0));
        Ok(Self {
            keep_alive_duration,
            ping_count,
            pong_count,
            inner,
            tasks_generated,
            port: Arc::new(port),
            broadcast: broadcast_s,
            direct: direct_s,
        })
    }

    /// Spawns a task to process the input from an incoming stream
    async fn register_incoming_connection(
        &self,
        addr: SocketAddr,
        stream: SplitStream<WebSocketStream<TcpStream>>,
    ) {
        let x = self.clone();
        spawn(async move {
            /*
            Utility method for creating a future to process the next value from the stream

            Return value is true if loop should be broken

            Really sorry for putting this behavior in a closure, I promise it makes wrangling
            borrowchk _much_ easier

            Moving the stream into and out of the future is effectively required, it's not directly
            possible to hold on to ownership of the stream and keep the current future for the next
            element in a local variable, as the future for the next element maintains a mutable
            reference to the stream in such a way that it becomes nearly impossible to replace the
            future directly. This approach sidesteps the issue by disposing of the mutable reference
            before returning ownership of the stream
            */
            let next_fut =
                |mut s: SplitStream<WebSocketStream<TcpStream>>|
                                    -> future::Boxed<(bool, SplitStream<WebSocketStream<TcpStream>>)> {
                    let x = x.clone();
                    async move {
                        let next = s.next().await.expect("Stream Ended").expect("Stream Error");
                        match next {
                            protocol::Message::Binary(bin) => {
                                let decoded: Command<T> = bincode::deserialize(&bin[..])
                                    .expect("Failed to deserialize incoming message");
                                // Branch on the type of command
                                match decoded {
                                    Command::Broadcast { inner, from: _ } => {
                                        // Add the message to our broadcast queue
                                        x.broadcast.send_async(inner).await.unwrap()
                                    }
                                    Command::Direct { inner, from: _, to } => {
                                        // make sure this is meant for us, otherwise, discard it
                                        if x.inner.own_key == to {
                                            x.direct.send_async(inner).await.unwrap()
                                        }
                                    }
                                    Command::Ping => {
                                        // Wrap up a Pong to send back
                                        // Unwrap can not fail, variant does not contain any mutexs
                                        let bin = bincode::serialize(&Command::<T>::Pong).unwrap();
                                        let message = protocol::Message::Binary(bin);
                                        // Grab the socket and send the ping
                                        let map = x.inner.outgoing_connections.read().await;
                                        let socket_lock = map.get(&addr)
                                            .expect("Received on a socket we have no record of.");
                                        let mut socket = socket_lock.write().await;
                                        socket.feed(message).await.expect("Failed to send pong");
                                    }
                                    Command::Pong => {
                                        // Increment the pong counter
                                        x.pong_count.fetch_add(1, Ordering::SeqCst);
                                    }
                                    Command::Identify{from} => {
                                        // Add the node to our node list
                                        let mut map = x.inner.nodes.write().await;
                                        map.insert(from, addr);
                                    }
                                }
                                (false, s)
                            }
                            protocol::Message::Close(_) => (true, s),
                            _ => (false, s),
                        }
                    }
                    .boxed()
                };
            // Keep alive interrupt
            let timer = sleep(x.keep_alive_duration.clone()).fuse();
            pin_mut!(timer);
            // Next item future
            let mut next = next_fut(stream).fuse();
            /*
            I apologize for this nasty loop structure

            The need to keep an application-level keep alive requires that I keep both a future for
            the next item to come in, as well as the timer, in the mind of the task doing the
            background network processing.

            This requires the use of select!, and there is no ergonomic way to loop over a select!
            statement being used in such a way that that I have yet found.
             */
            loop {
                select! {
                    _ = timer => {
                        /*
                        Find the socket in the outgoing_connections map

                        Unwrap for the time being, its a violation of internal constraints and a
                        sign of a bug if we don't have a matching outgoing connection
                         */
                        let map = x.inner.outgoing_connections.read().await;
                        let socket_lock = map.get(&addr).unwrap();
                        let mut socket = socket_lock.write().await;
                        // Prepare the ping
                        // Cant fail to serialize, this variant doesn't contain anything
                        let bytes = bincode::serialize(&Command::<T>::Ping).unwrap();
                        let message = protocol::Message::Binary(bytes);
                        // Send the ping
                        socket.feed(message).await.expect("failed to send ping");
                        // Increment the counter
                        x.ping_count.fetch_add(1, Ordering::SeqCst);

                        // reset the timer
                        timer.set(sleep(x.keep_alive_duration.clone()).fuse());
                    },
                    (stop, stream) = next => {
                        if stop {
                            break;
                        }
                        // Replace the future
                        next = next_fut(stream).fuse()
                    }
                }
            }
        });
    }

    pub fn generate_task(
        &self,
        sync: oneshot::Sender<()>,
    ) -> Option<future::Boxed<Result<(), NetworkError>>> {
        // first check to see if we have generated the task before
        let generated = self
            .tasks_generated
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .unwrap_or(true);
        if generated {
            // We will only generate the tasks once, so go ahead and fault out
            None
        } else {
            let x = self.clone();
            Some(
                async move {
                    // Open up a listener
                    let listen_socket = ("0.0.0.0", *x.port)
                        .to_socket_addrs()
                        .await
                        .context(SocketDecodeError {
                            input: x.port.to_string(),
                        })?
                        .into_iter()
                        .next()
                        .context(NoSocketsError {
                            input: x.port.to_string(),
                        })?;
                    let listener = TcpListener::bind(listen_socket)
                        .await
                        .context(FailedToBindListener)?;
                    // Connection processing loop
                    let mut incoming = listener.incoming();
                    // Our port is now open, send the sync signal
                    sync.send(()).unwrap();
                    while let Some(stream) = incoming.next().await {
                        let stream = stream.expect("Failed to bind incoming connection.");
                        let addr = stream.peer_addr().unwrap();
                        // Process the stream and open up a new task to handle this connection
                        let ws_stream = accept_async(stream).await.expect("Error during handshake");
                        let (outgoing, incoming) = ws_stream.split();
                        /*
                        Register the outbound connection manually
                        */
                        let mut outgoing_connections = x.inner.outgoing_connections.write().await;
                        outgoing_connections.insert(addr, RwLock::new(outgoing));
                        // Register the inbound connection
                        x.register_incoming_connection(addr, incoming).await;
                    }
                    Ok(())
                }
                .boxed(),
            )
        }
    }
    pub async fn connection_table_size(&self) -> usize {
        self.inner.outgoing_connections.read().await.len()
    }
    pub async fn nodes_table_size(&self) -> usize {
        self.inner.nodes.read().await.len()
    }
}

impl<T: Clone + Serialize + DeserializeOwned + Send + std::fmt::Debug + Sync + 'static>
    NetworkingImplementation<T> for WNetwork<T>
{
    fn broadcast_message(&self, message: T) -> future::Boxed<Result<(), super::NetworkError>> {
        let w = self.clone();
        async move {
            // Create a command out of the message
            let m = Command::Broadcast {
                inner: message,
                from: w.inner.own_key.clone(),
            };
            // Iterate through every known node
            let node_list: Vec<_> = {
                // Use a block here to make sure we drop the lock, as send_raw_message may attempt
                // to open a new connection, via connect_to, which modifies nodes
                let nodes_lock = w.inner.nodes.read().await;
                nodes_lock.keys().cloned().collect()
            };
            for node in &node_list {
                // Hacky work around with some futures lifetime nonsense
                let m = m.clone();
                // Send the node the message
                w.send_raw_message(node, m).await?;
            }
            Ok(())
        }
        .boxed()
    }

    fn message_node(
        &self,
        message: T,
        recipient: PubKey,
    ) -> future::Boxed<Result<(), super::NetworkError>> {
        let w = self.clone();
        async move {
            // Create a command out of the message
            let m = Command::Direct {
                inner: message,
                from: w.inner.own_key.clone(),
                to: recipient.clone(),
            };
            // Attempt to send the command
            w.send_raw_message(&recipient, m).await?;
            Ok(())
        }
        .boxed()
    }

    fn broadcast_queue(&self) -> future::Boxed<Result<Vec<T>, super::NetworkError>> {
        let w = self.clone();
        async move {
            let mut output = vec![];
            if w.inner.broadcast_queue.is_empty() {
                let x = w.inner.broadcast_queue.recv_async().await.unwrap();
                output.push(x);
            }
            while let Ok(x) = w.inner.broadcast_queue.try_recv() {
                output.push(x);
            }
            Ok(output)
        }
        .boxed()
    }

    fn next_broadcast(&self) -> future::Boxed<Result<Option<T>, super::NetworkError>> {
        let w = self.clone();
        async move { Ok(Some(w.inner.broadcast_queue.recv_async().await.unwrap())) }.boxed()
    }

    fn direct_queue(&self) -> future::Boxed<Result<Vec<T>, super::NetworkError>> {
        let w = self.clone();
        async move {
            let mut output = vec![];
            if w.inner.direct_queue.is_empty() {
                let x = w.inner.direct_queue.recv_async().await.unwrap();
                output.push(x);
            }
            while let Ok(x) = w.inner.direct_queue.try_recv() {
                output.push(x);
            }
            Ok(output)
        }
        .boxed()
    }

    fn next_direct(&self) -> future::Boxed<Result<Option<T>, super::NetworkError>> {
        let w = self.clone();
        async move { Ok(Some(w.inner.direct_queue.recv_async().await.unwrap())) }.boxed()
    }

    fn known_nodes(&self) -> future::Boxed<Vec<PubKey>> {
        let w = self.clone();
        async move { w.inner.nodes.read().await.keys().cloned().collect() }.boxed()
    }

    fn obj_clone(&self) -> Box<dyn NetworkingImplementation<T> + 'static> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    #[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
    struct Test {
        message: u64,
    }
    use super::*;
    use async_std::task::yield_now;
    // Test both direct from SocketAddr creation and from String creation, and sanity check the
    // results against each other
    #[async_std::test]
    async fn w_network_inner_address_smoke() -> Result<(), NetworkError> {
        // Give ourselves an arbitrary pub key
        let own_key = PubKey::random(1234);
        // Make some key/address pairs
        let pub_keys: Vec<PubKey> = (0..3).map(|x| PubKey::random(x)).collect();
        let inputs = vec!["localhost:8080", "localhost:8081", "localhost:8082"];
        // Manually resolve them
        let mut inputs_sockets = vec![];
        for input in &inputs {
            let socket = input
                .to_socket_addrs()
                .await
                .context(SocketDecodeError {
                    input: input.clone(),
                })?
                .next()
                .context(NoSocketsError {
                    input: input.clone(),
                })?;
            inputs_sockets.push(socket);
        }

        // shove each set of pairs into a hashmap
        let mut input_strings = HashMap::new();
        let mut input_sockets = HashMap::new();

        for i in 0..3 {
            input_strings.insert(pub_keys[i].clone(), inputs[i].to_string());
            input_sockets.insert(pub_keys[i].clone(), inputs_sockets[i].clone());
        }

        // Get our networking implementation and don't
        let (_broadcast_s, broadcast_r) = flume::bounded(16);
        let (_direct_s, direct_r) = flume::bounded(16);
        let x: WNetworkInner<Test> =
            WNetworkInner::new(own_key.clone(), input_sockets, broadcast_r, direct_r);
        let (_broadcast_s, broadcast_r) = flume::bounded(16);
        let (_direct_s, direct_r) = flume::bounded(16);
        let y: WNetworkInner<Test> = WNetworkInner::new_from_strings(
            own_key.clone(),
            input_strings.clone(),
            broadcast_r,
            direct_r,
        )
        .await?;

        // Compare the nodes tables for equality
        assert!(x.nodes.try_unwrap().unwrap() == y.nodes.try_unwrap().unwrap());

        // Ensure that we can construct an outer WNetwork with the same strings
        let _: WNetwork<Test> =
            WNetwork::new_from_strings(own_key.clone(), input_strings.clone(), 1234, None).await?;

        Ok(())
    }

    // Ensures that the background task is generated once and only once
    #[async_std::test]
    async fn process_generates_once() {
        let node_list = HashMap::new();
        let own_key = PubKey::random(1234);
        let port = 8087;
        let y: WNetwork<Test> = WNetwork::new_from_strings(own_key.clone(), node_list, port, None)
            .await
            .expect("Creating WNetwork");

        // First call
        let (x, _sync) = oneshot::channel();
        let first = y.generate_task(x);
        assert!(first.is_some());

        // Second call
        let (x, _sync) = oneshot::channel();
        let second = y.generate_task(x);
        assert!(second.is_none());
    }

    // Tests to see if we can pass a message from node_a to node_b
    #[async_std::test]
    async fn verify_single_message() {
        let node_a_key = PubKey::random(1000);
        let node_b_key = PubKey::random(1001);
        // Construct the nodes
        println!("Constructing node a");
        let node_a: WNetwork<Test> =
            WNetwork::new_from_strings(node_a_key.clone(), vec![], 10000, None)
                .await
                .unwrap();
        println!("Constructing node b");
        let node_b: WNetwork<Test> =
            WNetwork::new_from_strings(node_b_key.clone(), vec![], 10001, None)
                .await
                .unwrap();
        // Launch the tasks
        println!("Launching node a");
        let (x, sync) = oneshot::channel();
        let node_a_task = node_a
            .generate_task(x)
            .expect("Failed to open task for node a");
        spawn(node_a_task);
        sync.await.unwrap();
        println!("Launching node b");
        let (x, sync) = oneshot::channel();
        let node_b_task = node_b
            .generate_task(x)
            .expect("Failed to open task for node b");
        spawn(node_b_task);
        sync.await.unwrap();
        // Manually connect the nodes, this test is not intended to cover the auto-connection
        println!("Connecting nodes");
        node_a
            .connect_to(node_b_key.clone(), "127.0.0.1:10001")
            .await
            .expect("Failed to connect to node");
        // Prepare a message
        let message = Test { message: 42 };
        // Send message from a to b
        println!("Messaging node b from node a");
        node_a
            .message_node(message.clone(), node_b_key.clone())
            .await
            .expect("Failed to message node b");
        // attempt to pick it back up from node b
        let mut recieved_messages = node_b.direct_queue().await.unwrap();
        while recieved_messages.is_empty() {
            yield_now().await;
            recieved_messages = node_b.direct_queue().await.unwrap();
        }
        println!("recieved: {:?}", recieved_messages);
        assert_eq!(recieved_messages[0], message);
    }

    // Bidirectinal message passing
    #[async_std::test]
    async fn verify_double_message() {
        let node_a_key = PubKey::random(1002);
        let node_b_key = PubKey::random(1003);
        // Construct the nodes
        println!("Constructing node a");
        let node_a: WNetwork<Test> =
            WNetwork::new_from_strings(node_a_key.clone(), vec![], 10002, None)
                .await
                .unwrap();
        println!("Constructing node b");
        let node_b: WNetwork<Test> =
            WNetwork::new_from_strings(node_b_key.clone(), vec![], 10003, None)
                .await
                .unwrap();
        // Launch the tasks
        println!("Launching node a");
        let (x, sync) = oneshot::channel();
        let node_a_task = node_a
            .generate_task(x)
            .expect("Failed to open task for node a");
        spawn(node_a_task);
        sync.await.unwrap();
        println!("Launching node b");
        let (x, sync) = oneshot::channel();
        let node_b_task = node_b
            .generate_task(x)
            .expect("Failed to open task for node b");
        spawn(node_b_task);
        sync.await.unwrap();
        // Manually connect the nodes, this test is not intended to cover the auto-connection
        println!("Connecting nodes");
        node_a
            .connect_to(node_b_key.clone(), "127.0.0.1:10003")
            .await
            .expect("Failed to connect to node");
        // Prepare a message
        let message = Test { message: 42 };
        // Send message from a to b
        println!("Messaging node b from node a");
        node_a
            .message_node(message.clone(), node_b_key.clone())
            .await
            .expect("Failed to message node b");
        // attempt to pick it back up from node b
        let mut recieved_messages = node_b.direct_queue().await.unwrap();
        while recieved_messages.is_empty() {
            yield_now().await;
            recieved_messages = node_b.direct_queue().await.unwrap();
        }
        println!("recieved: {:?}", recieved_messages);
        assert_eq!(recieved_messages[0], message);
        // Send message from b to a
        let message2 = Test { message: 43 };
        println!("Messaging node a from nod b");
        node_b
            .message_node(message2.clone(), node_a_key.clone())
            .await
            .expect("Failed to message node a");
        let mut recieved_messages = node_a.direct_queue().await.unwrap();
        while recieved_messages.is_empty() {
            yield_now().await;
            recieved_messages = node_a.direct_queue().await.unwrap();
        }
        assert_eq!(recieved_messages[0], message2);
    }

    // Fire off 20 messages between each node
    #[async_std::test]
    async fn twenty_messsages() {
        let node_a_key = PubKey::random(1004);
        let node_b_key = PubKey::random(1005);
        // Construct the nodes
        println!("Constructing node a");
        let node_a: WNetwork<Test> =
            WNetwork::new_from_strings(node_a_key.clone(), vec![], 10004, None)
                .await
                .unwrap();
        println!("Constructing node b");
        let node_b: WNetwork<Test> =
            WNetwork::new_from_strings(node_b_key.clone(), vec![], 10005, None)
                .await
                .unwrap();
        // Launch the tasks
        println!("Launching node a");
        let (x, sync) = oneshot::channel();
        let node_a_task = node_a
            .generate_task(x)
            .expect("Failed to open task for node a");
        spawn(node_a_task);
        sync.await.unwrap();
        println!("Launching node b");
        let (x, sync) = oneshot::channel();
        let node_b_task = node_b
            .generate_task(x)
            .expect("Failed to open task for node b");
        spawn(node_b_task);
        sync.await.unwrap();
        // Manually connect the nodes, this test is not intended to cover the auto-connection
        println!("Connecting nodes");
        node_a
            .connect_to(node_b_key.clone(), "127.0.0.1:10005")
            .await
            .expect("Failed to connect to node");
        // Fire off 20 messages
        for i in 0..20 {
            // a -> b
            let message_a = Test { message: i };
            // Send from a->b
            node_a
                .message_node(message_a.clone(), node_b_key.clone())
                .await
                .expect("Failed to message node b");
            let mut rec = node_b
                .next_direct()
                .await
                .expect("Failed to check b for pending message");
            while rec.is_none() {
                yield_now().await;
                rec = node_b
                    .next_direct()
                    .await
                    .expect("Failed to check b for pending message");
            }
            assert_eq!(rec.unwrap(), message_a);
            // Send from b->a
            let message_b = Test { message: i + 1000 };
            node_b
                .message_node(message_b.clone(), node_a_key.clone())
                .await
                .expect("Failed to message node a");
            let mut rec = node_a
                .next_direct()
                .await
                .expect("Failed to check b for pending message");
            while rec.is_none() {
                yield_now().await;
                rec = node_a
                    .next_direct()
                    .await
                    .expect("Failed to check b for pending message");
            }
            assert_eq!(rec.unwrap(), message_b);
        }
    }
}
