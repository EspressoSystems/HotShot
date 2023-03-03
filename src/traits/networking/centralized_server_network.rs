//! A network implementation that attempts to connect to a centralized server.
//!
//! To run the server, see the `./centralized_server/` folder in this repo.
//!
#[cfg(feature = "async-std-executor")]
use async_std::net::TcpStream;
#[cfg(feature = "tokio-executor")]
use tokio::net::TcpStream;
#[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
std::compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}

use async_compatibility_layer::{
    art::{async_block_on, async_sleep, async_spawn, split_stream},
    channel::{oneshot, unbounded, OneShotSender, UnboundedReceiver, UnboundedSender},
};
use async_lock::{RwLock, RwLockUpgradableReadGuard};
use async_trait::async_trait;
use bincode::Options;
use futures::{future::BoxFuture, FutureExt};
use hotshot_centralized_server::{
    FromServer, NetworkConfig, Run, RunResults, TcpStreamRecvUtil, TcpStreamSendUtil,
    TcpStreamUtilWithRecv, TcpStreamUtilWithSend, ToServer,
};
use hotshot_types::traits::node_implementation::NodeImplementation;
use hotshot_types::{
    data::ProposalType,
    message::Message,
    traits::{
        election::{ElectionConfig, Membership},
        metrics::{Metrics, NoMetrics},
        network::{
            CentralizedServerNetworkError, CommunicationChannel, ConnectedNetwork,
            FailedToDeserializeSnafu, FailedToSerializeSnafu, NetworkError, NetworkMsg,
            TestableNetworkingImplementation, TransmitType,
        },
        node_implementation::NodeType,
        signature_key::{ed25519::Ed25519Pub, SignatureKey, TestableSignatureKey},
    },
    vote::VoteType,
};
use hotshot_utils::bincode::bincode_opts;
use serde::{de::DeserializeOwned, Serialize};
use snafu::ResultExt;
use std::{
    cmp,
    collections::{hash_map::Entry, BTreeSet, HashMap},
    marker::PhantomData,
    net::{Ipv4Addr, SocketAddr},
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tracing::{error, instrument};

use super::NetworkingMetrics;

/// The inner state of the `CentralizedServerNetwork`
#[derive(custom_debug::Debug)]
struct Inner<K: SignatureKey, E: ElectionConfig> {
    /// Self-identifying public key
    own_key: K,
    /// List of all known nodes
    known_nodes: Vec<K>,
    /// `true` if the TCP stream is connected to the server
    connected: AtomicBool,
    /// `true` if the client is still running.
    running: AtomicBool,
    /// A queue of messages to be send to the server. This is emptied by `run_background`.
    /// Each message can optionally have a callback sender that will be invoked when the message is send.
    sending: UnboundedSender<((ToServer<K>, Vec<u8>), Option<OneShotSender<()>>)>,
    /// A loopback sender that will send to `receiving`, for broadcasting to self.
    receiving_loopback: UnboundedSender<(FromServer<K, E>, Vec<u8>)>,
    /// A queue of messages to be received by this node. This is filled by `run_background`.
    receiving: UnboundedReceiver<(FromServer<K, E>, Vec<u8>)>,
    /// An internal queue of messages and, for some message types, payloads that have been received but not yet processed.
    incoming_queue: RwLock<Vec<(FromServer<K, E>, Vec<u8>)>>,
    /// a sender used to immediately broadcast the amount of clients connected
    request_client_count_sender: RwLock<Vec<OneShotSender<u32>>>,
    /// `true` if the server indicated that the run is ready to start, otherwise `false`
    run_ready: AtomicBool,
    #[debug(skip)]
    /// The networking metrics we're keeping track of
    metrics: NetworkingMetrics,

    /// the next ID
    cur_id: Arc<AtomicU64>,
}

/// Internal implementation detail; effectively allows interleaved streams to each behave as a state machine
enum MsgStepOutcome<RET> {
    /// this does not match the closure's criteria
    Skip,
    /// this is the first step of a multi-step match
    Begin,
    /// this is an intermediate step of a multi-step match
    Continue,
    /// this completes a match of one or more steps
    Complete(BTreeSet<usize>, RET),
}

/// Internal implementation detail; retains state for interleaved streams external to the closure, for consistency
struct MsgStepContext {
    /// Accumulates the indexes this stream will consume, if completed
    consumed_indexes: BTreeSet<usize>,
    /// The total size the message will have
    /// For streams that start with a size, rather than being unbounded with an explicit terminator
    message_len: u64,
    /// collects the data for a stream, allowing it to be deserialized upon completion
    accumulated_stream: Vec<u8>,
}

impl<K: SignatureKey, E: ElectionConfig> Inner<K, E> {
    /// Send a broadcast mesasge to the server.
    async fn broadcast(&self, message: Vec<u8>) {
        self.sending
            .send((
                (
                    ToServer::Broadcast {
                        message_len: message.len() as u64,
                    },
                    message.clone(),
                ),
                None,
            ))
            .await
            .expect("Background thread exited");
        self.receiving_loopback.send((
            FromServer::Broadcast {
                source: self.own_key.clone(),
                message_len: message.len() as u64,
                payload_len: message.len() as u64,
            },
            message,
        ))
        .await
        .expect("Loopback exited, this should never happen because we have a reference to this receiver ourselves");

        self.metrics.outgoing_message_count.add(1);
    }
    /// Send a direct message to the server.
    async fn direct_message(&self, target: K, message: Vec<u8>) {
        if target == self.own_key {
            self.receiving_loopback.send((
                FromServer::Direct {
                    source: self.own_key.clone(),
                    message_len: message.len() as u64,
                    payload_len: message.len() as u64,
                },
                message,
            ))
            .await
            .expect("Loopback exited, this should never happen because we have a reference to this receiver ourselves");
        } else {
            self.sending
                .send((
                    (
                        ToServer::Direct {
                            target,
                            message_len: message.len() as u64,
                        },
                        message,
                    ),
                    None,
                ))
                .await
                .expect("Background thread exited");
        }
        self.metrics.outgoing_message_count.add(1);
    }

    /// Request the client count from the server
    async fn request_client_count(&self, sender: OneShotSender<u32>) {
        self.request_client_count_sender.write().await.push(sender);
        self.sending
            .send(((ToServer::RequestClientCount, Vec::new()), None))
            .await
            .expect("Background thread exited");
    }

    /// Remove the first message from the internal queue, or the internal receiving channel, if the given `c` method returns `Some(RET)` on that entry.
    ///
    /// This will block this entire `Inner` struct until a message is found.
    #[allow(dead_code)]
    async fn remove_next_message_from_queue<F, FAIL, RET>(&self, c: F, f: FAIL) -> RET
    where
        F: Fn(
            &(FromServer<K, E>, Vec<u8>),
            usize,
            &mut HashMap<K, MsgStepContext>,
        ) -> MsgStepOutcome<RET>,
        FAIL: FnOnce(usize, &mut HashMap<K, MsgStepContext>) -> RET,
    {
        let incoming_queue = self.incoming_queue.upgradable_read().await;
        let mut context_map: HashMap<_, MsgStepContext> = HashMap::new();
        // pop all messages from the incoming stream, push them onto `result` if they match `c`, else push them onto our `lock`
        let temp_start_index = incoming_queue.len();
        for (i, msg) in incoming_queue.iter().enumerate() {
            match c(msg, i, &mut context_map) {
                MsgStepOutcome::Skip | MsgStepOutcome::Begin | MsgStepOutcome::Continue => {
                    continue;
                }
                MsgStepOutcome::Complete(indexes, ret) => {
                    let mut incoming_queue_mutation =
                        RwLockUpgradableReadGuard::upgrade(incoming_queue).await;

                    let incoming_queue = std::mem::take(&mut *incoming_queue_mutation);
                    *incoming_queue_mutation = incoming_queue
                        .into_iter()
                        .enumerate()
                        .filter_map(|(i, msg)| {
                            if indexes.contains(&i) {
                                None
                            } else {
                                Some(msg)
                            }
                        })
                        .collect::<Vec<_>>();

                    self.metrics.incoming_message_count.add(1);
                    return ret;
                }
            }
        }
        let mut temp_queue = Vec::new();
        let mut i = temp_start_index;
        while let Ok(msg) = self.receiving.recv().await {
            let step_outcome = c(&msg, i, &mut context_map);
            i += 1;
            match step_outcome {
                MsgStepOutcome::Skip | MsgStepOutcome::Begin | MsgStepOutcome::Continue => {
                    temp_queue.push(msg);
                    continue;
                }
                MsgStepOutcome::Complete(indexes, ret) => {
                    // no queued messages taken,
                    // all received messages taken (including this one)
                    let unchanged = indexes.iter().peekable().peek() == Some(&&temp_start_index)
                        && indexes.len() == temp_queue.len() + 1;
                    if !unchanged {
                        let mut incoming_queue_mutation =
                            RwLockUpgradableReadGuard::upgrade(incoming_queue).await;

                        let incoming_queue = std::mem::take(&mut *incoming_queue_mutation);
                        *incoming_queue_mutation = incoming_queue
                            .into_iter()
                            .chain(temp_queue)
                            .enumerate()
                            .filter_map(|(i, msg)| {
                                if indexes.contains(&i) {
                                    None
                                } else {
                                    Some(msg)
                                }
                            })
                            .collect::<Vec<_>>();
                    }

                    self.metrics.incoming_message_count.add(1);
                    return ret;
                }
            }
        }
        let mut incoming_queue_mutation = RwLockUpgradableReadGuard::upgrade(incoming_queue).await;
        incoming_queue_mutation.append(&mut temp_queue);
        tracing::error!("Could not receive message from centralized server queue");
        f(incoming_queue_mutation.len(), &mut context_map)
    }

    /// Remove all messages from the internal queue, and then the internal receiving channel, if the given `c` method returns `Some(RET)` on that entry.
    ///
    /// This will not block, and will return 0 items if nothing is in the internal queue or channel.
    async fn remove_messages_from_queue<F, RET>(&self, c: F) -> Vec<RET>
    where
        F: Fn(
            &(FromServer<K, E>, Vec<u8>),
            usize,
            &mut HashMap<K, MsgStepContext>,
        ) -> MsgStepOutcome<RET>,
    {
        let incoming_queue = self.incoming_queue.upgradable_read().await;
        let mut result = Vec::new();
        let mut context_map: HashMap<_, MsgStepContext> = HashMap::new();
        // pop all messages from the incoming stream, push them onto `result` if they match `c`, else push them onto our `lock`
        let temp_queue: Vec<_> = self
            .receiving
            .drain()
            .expect("Could not drain the receiver");
        let mut dead_indexes = BTreeSet::new();

        incoming_queue
            .iter()
            .chain(temp_queue.iter())
            .enumerate()
            .for_each(|(i, msg)| match c(msg, i, &mut context_map) {
                MsgStepOutcome::Skip | MsgStepOutcome::Begin | MsgStepOutcome::Continue => {}
                MsgStepOutcome::Complete(mut indexes, ret) => {
                    dead_indexes.append(&mut indexes);
                    result.push(ret);
                }
            });

        // (nothing taken && no new messages received)
        // || (no queued messages taken
        //   && all received messages taken)
        let unchanged = (dead_indexes.is_empty() && temp_queue.is_empty())
            || (dead_indexes.iter().peekable().peek() == Some(&&incoming_queue.len())
                && dead_indexes.len() == temp_queue.len());

        if !unchanged {
            let mut incoming_queue_mutation =
                RwLockUpgradableReadGuard::upgrade(incoming_queue).await;

            let incoming_queue = std::mem::take(&mut *incoming_queue_mutation);
            *incoming_queue_mutation = incoming_queue
                .into_iter()
                .chain(temp_queue)
                .enumerate()
                .filter_map(|(i, msg)| {
                    if dead_indexes.contains(&i) {
                        None
                    } else {
                        Some(msg)
                    }
                })
                .collect();
        }
        self.metrics.incoming_message_count.add(result.len());
        result
    }

    /// Get all the incoming broadcast messages received from the server. Returning 0 messages if nothing was received.
    async fn get_broadcasts<M: Serialize + DeserializeOwned + Send + Sync + Clone + 'static>(
        &self,
    ) -> Vec<Result<M, bincode::Error>> {
        self.remove_messages_from_queue(|msg, index, context_map| {
            match msg {
                (FromServer::Broadcast {
                    source,
                    message_len,
                    ..
                }, payload) =>
                {
                    let mut consumed_indexes = BTreeSet::new();
                    consumed_indexes.insert(index);
                    match (payload.len() as u64).cmp(message_len) {
                        cmp::Ordering::Less => {
                            let prev = context_map.insert(source.clone(), MsgStepContext {
                                consumed_indexes,
                                message_len: *message_len,
                                accumulated_stream: payload.clone(),
                            });

                            if prev.is_some() {
                                tracing::error!(?source, "FromServer::Broadcast encountered, incomplete prior Broadcast from same source");
                            }

                            MsgStepOutcome::Begin
                        },
                        cmp::Ordering::Greater => {
                            tracing::error!("FromServer::Broadcast with message_len {message_len}b, payload is {}b", payload.len());
                            MsgStepOutcome::Skip
                        },
                        cmp::Ordering::Equal => MsgStepOutcome::Complete(consumed_indexes, bincode_opts().deserialize(payload)),
                    }
                },
                (FromServer::BroadcastPayload { source, .. }, payload) => {
                    if let Entry::Occupied(mut context) = context_map.entry(source.clone()) {
                        context.get_mut().consumed_indexes.insert(index);
                        if context.get().accumulated_stream.is_empty() && context.get().message_len as usize == payload.len() {
                            let (_, context) = context.remove_entry();
                            MsgStepOutcome::Complete(context.consumed_indexes, bincode_opts().deserialize(payload))
                        } else {
                            context.get_mut().accumulated_stream.append(&mut payload.clone());
                            match context.get().accumulated_stream.len().cmp(&(context.get().message_len as usize)) {
                                cmp::Ordering::Less => MsgStepOutcome::Continue,
                                cmp::Ordering::Greater => {
                                    let (_, context) = context.remove_entry();
                                    tracing::error!("FromServer::Broadcast with message_len {}b, accumulated payload with {}b",context.message_len, context.accumulated_stream.len());
                                    MsgStepOutcome::Skip
                                }
                                cmp::Ordering::Equal => {
                                    let (_, context) = context.remove_entry();
                                    MsgStepOutcome::Complete(context.consumed_indexes, bincode_opts().deserialize(&context.accumulated_stream))
                                }
                            }
                        }
                    } else {
                        tracing::error!("FromServer::BroadcastPayload found, but no incomplete FromServer::Broadcast exists");
                        MsgStepOutcome::Skip
                    }
                },
                (_, _) => MsgStepOutcome::Skip,
            }
        })
        .await
    }

    /// Get the next incoming broadcast message received from the server. Will lock up this struct internally until a message was received.
    #[allow(dead_code)]
    async fn get_next_broadcast<M: Serialize + DeserializeOwned + Send + Sync + Clone + 'static>(
        &self,
    ) -> Result<M, NetworkError> {
        self.remove_next_message_from_queue(|msg, index, context_map| {
            match msg {
                (FromServer::Broadcast {
                    source,
                    message_len,
                    ..
                }, payload) =>
                {
                    let mut consumed_indexes = BTreeSet::new();
                    consumed_indexes.insert(index);
                    match (payload.len() as u64).cmp(message_len) {
                        cmp::Ordering::Less => {
                            let prev = context_map.insert(source.clone(), MsgStepContext {
                                consumed_indexes,
                                message_len: *message_len,
                                accumulated_stream: payload.clone(),
                            });

                            if prev.is_some() {
                                tracing::error!(?source, "FromServer::Broadcast encountered, incomplete prior Broadcast from same source");

                            }

                            MsgStepOutcome::Begin
                        },
                        cmp::Ordering::Greater => {
                            tracing::error!("FromServer::Broadcast with message_len {message_len}b, payload is {}b", payload.len());
                            MsgStepOutcome::Skip
                        },
                        cmp::Ordering::Equal => MsgStepOutcome::Complete(consumed_indexes, bincode_opts().deserialize(payload).context(FailedToDeserializeSnafu)),
                    }
                },
                (FromServer::BroadcastPayload { source, .. }, payload) => {
                    if let Entry::Occupied(mut context) = context_map.entry(source.clone()) {
                        context.get_mut().consumed_indexes.insert(index);
                        if context.get().accumulated_stream.is_empty() && context.get().message_len as usize == payload.len() {
                            let (_, context) = context.remove_entry();
                            MsgStepOutcome::Complete(context.consumed_indexes, bincode_opts().deserialize(payload).context(FailedToDeserializeSnafu))
                        } else {
                            context.get_mut().accumulated_stream.append(&mut payload.clone());
                            match context.get().accumulated_stream.len().cmp(&(context.get().message_len as usize)) {
                                cmp::Ordering::Less => MsgStepOutcome::Continue,
                                cmp::Ordering::Greater => {
                                    let (_, context) = context.remove_entry();
                                    tracing::error!("FromServer::Broadcast with message_len {}b, accumulated payload with {}b", context.message_len, context.accumulated_stream.len());
                                    MsgStepOutcome::Skip
                                }
                                cmp::Ordering::Equal => {
                                let (_, context) = context.remove_entry();
                                MsgStepOutcome::Complete(context.consumed_indexes, bincode_opts().deserialize(&context.accumulated_stream).context(FailedToDeserializeSnafu))
                            }
                        }
                        }
                    } else {
                        tracing::error!("FromServer::BroadcastPayload found, but no incomplete FromServer::Broadcast exists");
                        MsgStepOutcome::Skip
                    }
                },
                (_, _) => MsgStepOutcome::Skip,
            }
        },
        |_, _| {
            Err(NetworkError::CentralizedServer { source: CentralizedServerNetworkError::NoMessagesInQueue })
        },
)
        .await
    }

    /// Get all the incoming direct messages received from the server. Returning 0 messages if nothing was received.
    async fn get_direct_messages<
        M: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    >(
        &self,
    ) -> Vec<Result<M, bincode::Error>> {
        self.remove_messages_from_queue(|msg, index, context_map| {
            match msg {
                (FromServer::Direct {
                    source,
                    message_len,
                    ..
                }, payload) =>
                {
                    let mut consumed_indexes = BTreeSet::new();
                    consumed_indexes.insert(index);
                    match (payload.len() as u64).cmp(message_len) {
                        cmp::Ordering::Less => {
                            let prev = context_map.insert(source.clone(), MsgStepContext {
                                consumed_indexes,
                                message_len: *message_len,
                                accumulated_stream: payload.clone(),
                            });

                            if prev.is_some() {
                                tracing::error!(?source, "FromServer::Direct encountered, incomplete prior Direct from same source");
                            }

                            MsgStepOutcome::Begin
                        },
                        cmp::Ordering::Greater => {
                            tracing::error!("FromServer::Direct with message_len {message_len}b, payload is {}b", payload.len());
                            MsgStepOutcome::Skip
                        },
                        cmp::Ordering::Equal => {
                            MsgStepOutcome::Complete(consumed_indexes, bincode_opts().deserialize(payload))
                        },
                    }
                },
                (FromServer::DirectPayload { source, .. }, payload) => {
                    if let Entry::Occupied(mut context) = context_map.entry(source.clone()) {
                        context.get_mut().consumed_indexes.insert(index);
                        if context.get().accumulated_stream.is_empty() && context.get().message_len as usize == payload.len() {
                            let (_, context) = context.remove_entry();
                            MsgStepOutcome::Complete(context.consumed_indexes, bincode_opts().deserialize(payload))
                        } else {
                            context.get_mut().accumulated_stream.append(&mut payload.clone());
                            match context.get().accumulated_stream.len().cmp(&(context.get().message_len as usize)) {
                                cmp::Ordering::Less => {
                                    MsgStepOutcome::Continue
                                }
                                cmp::Ordering::Greater => {
                                    tracing::error!("FromServer::Broadcast with message_len {}b, accumulated payload with {}b",context.get().message_len, context.get().accumulated_stream.len());
                                    context.remove_entry();
                                    MsgStepOutcome::Skip
                                }
                                cmp::Ordering::Equal => {
                                    let (_, context) = context.remove_entry();
                                    MsgStepOutcome::Complete(context.consumed_indexes, bincode_opts().deserialize(&context.accumulated_stream))
                                }
                            }
                        }
                    } else {
                        tracing::error!("FromServer::BroadcastPayload found, but no incomplete FromServer::Broadcast exists");
                        MsgStepOutcome::Skip
                    }
                },
                (_, _) => MsgStepOutcome::Skip,
            }
        })
        .await
    }

    /// Get the next incoming direct message received from the server. Will lock up this struct internally until a message was received.
    #[allow(dead_code)]
    async fn get_next_direct_message<
        M: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    >(
        &self,
    ) -> Result<M, NetworkError> {
        self.remove_next_message_from_queue(|msg, index, context_map| {
            match msg {
                (FromServer::Direct {
                    source,
                    message_len,
                    ..
                }, payload) =>
                {
                    let mut consumed_indexes = BTreeSet::new();
                    consumed_indexes.insert(index);
                    match (payload.len() as u64).cmp(message_len) {
                        cmp::Ordering::Less => {
                            let prev = context_map.insert(source.clone(), MsgStepContext {
                                consumed_indexes,
                                message_len: *message_len,
                                accumulated_stream: payload.clone(),
                            });

                            if prev.is_some() {
                                tracing::error!(?source, "FromServer::Direct encountered, incomplete prior Direct from same source");
                            }

                            MsgStepOutcome::Begin
                        },
                        cmp::Ordering::Greater => {
                            tracing::error!("FromServer::Direct with message_len {message_len}b, payload is {}b", payload.len());
                            MsgStepOutcome::Skip
                        },
                        cmp::Ordering::Equal => {
                            MsgStepOutcome::Complete(consumed_indexes, bincode_opts().deserialize(payload).context(FailedToDeserializeSnafu))
                        },
                    }
                },
                (FromServer::DirectPayload { source, .. }, payload) => {
                    if let Entry::Occupied(mut context) = context_map.entry(source.clone()) {
                        context.get_mut().consumed_indexes.insert(index);
                        if context.get().accumulated_stream.is_empty() && context.get().message_len as usize == payload.len() {
                            let (_, context) = context.remove_entry();
                            MsgStepOutcome::Complete(context.consumed_indexes, bincode_opts().deserialize(payload).context(FailedToDeserializeSnafu))
                        } else {
                            context.get_mut().accumulated_stream.append(&mut payload.clone());
                            match context.get().accumulated_stream.len().cmp(&(context.get().message_len as usize)) {
                                cmp::Ordering::Less => {
                                    MsgStepOutcome::Continue
                                }
                                cmp::Ordering::Greater => {
                                    let (_, context) = context.remove_entry();
                                    tracing::error!("FromServer::Broadcast with message_len {}b, accumulated payload with {}b", context.message_len, context.accumulated_stream.len());
                                    MsgStepOutcome::Skip
                                }
                                cmp::Ordering::Equal => {
                                    let (_, context) = context.remove_entry();
                                    MsgStepOutcome::Complete(context.consumed_indexes, bincode_opts().deserialize(&context.accumulated_stream).context(FailedToDeserializeSnafu))
                                }
                            }
                        }
                    } else {
                        tracing::error!("FromServer::BroadcastPayload found, but no incomplete FromServer::Broadcast exists");
                        MsgStepOutcome::Skip
                    }
                },
                (_, _) => MsgStepOutcome::Skip,
            }
        },
        |_, _| {
            Err(NetworkError::CentralizedServer { source: CentralizedServerNetworkError::NoMessagesInQueue })
        })
        .await
    }
}

