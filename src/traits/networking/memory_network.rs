//! In memory network simulator
//!
//! This module provides an in-memory only simulation of an actual network, useful for unit and
//! integration tests.

use super::{
    FailedToSerializeSnafu, NetworkError, NetworkReliability, NetworkingImplementation,
    NetworkingMetrics,
};
use async_compatibility_layer::{
    art::{async_block_on, async_sleep, async_spawn},
    channel::{bounded, Receiver, SendError, Sender},
};
use async_lock::{Mutex, RwLock};
use async_trait::async_trait;
use bincode::Options;
use dashmap::DashMap;
use futures::StreamExt;
use hotshot_types::{
    data::{LeafType, ProposalType},
    message::Message,
    traits::{
        metrics::{Metrics, NoMetrics},
        network::{ChannelDisconnectedSnafu, NetworkChange, TestableNetworkingImplementation},
        node_implementation::NodeTypes,
        signature_key::{SignatureKey, TestableSignatureKey},
    },
};
use hotshot_utils::bincode::bincode_opts;
use rand::Rng;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use std::{
    fmt::Debug,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};
use tracing::{debug, error, info, info_span, instrument, trace, warn, Instrument};

#[derive(Debug, Clone, Copy)]
/// dummy implementation of network reliability
pub struct DummyReliability {}
impl NetworkReliability for DummyReliability {
    fn sample_keep(&self) -> bool {
        true
    }
    fn sample_delay(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
}

/// Shared state for in-memory mock networking.
///
/// This type is responsible for keeping track of the channels to each [`MemoryNetwork`], and is
/// used to group the [`MemoryNetwork`] instances.
#[derive(custom_debug::Debug)]
pub struct MasterMap<
    TYPES: NodeTypes,
    LEAF: LeafType<NodeType = TYPES>,
    PROPOSAL: ProposalType<NodeTypes = TYPES>,
> {
    /// The list of `MemoryNetwork`s
    #[debug(skip)]
    map: DashMap<TYPES::SignatureKey, MemoryNetwork<TYPES, LEAF, PROPOSAL>>,
    /// The id of this `MemoryNetwork` cluster
    id: u64,
}

impl<
        TYPES: NodeTypes,
        LEAF: LeafType<NodeType = TYPES>,
        PROPOSAL: ProposalType<NodeTypes = TYPES>,
    > MasterMap<TYPES, LEAF, PROPOSAL>
{
    /// Create a new, empty, `MasterMap`
    pub fn new() -> Arc<MasterMap<TYPES, LEAF, PROPOSAL>> {
        Arc::new(MasterMap {
            map: DashMap::new(),
            id: rand::thread_rng().gen(),
        })
    }
}

/// Internal enum for combining streams
enum Combo<T> {
    /// Direct message
    Direct(T),
    /// Broadcast message
    Broadcast(T),
}

/// Internal state for a `MemoryNetwork` instance
struct MemoryNetworkInner<
    TYPES: NodeTypes,
    LEAF: LeafType<NodeType = TYPES>,
    PROPOSAL: ProposalType<NodeTypes = TYPES>,
> {
    /// The public key of this node
    #[allow(dead_code)]
    pub_key: TYPES::SignatureKey,
    /// Input for broadcast messages
    broadcast_input: RwLock<Option<Sender<Vec<u8>>>>,
    /// Input for direct messages
    direct_input: RwLock<Option<Sender<Vec<u8>>>>,
    /// Output for broadcast messages
    broadcast_output: Mutex<Receiver<Message<TYPES, LEAF, PROPOSAL>>>,
    /// Output for direct messages
    direct_output: Mutex<Receiver<Message<TYPES, LEAF, PROPOSAL>>>,
    /// The master map
    master_map: Arc<MasterMap<TYPES, LEAF, PROPOSAL>>,

    /// Input for network change messages
    network_changes_input: RwLock<Option<Sender<NetworkChange<TYPES::SignatureKey>>>>,
    /// Output for network change messages
    network_changes_output: Mutex<Receiver<NetworkChange<TYPES::SignatureKey>>>,

    /// Count of messages that are in-flight (send but not processed yet)
    in_flight_message_count: AtomicUsize,

    /// The networking metrics we're keeping track of
    metrics: NetworkingMetrics,
}

/// In memory only network simulator.
///
/// This provides an in memory simulation of a networking implementation, allowing nodes running on
/// the same machine to mock networking while testing other functionality.
///
/// Under the hood, this simply maintains mpmc channels to every other `MemoryNetwork` insane of the
/// same group.
#[derive(Clone)]
pub struct MemoryNetwork<
    TYPES: NodeTypes,
    LEAF: LeafType<NodeType = TYPES>,
    PROPOSAL: ProposalType<NodeTypes = TYPES>,
> {
    /// The actual internal state
    inner: Arc<MemoryNetworkInner<TYPES, LEAF, PROPOSAL>>,
}

impl<
        TYPES: NodeTypes,
        LEAF: LeafType<NodeType = TYPES>,
        PROPOSAL: ProposalType<NodeTypes = TYPES>,
    > Debug for MemoryNetwork<TYPES, LEAF, PROPOSAL>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryNetwork")
            .field("inner", &"inner")
            .finish()
    }
}

impl<
        TYPES: NodeTypes,
        LEAF: LeafType<NodeType = TYPES>,
        PROPOSAL: ProposalType<NodeTypes = TYPES>,
    > MemoryNetwork<TYPES, LEAF, PROPOSAL>
{
    /// Creates a new `MemoryNetwork` and hooks it up to the group through the provided `MasterMap`
    #[instrument(skip(metrics))]
    pub fn new(
        pub_key: TYPES::SignatureKey,
        metrics: Box<dyn Metrics>,
        master_map: Arc<MasterMap<TYPES, LEAF, PROPOSAL>>,
        reliability_config: Option<Arc<dyn 'static + NetworkReliability>>,
    ) -> MemoryNetwork<TYPES, LEAF, PROPOSAL> {
        info!("Attaching new MemoryNetwork");
        let (broadcast_input, broadcast_task_recv) = bounded(128);
        let (direct_input, direct_task_recv) = bounded(128);
        let (broadcast_task_send, broadcast_output) = bounded(128);
        let (direct_task_send, direct_output) = bounded(128);
        let (network_changes_input, network_changes_output) = bounded(128);
        let in_flight_message_count = AtomicUsize::new(0);
        trace!("Channels open, spawning background task");

        async_spawn(
            async move {
                debug!("Starting background task");
                // direct input is right stream
                let direct = direct_task_recv.into_stream().map(Combo::<Vec<u8>>::Direct);
                // broadcast input is left stream
                let broadcast = broadcast_task_recv
                    .into_stream()
                    .map(Combo::<Vec<u8>>::Broadcast);
                // Combine the streams
                let mut combined = futures::stream::select(direct, broadcast);
                trace!("Entering processing loop");
                while let Some(message) = combined.next().await {
                    match message {
                        Combo::Direct(vec) => {
                            trace!(?vec, "Incoming direct message");
                            // Attempt to decode message
                            let x = bincode_opts().deserialize(&vec);
                            match x {
                                Ok(x) => {
                                    let dts = direct_task_send.clone();
                                    if let Some(r) = reliability_config.clone() {
                                        async_spawn(async move {
                                            if r.sample_keep() {
                                                let delay = r.sample_delay();
                                                if delay > std::time::Duration::ZERO {
                                                    async_sleep(delay).await;
                                                }
                                                let res = dts.send(x).await;
                                                if res.is_ok() {
                                                    trace!("Passed message to output queue");
                                                } else {
                                                    error!("Output queue receivers are shutdown");
                                                }
                                            } else {
                                                warn!("dropping packet!");
                                            }
                                        });
                                    } else {
                                        let res = dts.send(x).await;
                                        if res.is_ok() {
                                            trace!("Passed message to output queue");
                                        } else {
                                            error!("Output queue receivers are shutdown");
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!(?e, "Failed to decode incoming message, skipping");
                                }
                            }
                        }
                        Combo::Broadcast(vec) => {
                            trace!(?vec, "Incoming broadcast message");
                            // Attempt to decode message
                            let x = bincode_opts().deserialize(&vec);
                            match x {
                                Ok(x) => {
                                    let bts = broadcast_task_send.clone();
                                    if let Some(r) = reliability_config.clone() {
                                        async_spawn(async move {
                                            if r.sample_keep() {
                                                let delay = r.sample_delay();
                                                if delay > std::time::Duration::ZERO {
                                                    async_sleep(delay).await;
                                                }
                                                let res = bts.send(x).await;
                                                if res.is_ok() {
                                                    trace!("Passed message to output queue");
                                                } else {
                                                    warn!("dropping packet!");
                                                }
                                            }
                                        });
                                    } else {
                                        let res = bts.send(x).await;
                                        if res.is_ok() {
                                            trace!("Passed message to output queue");
                                        } else {
                                            warn!("dropping packet!");
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!(?e, "Failed to decode incoming message, skipping");
                                }
                            }
                        }
                    }
                }
                error!("Stream shutdown");
            }
            .instrument(info_span!("MemoryNetwork Background task", map = ?master_map)),
        );
        trace!("Notifying other networks of the new connected peer");
        for other in master_map.map.iter() {
            async_block_on(
                other
                    .value()
                    .network_changes_input(NetworkChange::NodeConnected(pub_key.clone())),
            )
            .expect("Could not deliver message");
        }
        trace!("Task spawned, creating MemoryNetwork");
        let mn = MemoryNetwork {
            inner: Arc::new(MemoryNetworkInner {
                pub_key: pub_key.clone(),
                broadcast_input: RwLock::new(Some(broadcast_input)),
                direct_input: RwLock::new(Some(direct_input)),
                broadcast_output: Mutex::new(broadcast_output),
                direct_output: Mutex::new(direct_output),
                master_map: master_map.clone(),
                network_changes_input: RwLock::new(Some(network_changes_input)),
                network_changes_output: Mutex::new(network_changes_output),
                in_flight_message_count,
                metrics: NetworkingMetrics::new(metrics),
            }),
        };
        master_map.map.insert(pub_key, mn.clone());
        trace!("Master map updated");

        mn
    }

    /// Send a [`Vec<u8>`] message to the inner `broadcast_input`
    async fn broadcast_input(&self, message: Vec<u8>) -> Result<(), SendError<Vec<u8>>> {
        self.inner
            .in_flight_message_count
            .fetch_add(1, Ordering::Relaxed);
        let input = self.inner.broadcast_input.read().await;
        if let Some(input) = &*input {
            self.inner.metrics.outgoing_message_count.add(1);
            input.send(message).await
        } else {
            Err(SendError(message))
        }
    }

    /// Send a [`Vec<u8>`] message to the inner `direct_input`
    async fn direct_input(&self, message: Vec<u8>) -> Result<(), SendError<Vec<u8>>> {
        self.inner
            .in_flight_message_count
            .fetch_add(1, Ordering::Relaxed);
        let input = self.inner.direct_input.read().await;
        if let Some(input) = &*input {
            self.inner.metrics.outgoing_message_count.add(1);
            input.send(message).await
        } else {
            Err(SendError(message))
        }
    }

    /// Send a [`NetworkChange`] message to the inner `network_changes_input`
    async fn network_changes_input(
        &self,
        message: NetworkChange<TYPES::SignatureKey>,
    ) -> Result<(), SendError<NetworkChange<TYPES::SignatureKey>>> {
        let input = self.inner.network_changes_input.read().await;
        if let Some(input) = &*input {
            input.send(message).await
        } else {
            Err(SendError(message))
        }
    }
}

impl<
        TYPES: NodeTypes,
        LEAF: LeafType<NodeType = TYPES>,
        PROPOSAL: ProposalType<NodeTypes = TYPES>,
    > TestableNetworkingImplementation<TYPES, LEAF, PROPOSAL>
    for MemoryNetwork<TYPES, LEAF, PROPOSAL>
where
    TYPES::SignatureKey: TestableSignatureKey,
{
    fn generator(
        _expected_node_count: usize,
        _num_bootstrap: usize,
    ) -> Box<dyn Fn(u64) -> Self + 'static> {
        let master: Arc<_> = MasterMap::new();
        Box::new(move |node_id| {
            let privkey = TYPES::SignatureKey::generate_test_key(node_id);
            let pubkey = TYPES::SignatureKey::from_private(&privkey);
            MemoryNetwork::new(pubkey, NoMetrics::new(), master.clone(), None)
        })
    }

    fn in_flight_message_count(&self) -> Option<usize> {
        Some(self.inner.in_flight_message_count.load(Ordering::Relaxed))
    }
}

#[async_trait]
impl<
        TYPES: NodeTypes,
        LEAF: LeafType<NodeType = TYPES>,
        PROPOSAL: ProposalType<NodeTypes = TYPES>,
    > NetworkingImplementation<TYPES, LEAF, PROPOSAL> for MemoryNetwork<TYPES, LEAF, PROPOSAL>
{
    #[instrument(name = "MemoryNetwork::broadcast_message")]
    async fn broadcast_message(
        &self,
        message: Message<TYPES, LEAF, PROPOSAL>,
    ) -> Result<(), NetworkError> {
        debug!(?message, "Broadcasting message");
        // Bincode the message
        let vec = bincode_opts()
            .serialize(&message)
            .context(FailedToSerializeSnafu)?;
        trace!("Message bincoded, sending");
        for node in self.inner.master_map.map.iter() {
            let (key, node) = node.pair();
            trace!(?key, "Sending message to node");
            let res = node.broadcast_input(vec.clone()).await;
            match res {
                Ok(_) => {
                    self.inner.metrics.outgoing_message_count.add(1);
                    trace!(?key, "Delivered message to remote");
                }
                Err(e) => {
                    self.inner.metrics.message_failed_to_send.add(1);
                    error!(?e, ?key, "Error sending broadcast message to node");
                }
            }
        }
        Ok(())
    }

    #[instrument(name = "MemoryNetwork::ready")]
    async fn ready(&self) -> bool {
        true
    }

    #[instrument(name = "MemoryNetwork::message_node")]
    async fn message_node(
        &self,
        message: Message<TYPES, LEAF, PROPOSAL>,
        recipient: TYPES::SignatureKey,
    ) -> Result<(), NetworkError> {
        debug!(?message, ?recipient, "Sending direct message");
        // Bincode the message
        let vec = bincode_opts()
            .serialize(&message)
            .context(FailedToSerializeSnafu)?;
        trace!("Message bincoded, finding recipient");
        if let Some(node) = self.inner.master_map.map.get(&recipient) {
            let node = node.value();
            let res = node.direct_input(vec).await;
            match res {
                Ok(_) => {
                    self.inner.metrics.outgoing_message_count.add(1);
                    trace!(?recipient, "Delivered message to remote");
                    Ok(())
                }
                Err(e) => {
                    self.inner.metrics.message_failed_to_send.add(1);
                    error!(?e, ?recipient, "Error delivering direct message");
                    Err(NetworkError::CouldNotDeliver)
                }
            }
        } else {
            self.inner.metrics.message_failed_to_send.add(1);
            error!(?recipient, ?self.inner.master_map.map, "Node does not exist in map");
            Err(NetworkError::NoSuchNode)
        }
    }

    #[instrument(name = "MemoryNetwork::broadcast_queue")]
    async fn broadcast_queue(&self) -> Result<Vec<Message<TYPES, LEAF, PROPOSAL>>, NetworkError> {
        let ret = self
            .inner
            .broadcast_output
            .lock()
            .await
            .drain_at_least_one()
            .await
            .context(ChannelDisconnectedSnafu)?;
        self.inner
            .in_flight_message_count
            .fetch_sub(ret.len(), Ordering::Relaxed);
        self.inner.metrics.incoming_message_count.add(ret.len());
        Ok(ret)
    }

    #[instrument(name = "MemoryNetwork::next_broadcast")]
    async fn next_broadcast(&self) -> Result<Message<TYPES, LEAF, PROPOSAL>, NetworkError> {
        let ret = self
            .inner
            .broadcast_output
            .lock()
            .await
            .recv()
            .await
            .map_err(|_| NetworkError::ShutDown)?;
        self.inner
            .in_flight_message_count
            .fetch_sub(1, Ordering::Relaxed);
        self.inner.metrics.incoming_message_count.add(1);
        Ok(ret)
    }

    #[instrument(name = "MemoryNetwork::direct_queue")]
    async fn direct_queue(&self) -> Result<Vec<Message<TYPES, LEAF, PROPOSAL>>, NetworkError> {
        let ret = self
            .inner
            .direct_output
            .lock()
            .await
            .drain_at_least_one()
            .await
            .context(ChannelDisconnectedSnafu)?;
        self.inner
            .in_flight_message_count
            .fetch_sub(ret.len(), Ordering::Relaxed);
        self.inner.metrics.incoming_message_count.add(ret.len());
        Ok(ret)
    }

    #[instrument(name = "MemoryNetwork::next_direct")]
    async fn next_direct(&self) -> Result<Message<TYPES, LEAF, PROPOSAL>, NetworkError> {
        let ret = self
            .inner
            .direct_output
            .lock()
            .await
            .recv()
            .await
            .map_err(|_| NetworkError::ShutDown)?;
        self.inner
            .in_flight_message_count
            .fetch_sub(1, Ordering::Relaxed);
        self.inner.metrics.incoming_message_count.add(1);
        Ok(ret)
    }

    async fn known_nodes(&self) -> Vec<TYPES::SignatureKey> {
        self.inner
            .master_map
            .map
            .iter()
            .map(|x| x.key().clone())
            .collect()
    }

    #[instrument(name = "MemoryNetwork::network_changes")]
    async fn network_changes(
        &self,
    ) -> Result<Vec<NetworkChange<TYPES::SignatureKey>>, NetworkError> {
        self.inner
            .network_changes_output
            .lock()
            .await
            .drain_at_least_one()
            .await
            .context(ChannelDisconnectedSnafu)
    }

    async fn shut_down(&self) {
        *self.inner.broadcast_input.write().await = None;
        *self.inner.direct_input.write().await = None;
        *self.inner.network_changes_input.write().await = None;
    }

    async fn put_record(
        &self,
        _key: impl Serialize + Send + Sync + 'static,
        _value: impl Serialize + Send + Sync + 'static,
    ) -> Result<(), NetworkError> {
        unimplemented!("MemoryNetwork: Put record not supported")
    }

    async fn get_record<V: for<'a> Deserialize<'a>>(
        &self,
        _key: impl Serialize + Send + Sync + 'static,
    ) -> Result<V, NetworkError> {
        unimplemented!("MemoryNetwork: Get record not supported")
    }

    async fn notify_of_subsequent_leader(
        &self,
        _pk: TYPES::SignatureKey,
        _is_cancelled: Arc<AtomicBool>,
    ) {
        // do nothing
    }
}

// Tests have been commented out, so `mod tests` isn't used.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        demos::dentry::{DEntryBlock, DEntryState, DEntryTransaction},
        traits::election::static_committee::{StaticElectionConfig, StaticVoteToken},
    };

    use hotshot_types::traits::state::ValidatingConsensus;
    use hotshot_types::{
        data::ViewNumber,
        traits::{
            node_implementation::ApplicationMetadata,
            signature_key::ed25519::{Ed25519Priv, Ed25519Pub},
        },
    };

    /// application metadata stub
    #[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
    pub struct TestMetaData {}

    impl ApplicationMetadata for TestMetaData {}

    #[derive(
        Copy,
        Clone,
        Debug,
        Default,
        Hash,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        serde::Serialize,
        serde::Deserialize,
    )]
    struct Test {
        message: u64,
    }

    impl NodeTypes for Test {
        // TODO (da) can this be SequencingConsensus?
        type ConsensusType = ValidatingConsensus;
        type Time = ViewNumber;
        type BlockType = DEntryBlock;
        type SignatureKey = Ed25519Pub;
        type VoteTokenType = StaticVoteToken<Ed25519Pub>;
        type Transaction = DEntryTransaction;
        type ElectionConfigType = StaticElectionConfig;
        type StateType = DEntryState;
        type ApplicationMetadataType = TestMetaData;
    }

    #[instrument]
    fn get_pubkey() -> Ed25519Pub {
        let priv_key = Ed25519Priv::generate();
        Ed25519Pub::from_private(&priv_key)
    }

    // // Spawning a single MemoryNetwork should produce no errors
    // #[cfg_attr(
    //     feature = "tokio-executor",
    //     tokio::test(flavor = "multi_thread", worker_threads = 2)
    // )]
    // #[cfg_attr(feature = "async-std-executor", async_std::test)]
    // #[instrument]
    // async fn spawn_single<ELECTION: Election<>>() {
    //     setup_logging();
    //     let group: Arc<MasterMap<Test, ValidatingLeaf<Test>, ValidatingProposal<Test, ELECTION>>> =
    //         MasterMap::new();
    //     trace!(?group);
    //     let pub_key = get_pubkey();
    //     let _network = MemoryNetwork::new(pub_key, NoMetrics::new(), group, Option::None);
    // }

    // // Spawning a two MemoryNetworks and connecting them should produce no errors
    // #[cfg_attr(
    //     feature = "tokio-executor",
    //     tokio::test(flavor = "multi_thread", worker_threads = 2)
    // )]
    // #[cfg_attr(feature = "async-std-executor", async_std::test)]
    // #[instrument]
    // async fn spawn_double() {
    //     setup_logging();
    //     let group: Arc<MasterMap<Test, ValidatingLeaf<Test>, ValidatingProposal<Test, ELECTION>>> =
    //         MasterMap::new();
    //     trace!(?group);
    //     let pub_key_1 = get_pubkey();
    //     let _network_1 =
    //         MemoryNetwork::new(pub_key_1, NoMetrics::new(), group.clone(), Option::None);
    //     let pub_key_2 = get_pubkey();
    //     let _network_2 = MemoryNetwork::new(pub_key_2, NoMetrics::new(), group, Option::None);
    // }

    // Check to make sure direct queue works
    // #[cfg_attr(
    //     feature = "tokio-executor",
    //     tokio::test(flavor = "multi_thread", worker_threads = 2)
    // )]
    // #[cfg_attr(feature = "async-std-executor", async_std::test)]
    // #[instrument]
    // async fn direct_queue() {
    //     setup_logging();
    //     // Create some dummy messages
    //     let messages: Vec<Message<Test>> = (0..5).map(|x| ()).collect();
    //     // Make and connect the networking instances
    //     let group: Arc<MasterMap<Test,LEAF,PROPOSAL>> = MasterMap::new();
    //     trace!(?group);
    //     let pub_key_1 = get_pubkey();
    //     let network1 = MemoryNetwork::new(pub_key_1, group.clone(), Option::None);
    //     let pub_key_2 = get_pubkey();
    //     let network2 = MemoryNetwork::new(pub_key_2, group, Option::None);

    //     // Test 1 -> 2
    //     // Send messages
    //     for message in &messages {
    //         network1
    //             .message_node(message.clone(), pub_key_2)
    //             .await
    //             .expect("Failed to message node");
    //     }
    //     let mut output = Vec::new();
    //     while output.len() < messages.len() {
    //         let message = network2
    //             .next_direct()
    //             .await
    //             .expect("Failed to receive message");
    //         output.push(message);
    //     }
    //     output.sort();
    //     // Check for equality
    //     assert_eq!(output, messages);

    //     // Test 2 -> 1
    //     // Send messages
    //     for message in &messages {
    //         network2
    //             .message_node(message.clone(), pub_key_1)
    //             .await
    //             .expect("Failed to message node");
    //     }
    //     let mut output = Vec::new();
    //     while output.len() < messages.len() {
    //         let message = network1
    //             .next_direct()
    //             .await
    //             .expect("Failed to receive message");
    //         output.push(message);
    //     }
    //     output.sort();
    //     // Check for equality
    //     assert_eq!(output, messages);
    // }

    // // Check to make sure direct queue works
    // #[cfg_attr(
    //     feature = "tokio-executor",
    //     tokio::test(flavor = "multi_thread", worker_threads = 2)
    // )]
    // #[cfg_attr(feature = "async-std-executor", async_std::test)]
    // #[instrument]
    // async fn broadcast_queue() {
    //     setup_logging();
    //     // Create some dummy messages
    //     let messages: Vec<Test> = (0..5).map(|x| Test { message: x }).collect();
    //     // Make and connect the networking instances
    //     let group: Arc<MasterMap<Test,LEAF,PROPOSAL>> = MasterMap::new();
    //     trace!(?group);
    //     let pub_key_1 = get_pubkey();
    //     let network1 = MemoryNetwork::new(pub_key_1, group.clone(), Option::None);
    //     let pub_key_2 = get_pubkey();
    //     let network2 = MemoryNetwork::new(pub_key_2, group, Option::None);

    //     // Test 1 -> 2
    //     // Send messages
    //     for message in &messages {
    //         network1
    //             .broadcast_message(message.clone())
    //             .await
    //             .expect("Failed to message node");
    //     }
    //     let mut output = Vec::new();
    //     while output.len() < messages.len() {
    //         let message = network2
    //             .next_broadcast()
    //             .await
    //             .expect("Failed to receive message");
    //         output.push(message);
    //     }
    //     output.sort();
    //     // Check for equality
    //     assert_eq!(output, messages);

    //     // Test 2 -> 1
    //     // Send messages
    //     for message in &messages {
    //         network2
    //             .broadcast_message(message.clone())
    //             .await
    //             .expect("Failed to message node");
    //     }
    //     let mut output = Vec::new();
    //     while output.len() < messages.len() {
    //         let message = network1
    //             .next_broadcast()
    //             .await
    //             .expect("Failed to receive message");
    //         output.push(message);
    //     }
    //     output.sort();
    //     // Check for equality
    //     assert_eq!(output, messages);
    // }
    // #[cfg_attr(
    //     feature = "tokio-executor",
    //     tokio::test(flavor = "multi_thread", worker_threads = 2)
    // )]
    // #[cfg_attr(feature = "async-std-executor", async_std::test)]
    // #[instrument]
    // async fn test_in_flight_message_count() {
    //     setup_logging();
    //     // Create some dummy messages
    //     let messages: Vec<Test> = (0..5).map(|x| Test { message: x }).collect();
    //     let group: Arc<MasterMap<Test, Ed25519Pub>> = MasterMap::new();
    //     trace!(?group);
    //     let pub_key_1 = get_pubkey();
    //     let network1 = MemoryNetwork::new(pub_key_1, group.clone(), Option::None);
    //     let pub_key_2 = get_pubkey();
    //     let network2 = MemoryNetwork::new(pub_key_2, group, Option::None);

    //     assert_eq!(network1.in_flight_message_count(), Some(0));
    //     assert_eq!(network2.in_flight_message_count(), Some(0));

    //     for (count, message) in messages.iter().enumerate() {
    //         network1
    //             .message_node(message.clone(), pub_key_2)
    //             .await
    //             .unwrap();
    //         // network 2 has received `count` broadcast messages and `count + 1` direct messages
    //         assert_eq!(network2.in_flight_message_count(), Some(count + count + 1));

    //         network2.broadcast_message(message.clone()).await.unwrap();
    //         // network 1 has received `count` broadcast messages
    //         assert_eq!(network1.in_flight_message_count(), Some(count + 1));

    //         // network 2 has received `count + 1` broadcast messages and `count + 1` direct messages
    //         assert_eq!(network2.in_flight_message_count(), Some((count + 1) * 2));
    //     }

    //     for count in (0..messages.len()).rev() {
    //         network1.next_broadcast().await.unwrap();
    //         assert_eq!(network1.in_flight_message_count(), Some(count));

    //         network2.next_broadcast().await.unwrap();
    //         network2.next_direct().await.unwrap();
    //         assert_eq!(network2.in_flight_message_count(), Some(count * 2));
    //     }

    //     assert_eq!(network1.in_flight_message_count(), Some(0));
    //     assert_eq!(network2.in_flight_message_count(), Some(0));
    // }
}