/// Handle for connecting to a centralized server
#[derive(Clone, Debug)]
pub struct CentralizedServerNetwork<K: SignatureKey, E: ElectionConfig> {
    /// The inner state
    inner: Arc<Inner<K, E>>,
    /// An optional shutdown signal. This is only used when this connection is created through the `TestableNetworkingImplementation` API.
    server_shutdown_signal: Option<Arc<OneShotSender<()>>>,
}

impl<K: SignatureKey + 'static, E: ElectionConfig + 'static> CentralizedServerNetwork<K, E> {
    /// Connect with the server running at `addr` and retrieve the config from the server.
    ///
    /// The config is returned along with the current run index and the running `CentralizedServerNetwork`
    ///
    /// # Panics
    ///
    /// Will panic if the server has a different signature key (`K`) or election config (`E`)
    pub async fn connect_with_server_config(
        metrics: Box<dyn Metrics>,
        addr: SocketAddr,
    ) -> (NetworkConfig<K, E>, Run, Self) {
        let (streams, run, config) = loop {
            let (mut recv_stream, mut send_stream) = match TcpStream::connect(addr).await {
                Ok(stream) => {
                    let (read_stream, write_stream) = split_stream(stream);
                    (
                        TcpStreamRecvUtil::new(read_stream),
                        TcpStreamSendUtil::new(write_stream),
                    )
                }
                Err(e) => {
                    error!("Could not connect to server: {:?}", e);
                    error!("Trying again in 5 seconds");
                    async_sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };
            if let Err(e) = send_stream.send(ToServer::<Ed25519Pub>::GetConfig).await {
                error!("Could not request config from server: {e:?}");
                error!("Trying again in 5 seconds");
                async_sleep(Duration::from_secs(5)).await;
                continue;
            }
            match recv_stream.recv().await {
                Ok(FromServer::Config { config, run }) => {
                    assert_eq!(config.key_type_name, std::any::type_name::<K>());
                    assert_eq!(config.election_config_type_name, std::any::type_name::<E>());
                    break ((recv_stream, send_stream), run, config);
                }
                x => {
                    error!("Expected config from server, got {:?}", x);
                    error!("Trying again in 5 seconds");
                    async_sleep(Duration::from_secs(5)).await;
                }
            }
        };

        let (pub_key, _priv_key) = K::generated_from_seed_indexed(config.seed, config.node_index);
        let known_nodes = config.config.known_nodes.clone();

        let mut streams = Some(streams);

        let result = Self::create(
            metrics,
            known_nodes,
            move || {
                let streams = streams.take();
                async move {
                    if let Some(streams) = streams {
                        streams
                    } else {
                        Self::connect_to(addr).await
                    }
                }
                .boxed()
            },
            pub_key,
        );
        (*config, run, result)
    }

    /// Send the results for this run to the server
    pub async fn send_results(&self, results: RunResults) {
        let (sender, receiver) = oneshot();
        let _result = self
            .inner
            .sending
            .send(((ToServer::Results(results), Vec::new()), Some(sender)))
            .await;
        // Wait until it's successfully send before shutting down
        let _ = receiver.recv().await;
    }

    /// Returns `true` if the server indicated that the current run was ready to start
    pub fn run_ready(&self) -> bool {
        self.inner.run_ready.load(Ordering::Relaxed)
    }
}

impl<K: SignatureKey + 'static, E: ElectionConfig + 'static> CentralizedServerNetwork<K, E> {
    /// Connect to a given socket address. Will loop and try to connect every 5 seconds if the server is unreachable.
    fn connect_to(addr: SocketAddr) -> BoxFuture<'static, (TcpStreamRecvUtil, TcpStreamSendUtil)> {
        async move {
            loop {
                match TcpStream::connect(addr).await {
                    Ok(stream) => {
                        break {
                            let (read_stream, write_stream) = split_stream(stream);
                            (
                                TcpStreamRecvUtil::new(read_stream),
                                TcpStreamSendUtil::new(write_stream),
                            )
                        }
                    }
                    Err(e) => {
                        error!("Could not connect to server: {:?}", e);
                        error!("Trying again in 5 seconds");
                        async_sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                }
            }
        }
        .boxed()
    }
    /// Connect to a centralized server
    pub fn connect(
        metrics: Box<dyn Metrics>,
        known_nodes: Vec<K>,
        addr: SocketAddr,
        key: K,
    ) -> Self {
        Self::create(metrics, known_nodes, move || Self::connect_to(addr), key)
    }

    /// Create a `CentralizedServerNetwork`. Every time a new TCP connection is needed, `create_connection` is called.
    ///
    /// This will auto-reconnect when the network loses connection to the server.
    fn create<F>(
        metrics: Box<dyn Metrics>,
        known_nodes: Vec<K>,
        mut create_connection: F,
        key: K,
    ) -> Self
    where
        F: FnMut() -> BoxFuture<'static, (TcpStreamRecvUtil, TcpStreamSendUtil)> + Send + 'static,
    {
        let (to_background_sender, mut to_background) = unbounded();
        let (from_background_sender, from_background) = unbounded();
        let receiving_loopback = from_background_sender.clone();

        let inner = Arc::new(Inner {
            own_key: key.clone(),
            connected: AtomicBool::new(false),
            running: AtomicBool::new(true),
            known_nodes,
            sending: to_background_sender,
            receiving_loopback,
            receiving: from_background,
            incoming_queue: RwLock::default(),
            request_client_count_sender: RwLock::default(),
            run_ready: AtomicBool::new(false),
            metrics: NetworkingMetrics::new(metrics),
            cur_id: Arc::new(AtomicU64::new(0)),
        });
        async_spawn({
            let inner = Arc::clone(&inner);
            async move {
                while inner.running.load(Ordering::Relaxed) {
                    let (recv_stream, send_stream) = create_connection().await;

                    if let Err(e) = run_background(
                        recv_stream,
                        send_stream,
                        key.clone(),
                        &mut to_background,
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
    pub async fn get_connected_client_count(&self) -> u32 {
        let (sender, receiver) = oneshot();
        self.inner.request_client_count(sender).await;
        receiver
            .recv()
            .await
            .expect("Could not request client count from server")
    }
}

/// Initialize a `TcpStreamUtil`. This will send an identify with key `key`.
///
/// - All messages sent to the sender of `to_background` will be sent to the server.
/// - All messages received from the TCP stream will be sent to `from_background_sender`.
async fn run_background<K: SignatureKey, E: ElectionConfig>(
    recv_stream: TcpStreamRecvUtil,
    mut send_stream: TcpStreamSendUtil,
    key: K,
    to_background: &mut UnboundedReceiver<((ToServer<K>, Vec<u8>), Option<OneShotSender<()>>)>,
    from_background_sender: UnboundedSender<(FromServer<K, E>, Vec<u8>)>,
    connection: Arc<Inner<K, E>>,
) -> Result<(), Error> {
    // send identify
    send_stream
        .send(ToServer::Identify { key: key.clone() })
        .await?;
    connection.connected.store(true, Ordering::Relaxed);

    // If we were in the middle of requesting # of clients, re-send that request
    if !connection
        .request_client_count_sender
        .read()
        .await
        .is_empty()
    {
        send_stream.send(ToServer::<K>::RequestClientCount).await?;
    }

    let send_handle = run_background_send(send_stream, to_background);
    let recv_handle = run_background_recv(recv_stream, from_background_sender, connection);

    futures::future::try_join(send_handle, recv_handle)
        .await
        .map(|(_, _)| ())
}

/// Loop on the `to_background` channel.
///
/// - All messages sent to the sender of `to_background` will be sent to the server.
async fn run_background_send<K: SignatureKey>(
    mut stream: TcpStreamSendUtil,
    to_background: &mut UnboundedReceiver<((ToServer<K>, Vec<u8>), Option<OneShotSender<()>>)>,
) -> Result<(), Error> {
    loop {
        let result = to_background.recv().await;
        let (msg, confirm) = result.map_err(|_| Error::FailedToSend)?;
        let (header, payload) = msg;
        let expect_payload = &header.payload_len();
        if let Some(payload_expected_len) = *expect_payload {
            if payload.len() != <NonZeroUsize as Into<usize>>::into(payload_expected_len) {
                tracing::warn!(
                    ?header,
                    "expected payload of {payload_expected_len} bytes, got {} bytes",
                    payload.len(),
                );
            }
        }
        stream.send(header).await?;
        if !payload.is_empty() {
            stream.send_raw(&payload, payload.len()).await?;
        }

        if let Some(confirm) = confirm {
            confirm.send(());
        }
    }
}

/// Loop on the TCP recv stream.
///
/// - All messages received from the TCP stream will be sent to `from_background_sender`.
async fn run_background_recv<K: SignatureKey, E: ElectionConfig>(
    mut stream: TcpStreamRecvUtil,
    from_background_sender: UnboundedSender<(FromServer<K, E>, Vec<u8>)>,
    connection: Arc<Inner<K, E>>,
) -> Result<(), Error> {
    loop {
        let msg = stream.recv().await?;
        match msg {
            x @ (FromServer::NodeConnected { .. } | FromServer::NodeDisconnected { .. }) => {
                from_background_sender
                    .send((x, Vec::new()))
                    .await
                    .map_err(|_| Error::FailedToReceive)?;
            }

            x @ (FromServer::Broadcast { .. } | FromServer::Direct { .. }) => {
                let payload = if let Some(payload_len) = x.payload_len() {
                    stream.recv_raw_all(payload_len.into()).await?
                } else {
                    Vec::new()
                };
                from_background_sender
                    .send((x, payload))
                    .await
                    .map_err(|_| Error::FailedToReceive)?;
            }

            x @ (FromServer::BroadcastPayload { .. } | FromServer::DirectPayload { .. }) => {
                let payload = if let Some(payload_len) = x.payload_len() {
                    stream.recv_raw_all(payload_len.into()).await?
                } else {
                    Vec::new()
                };
                from_background_sender
                    .send((x, payload))
                    .await
                    .map_err(|_| Error::FailedToReceive)?;
            }

            FromServer::ClientCount(count) => {
                let senders =
                    std::mem::take(&mut *connection.request_client_count_sender.write().await);
                connection.metrics.connected_peers.set(count as _);
                for sender in senders {
                    sender.send(count);
                }
            }

            FromServer::Config { .. } => {
                tracing::warn!("Received config from server but we're already running",);
            }

            FromServer::Start => {
                connection.run_ready.store(true, Ordering::Relaxed);
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
            hotshot_centralized_server::Error::BackgroundShutdown
            | hotshot_centralized_server::Error::SizeMismatch { .. }
            | hotshot_centralized_server::Error::VecToArray { .. } => unreachable!(), // should never be reached
        }
    }
}

#[async_trait]
impl<M: NetworkMsg, K: SignatureKey + 'static, E: ElectionConfig + 'static>
    ConnectedNetwork<M, M, K> for CentralizedServerNetwork<K, E>
{
    #[instrument(name = "CentralizedServer::ready_blocking", skip_all)]
    async fn wait_for_ready(&self) {
        while !self.inner.connected.load(Ordering::Relaxed) {
            async_sleep(Duration::from_secs(1)).await;
        }
    }

    #[instrument(name = "CentralizedServer::ready", skip_all)]
    async fn is_ready(&self) -> bool {
        self.run_ready()
    }

    #[instrument(name = "CentralizedServer::shut_down", skip_all)]
    async fn shut_down(&self) {
        error!("SHUTTING DOWN CENTRALIZED SERVER");
        self.inner.running.store(false, Ordering::Relaxed);
    }

    #[instrument(name = "CentralizedServer::broadcast_message", skip_all)]
    async fn broadcast_message(
        &self,
        message: M,
        _recipients: BTreeSet<K>,
    ) -> Result<(), NetworkError> {
        self.inner
            .broadcast(
                bincode_opts()
                    .serialize(&message)
                    .context(FailedToSerializeSnafu)?,
            )
            .await;
        Ok(())
    }

    #[instrument(name = "CentralizedServer::direct_message", skip_all)]
    async fn direct_message(&self, message: M, recipient: K) -> Result<(), NetworkError> {
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

    #[instrument(name = "CentralizedServer::recv_msgs", skip_all)]
    async fn recv_msgs(&self, transmit_type: TransmitType) -> Result<Vec<M>, NetworkError> {
        match transmit_type {
            TransmitType::Direct => self
                .inner
                .get_direct_messages()
                .await
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
                .context(FailedToDeserializeSnafu),
            TransmitType::Broadcast => self
                .inner
                .get_broadcasts()
                .await
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
                .context(FailedToDeserializeSnafu),
        }
    }

    async fn lookup_node(&self, _pk: K) -> Result<(), NetworkError> {
        // we are centralized. Should we do anything here?
        Ok(())
    }

    async fn inject_consensus_info(&self, _tuple: (u64, bool, bool)) -> Result<(), NetworkError> {
        // Not required
        Ok(())
    }
}

/// libp2p identity communication channel
#[derive(Clone)]
pub struct CentralizedCommChannel<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
    MEMBERSHIP: Membership<TYPES>,
>(
    CentralizedServerNetwork<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    PhantomData<(PROPOSAL, VOTE, MEMBERSHIP, I)>,
);

impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES>,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        MEMBERSHIP: Membership<TYPES>,
    > CentralizedCommChannel<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP>
{
    /// create new communication channel
    pub fn new(
        network: CentralizedServerNetwork<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    ) -> Self {
        Self(network, PhantomData::default())
    }

    /// passthru for example?
    pub async fn get_connected_client_count(&self) -> u32 {
        self.0.get_connected_client_count().await
    }

    /// passthru for example?
    pub async fn send_results(&self, results: RunResults) {
        self.0.send_results(results).await;
    }
}

#[async_trait]
impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES>,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        MEMBERSHIP: Membership<TYPES>,
    > CommunicationChannel<TYPES, Message<TYPES, I>, PROPOSAL, VOTE, MEMBERSHIP>
    for CentralizedCommChannel<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP>
{
    async fn wait_for_ready(&self) {
        <CentralizedServerNetwork<_, _> as ConnectedNetwork<
            Message<TYPES, I>,
            Message<TYPES, I>,
            TYPES::SignatureKey,
        >>::wait_for_ready(&self.0)
        .await;
    }

    async fn is_ready(&self) -> bool {
        <CentralizedServerNetwork<_, _> as ConnectedNetwork<
            Message<TYPES, I>,
            Message<TYPES, I>,
            TYPES::SignatureKey,
        >>::is_ready(&self.0)
        .await
    }

    async fn shut_down(&self) -> () {
        <CentralizedServerNetwork<_, _> as ConnectedNetwork<
            Message<TYPES, I>,
            Message<TYPES, I>,
            TYPES::SignatureKey,
        >>::shut_down(&self.0)
        .await;
    }

    async fn broadcast_message(
        &self,
        message: Message<TYPES, I>,
        membership: &MEMBERSHIP,
    ) -> Result<(), NetworkError> {
        let view_number = message.get_view_number();
        let recipients = <MEMBERSHIP as Membership<TYPES>>::get_committee(membership, view_number);
        self.0.broadcast_message(message, recipients).await
    }

    async fn direct_message(
        &self,
        message: Message<TYPES, I>,
        recipient: TYPES::SignatureKey,
    ) -> Result<(), NetworkError> {
        self.0.direct_message(message, recipient).await
    }

    async fn recv_msgs(
        &self,
        transmit_type: TransmitType,
    ) -> Result<Vec<Message<TYPES, I>>, NetworkError> {
        self.0.recv_msgs(transmit_type).await
    }

    async fn lookup_node(&self, pk: TYPES::SignatureKey) -> Result<(), NetworkError> {
        <CentralizedServerNetwork<_, _> as ConnectedNetwork<
            Message<TYPES, I>,
            Message<TYPES, I>,
            TYPES::SignatureKey,
        >>::lookup_node(&self.0, pk)
        .await
    }

    async fn inject_consensus_info(&self, _tuple: (u64, bool, bool)) -> Result<(), NetworkError> {
        // Not required
        Ok(())
    }
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES>,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        MEMBERSHIP: Membership<TYPES>,
    > TestableNetworkingImplementation<TYPES, Message<TYPES, I>, PROPOSAL, VOTE, MEMBERSHIP>
    for CentralizedCommChannel<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP>
where
    TYPES::SignatureKey: TestableSignatureKey,
{
    fn generator(
        expected_node_count: usize,
        _num_bootstrap: usize,
    ) -> Box<dyn Fn(u64) -> Self + 'static> {
        let (server_shutdown_sender, server_shutdown) = oneshot();
        let sender = Arc::new(server_shutdown_sender);

        let server = async_block_on(hotshot_centralized_server::Server::<
            TYPES::SignatureKey,
            TYPES::ElectionConfigType,
        >::new(Ipv4Addr::LOCALHOST.into(), 0))
        .with_shutdown_signal(server_shutdown);
        let addr = server.addr();
        async_spawn(server.run());

        let known_nodes = (0..expected_node_count as u64)
            .map(|id| {
                TYPES::SignatureKey::from_private(&TYPES::SignatureKey::generate_test_key(id))
            })
            .collect::<Vec<_>>();

        Box::new(move |id| {
            let sender = Arc::clone(&sender);
            let mut network = CentralizedServerNetwork::connect(
                NoMetrics::new(),
                known_nodes.clone(),
                addr,
                known_nodes[id as usize].clone(),
            );
            network.server_shutdown_signal = Some(sender);
            CentralizedCommChannel(network, PhantomData::default())
        })
    }

    fn in_flight_message_count(&self) -> Option<usize> {
        None
    }
}

impl<K: SignatureKey, E: ElectionConfig> Drop for CentralizedServerNetwork<K, E> {
    fn drop(&mut self) {
        if let Some(shutdown) = self.server_shutdown_signal.take() {
            // we try to unwrap this Arc. If we're the last one with a reference to this arc, we'll be able to unwrap this
            // if we're the last one with a reference, we should send a message on this channel as it'll gracefully shut down the server
            if let Ok(sender) = Arc::try_unwrap(shutdown) {
                sender.send(());
            }
        }
    }
}
