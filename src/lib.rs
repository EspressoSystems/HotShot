#![warn(
    clippy::all,
    clippy::pedantic,
    rust_2018_idioms,
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::panic
)]
#![allow(
    clippy::option_if_let_else,
    clippy::must_use_candidate,
    clippy::module_name_repetitions,
    clippy::similar_names,
    clippy::unused_self,
    clippy::unused_async, // For API reasons
)]
// Temporary
#![allow(clippy::cast_possible_truncation)]
// Temporary, should be disabled after the completion of the NodeImplementation refactor
#![allow(clippy::type_complexity)]
//! Provides a generic rust implementation of the `HotShot` BFT protocol
//!
//! See the [protocol documentation](https://github.com/EspressoSystems/hotshot-spec) for a protocol description.

// Documentation module
#[cfg(feature = "docs")]
pub mod documentation;

/// Contains structures and functions for committee election
pub mod committee;
pub mod data;
#[cfg(any(feature = "demo"))]
pub mod demos;
/// Contains traits consumed by [`HotShot`]
pub mod traits;
/// Contains types used by the crate
pub mod types;

mod tasks;
mod utils;

use crate::{
    data::{LeafHash, QuorumCertificate},
    traits::{BlockContents, NetworkingImplementation, NodeImplementation, Storage},
    types::{Event, EventType, HotShotHandle},
};

// mod state_machine;

use async_std::sync::{Mutex, RwLock, RwLockUpgradableReadGuard};
use async_trait::async_trait;
use flume::{Receiver, Sender};
use hotshot_consensus::{Consensus, SendToTasks, View, ViewInner, ViewQueue};
use hotshot_types::{
    data::{create_verify_hash, VerifyHash, ViewNumber},
    error::{NetworkFaultSnafu, StorageSnafu},
    message::{ConsensusMessage, DataMessage, Message},
    traits::{
        election::Election,
        network::{NetworkChange, NetworkError},
        node_implementation::TypeMap,
        signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey},
        stateful_handler::StatefulHandler,
        storage::StoredView,
    },
};
use hotshot_utils::broadcast::BroadcastSender;
use snafu::ResultExt;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Debug,
    num::NonZeroUsize,
    sync::Arc,
    time::Duration,
};
use tracing::{debug, error, info, instrument, trace, warn};

// -- Rexports
// External
/// Reexport rand crate
pub use rand;
// Internal
/// Reexport error type
pub use hotshot_types::error::HotShotError;

/// Length, in bytes, of a 512 bit hash
pub const H_512: usize = 64;
/// Length, in bytes, of a 256 bit hash
pub const H_256: usize = 32;

/// Convenience type alias
type Result<T> = std::result::Result<T, HotShotError>;

/// the type of consensus to run. Either:
/// wait for a signal to start a view,
/// or constantly run
/// you almost always want continuous
/// incremental is just for testing
#[derive(Debug, Clone, Copy)]
pub enum ExecutionType {
    /// constantly increment view as soon as view finishes
    Continuous,
    /// wait for a signal
    Incremental,
}

/// Holds configuration for a `HotShot`
#[derive(Debug, Clone)]
pub struct HotShotConfig<P: SignatureKey> {
    /// Whether to run one view or continuous views
    pub execution_type: ExecutionType,
    /// Total number of nodes in the network
    pub total_nodes: NonZeroUsize,
    /// Nodes required to reach a decision
    pub threshold: NonZeroUsize,
    /// Maximum transactions per block
    pub max_transactions: NonZeroUsize,
    /// List of known node's public keys, including own, sorted by nonce ()
    pub known_nodes: Vec<P>,
    /// Base duration for next-view timeout, in milliseconds
    pub next_view_timeout: u64,
    /// The exponential backoff ration for the next-view timeout
    pub timeout_ratio: (u64, u64),
    /// The delay a leader inserts before starting pre-commit, in milliseconds
    pub round_start_delay: u64,
    /// Delay after init before starting consensus, in milliseconds
    pub start_delay: u64,
    /// Number of network bootstrap nodes
    pub num_bootstrap: usize,
    /// The minimum amount of time a leader has to wait to start a round
    pub propose_min_round_time: Duration,
    /// The maximum amount of time a leader can wait to start a round
    pub propose_max_round_time: Duration,
}

/// Holds the state needed to participate in `HotShot` consensus
pub struct HotShotInner<I: NodeImplementation<N>, const N: usize> {
    /// The public key of this node
    public_key: I::SignatureKey,

    /// The private key of this node
    private_key: <I::SignatureKey as SignatureKey>::PrivateKey,

    /// The public keys for the cluster
    /// TODO: Move the functionality this expresses into the election trait
    cluster_public_keys: HashSet<I::SignatureKey>,

    /// Configuration items for this hotshot instance
    config: HotShotConfig<I::SignatureKey>,

    /// Networking interface for this hotshot instance
    networking: I::Networking,

    /// This `HotShot` instance's storage backend
    storage: I::Storage,

    /// This `HotShot` instance's stateful callback handler
    stateful_handler: Mutex<I::StatefulHandler>,

    /// This `HotShot` instance's election backend
    election: HotShotElectionState<I::SignatureKey, I::Election, N>,

    /// Sender for [`Event`]s
    event_sender: RwLock<Option<BroadcastSender<Event<I::Block, I::State, N>>>>,

    /// Senders to the background tasks.
    background_task_handle: tasks::TaskHandle,
}

/// Contains the state of the election of the current [`HotShot`].
struct HotShotElectionState<P: SignatureKey, E: Election<P, N>, const N: usize> {
    /// An instance of the election
    election: E,
    /// The inner state of the election
    #[allow(dead_code)]
    state: E::State,
    /// The stake table of the election
    stake_table: E::StakeTable,
}

/// Thread safe, shared view of a `HotShot`
#[derive(Clone)]
pub struct HotShot<I: NodeImplementation<N> + Send + Sync + 'static, const N: usize> {
    /// Handle to internal hotshot implementation
    inner: Arc<HotShotInner<I, N>>,

    /// Transactions
    /// (this is shared btwn hotshot and `Consensus`)
    transactions: Arc<RwLock<Vec<<I as TypeMap<N>>::Transaction>>>,

    /// The hotstuff implementation
    hotstuff: Arc<RwLock<Consensus<I, N>>>,

    /// for sending things to the task
    /// right now only the replica task
    send_to_tasks: Arc<RwLock<SendToTasks<I, N>>>,

    /// for sending consensus messages from handle_direct_consensus_message task
    /// to the next leader task. NOTE: only Vote, NextView should be sent over this channel
    /// TODO is there a way to enforce this with type safety
    send_next_leader: Sender<ConsensusMessage<I::Block, I::State, N>>,

    /// for recv-ing consensus messages from handle_direct_consensus_message task
    recv_next_leader: Receiver<ConsensusMessage<I::Block, I::State, N>>,

    /// uid for instrumentation
    id: u64,
}

impl<I: NodeImplementation<N> + Sync + Send + 'static, const N: usize> HotShot<I, N> {
    /// Creates a new hotshot with the given configuration options and sets it up with the given
    /// genesis block
    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    #[instrument(skip(private_key, cluster_public_keys, networking, storage, election))]
    pub async fn new(
        cluster_public_keys: impl IntoIterator<Item = I::SignatureKey>,
        public_key: I::SignatureKey,
        private_key: <I::SignatureKey as SignatureKey>::PrivateKey,
        nonce: u64,
        config: HotShotConfig<I::SignatureKey>,
        networking: I::Networking,
        storage: I::Storage,
        handler: I::StatefulHandler,
        election: I::Election,
    ) -> Result<Self> {
        info!("Creating a new hotshot");

        let election = {
            let state =
                <<I as NodeImplementation<N>>::Election as Election<I::SignatureKey, N>>::State::default();
            let stake_table = election.get_stake_table(&state);
            HotShotElectionState {
                election,
                state,
                stake_table,
            }
        };
        let inner: HotShotInner<I, N> = HotShotInner {
            public_key,
            private_key,
            config,
            networking,
            storage,
            stateful_handler: Mutex::new(handler),
            election,
            event_sender: RwLock::default(),
            background_task_handle: tasks::TaskHandle::default(),
            cluster_public_keys: cluster_public_keys.into_iter().collect(),
        };

        let (send_next_leader, recv_next_leader) = flume::unbounded();

        let anchored = inner
            .storage
            .get_anchored_view()
            .await
            .context(StorageSnafu)?;
        let mut genesis_map = BTreeMap::default();

        genesis_map.insert(
            anchored.view_number,
            View {
                view_inner: ViewInner::Leaf {
                    leaf: anchored.qc.leaf_hash,
                },
            },
        );

        let mut genesis_leaves = HashMap::new();
        genesis_leaves.insert(anchored.qc.leaf_hash, anchored.clone().into());

        let start_view = anchored.view_number + 1;

        // TODO jr add constructor and private the consensus fields
        // and also ViewNumber's contained number
        let hotstuff = Consensus {
            state_map: genesis_map,
            cur_view: start_view,
            last_decided_view: anchored.view_number,
            transactions: Arc::default(),
            undecided_leaves: genesis_leaves,
            // TODO unclear if this is correct
            // maybe we need 3 views?
            locked_view: anchored.view_number,
            high_qc: anchored.qc,
        };
        let hotstuff = Arc::new(RwLock::new(hotstuff));
        let txns = hotstuff.read().await.get_transactions();

        Ok(Self {
            id: nonce,
            inner: Arc::new(inner),
            transactions: txns,
            // TODO check this is what we want.
            hotstuff,
            send_next_leader,
            recv_next_leader,
            send_to_tasks: Arc::new(RwLock::new(SendToTasks::new(start_view))),
        })
    }

    /// Sends an event over an event broadcaster if one is registered, does nothing otherwise
    ///
    /// Returns `true` if the event was send, `false` otherwise
    pub async fn send_event(&self, event: Event<I::Block, I::State, N>) -> bool {
        if let Some(c) = self.inner.event_sender.read().await.as_ref() {
            if let Err(e) = c.send_async(event).await {
                warn!(?e, "Could not send event to the registered broadcaster");
            } else {
                return true;
            }
        }
        false
    }

    /// Marks a given view number as timed out. This should be called a fixed period after a round is started.
    ///
    /// If the round has already ended then this function will essentially be a no-op. Otherwise `run_round` will return shortly after this function is called.
    /// # Panics
    /// Panics if the current view is not in the channel map
    pub async fn timeout_view(
        &self,
        current_view: ViewNumber,
        send_replica: Sender<ConsensusMessage<I::Block, I::State, N>>,
    ) {
        let msg = ConsensusMessage::<I::Block, I::State, N>::NextViewInterrupt(current_view);
        if self.send_next_leader.send_async(msg.clone()).await.is_err() {
            error!("Error timing out next leader");
        };
        // NOTE this should always exist
        if send_replica.send_async(msg).await.is_err() {
            error!("Error timing out replica");
        };
    }

    /// Publishes a transaction to the network
    ///
    /// # Errors
    ///
    /// Will generate an error if an underlying network error occurs
    #[instrument(skip(self), err)]
    pub async fn publish_transaction_async(
        &self,
        transaction: <<I as NodeImplementation<N>>::Block as BlockContents<N>>::Transaction,
    ) -> Result<()> {
        // Add the transaction to our own queue first
        trace!("Adding transaction to our own queue");
        // Wrap up a message
        let message = DataMessage::SubmitTransaction(transaction);
        let network_result = self
            .send_broadcast_message(message.clone())
            .await
            .context(NetworkFaultSnafu);
        if let Err(e) = network_result {
            warn!(?e, ?message, "Failed to publish a transaction");
        };
        debug!(?message, "Message broadcasted");
        Ok(())
    }

    /// Returns a copy of the state
    ///
    /// # Panics
    ///
    /// Panics if internal state for consensus is inconsistent
    pub async fn get_state(&self) -> I::State {
        self.hotstuff.read().await.get_decided_leaf().state
    }

    /// Initializes a new hotshot and does the work of setting up all the background tasks
    ///
    /// Assumes networking implementation is already primed.
    ///
    /// Underlying `HotShot` instance starts out paused, and must be unpaused
    ///
    /// Upon encountering an unrecoverable error, such as a failure to send to a broadcast channel,
    /// the `HotShot` instance will log the error and shut down.
    ///
    /// # Errors
    ///
    /// Will return an error when the storage failed to insert the first `QuorumCertificate`
    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    pub async fn init(
        cluster_public_keys: impl IntoIterator<Item = I::SignatureKey>,
        public_key: I::SignatureKey,
        private_key: <I::SignatureKey as SignatureKey>::PrivateKey,
        node_id: u64,
        config: HotShotConfig<I::SignatureKey>,
        networking: I::Networking,
        storage: I::Storage,
        handler: I::StatefulHandler,
        election: I::Election,
    ) -> Result<HotShotHandle<I, N>> {
        // Save a clone of the storage for the handle
        let hotshot = Self::new(
            cluster_public_keys,
            public_key,
            private_key,
            node_id,
            config,
            networking,
            storage,
            handler,
            election,
        )
        .await?;
        let handle = tasks::spawn_all(&hotshot).await;
        // TODO this is where we should spin up

        Ok(handle)
    }

    /// Send a broadcast message.
    ///
    /// This is an alias for `hotshot.inner.networking.broadcast_message(msg.into())`.
    ///
    /// # Errors
    ///
    /// Will return any errors that the underlying `broadcast_message` can return.
    pub async fn send_broadcast_message(
        &self,
        kind: impl Into<<I as TypeMap<N>>::MessageKind>,
    ) -> std::result::Result<(), NetworkError> {
        self.inner
            .networking
            .broadcast_message(Message {
                sender: self.inner.public_key.clone(),
                kind: kind.into(),
            })
            .await
    }

    /// Send a direct message to a given recipient.
    ///
    /// This is an alias for `hotshot.inner.networking.message_node(msg.into(), recipient)`.
    ///
    /// # Errors
    ///
    /// Will return any errors that the underlying `message_node` can return.
    pub async fn send_direct_message(
        &self,
        kind: impl Into<<I as TypeMap<N>>::MessageKind>,
        recipient: I::SignatureKey,
    ) -> std::result::Result<(), NetworkError> {
        self.inner
            .networking
            .message_node(
                Message {
                    sender: self.inner.public_key.clone(),
                    kind: kind.into(),
                },
                recipient,
            )
            .await
    }

    /// Handle an incoming [`ConsensusMessage`] that was broadcasted on the network.
    async fn handle_broadcast_consensus_message(
        &self,
        msg: <I as TypeMap<N>>::ConsensusMessage,
        _sender: I::SignatureKey,
    ) {
        // TODO validate incoming data message based on sender signature key
        let msg_view_number = msg.view_number();

        match msg {
            ConsensusMessage::Proposal(_) => {
                let channel_map = self.send_to_tasks.upgradable_read().await;

                // skip if the proposal is stale
                if msg_view_number < channel_map.cur_view {
                    return;
                }

                // check if we have the entry
                // if we don't, insert
                let chan = if let Some(vq) = channel_map.replica_channel_map.get(&msg_view_number) {
                    vq.sender_chan.clone()
                } else {
                    let mut channel_map =
                        RwLockUpgradableReadGuard::<'_, SendToTasks<I, N>>::upgrade(channel_map)
                            .await;
                    let new_view_queue = ViewQueue::default();
                    let s_chan = new_view_queue.sender_chan.clone();
                    // NOTE: the read lock is held until all other read locks are DROPPED and
                    // the read lock may be turned into a write lock.
                    // This means that the `channel_map` will not change. So we don't need
                    // to check again to see if a channel was added

                    channel_map
                        .replica_channel_map
                        .insert(msg_view_number, new_view_queue);
                    s_chan
                };

                // sends the message if not stale
                if chan.send_async(msg).await.is_err() {
                    error!("Failed to replica task!");
                }
            }
            ConsensusMessage::NextViewInterrupt(_) => {
                warn!("Received a next view interrupt. This shouldn't be possible.");
            }
            ConsensusMessage::TimedOut(_) | ConsensusMessage::Vote(_) => {
                warn!("Received a broadcast for a vote or nextview message. This shouldn't be possible.");
            }
        };
    }

    /// Handle an incoming [`ConsensusMessage`] directed at this node.
    async fn handle_direct_consensus_message(
        &self,
        msg: <I as TypeMap<N>>::ConsensusMessage,
        _sender: I::SignatureKey,
    ) {
        // TODO validate incoming data message based on sender signature key
        // We can only recv from a replicas
        // replicas should only send votes or if they timed out, timeouts
        match msg {
            ConsensusMessage::Proposal(_) | ConsensusMessage::NextViewInterrupt(_) => {
                warn!("Received a direct message for a proposal. This shouldn't be possible.");
            }
            c @ (ConsensusMessage::Vote(_) | ConsensusMessage::TimedOut(_)) => {
                if self.send_next_leader.send_async(c).await.is_err() {
                    warn!("Failed to send to next leader!");
                }
            }
        }
    }

    /// Handle an incoming [`DataMessage`] that was broadcasted on the network
    async fn handle_broadcast_data_message(
        &self,
        msg: <I as TypeMap<N>>::DataMessage,
        _sender: I::SignatureKey,
    ) {
        // TODO validate incoming broadcast message based on sender signature key
        match msg {
            DataMessage::SubmitTransaction(transaction) => {
                // The API contract requires the hash to be unique
                // so we can assume entry == incoming txn
                // even if eq not satisfied
                // so insert is an idempotent operation
                self.transactions.write().await.push(transaction);
            }
            DataMessage::NewestQuorumCertificate { .. } => {
                // Log the exceptional situation and proceed
                warn!(?msg, "Direct message received over broadcast channel");
            }
        }
    }

    /// Handle an incoming [`DataMessage`] that directed at this node
    async fn handle_direct_data_message(
        &self,
        msg: <I as TypeMap<N>>::DataMessage,
        _sender: I::SignatureKey,
    ) {
        // TODO validate incoming data message based on sender signature key
        debug!(?msg, "Incoming direct data message");
        match msg {
            DataMessage::NewestQuorumCertificate {
                quorum_certificate: qc,
                block,
                state,
            } => {
                // TODO https://github.com/EspressoSystems/HotShot/issues/387
                let anchored = match self.inner.storage.get_anchored_view().await {
                    Err(e) => {
                        error!(?e, "Could not load QC");
                        return;
                    }
                    Ok(n) => n,
                };
                // TODO: Don't blindly accept the newest QC but make sure it's valid with other nodes too
                // we should be getting multiple data messages soon
                let should_save = anchored.view_number < qc.view_number; // incoming view is newer
                if should_save {
                    let view_number = qc.view_number;
                    let new_view = StoredView::from_qc_block_and_state(qc, block, state);

                    if let Err(e) = self.inner.storage.insert_single_view(new_view).await {
                        error!(?e, "Could not insert incoming QC");
                    }

                    // Broadcast that we're updated
                    self.send_event(Event {
                        view_number,
                        event: EventType::Synced { view_number },
                    })
                    .await;
                }
            }

            DataMessage::SubmitTransaction(_) => {
                // Log exceptional situation and proceed
                warn!(?msg, "Broadcast message received over direct channel");
            }
        }
    }

    /// Handle a change in the network
    async fn handle_network_change(&self, node: NetworkChange<I::SignatureKey>) {
        match node {
            NetworkChange::NodeConnected(peer) => {
                info!("Connected to node {:?}", peer);

                let anchor = match self.inner.storage.get_anchored_view().await {
                    Ok(anchor) => anchor,
                    Err(e) => {
                        error!(?e, "Could not retrieve newest QC");
                        return;
                    }
                };
                let msg = DataMessage::NewestQuorumCertificate {
                    quorum_certificate: anchor.qc,
                    block: anchor.append.into_deltas(),
                    state: anchor.state,
                };
                if let Err(e) = self.send_direct_message(msg, peer.clone()).await {
                    error!(
                        ?e,
                        "Could not send newest quorumcertificate to node {:?}", peer
                    );
                }
            }
            NetworkChange::NodeDisconnected(peer) => {
                info!("Lost connection to node {:?}", peer);
            }
        }
    }

    /// return the timeout for a view for `self`
    pub fn get_next_view_timeout(&self) -> u64 {
        self.inner.config.next_view_timeout
    }
}

/// A handle that is passed to [`hotshot_hotstuff`] with to expose the interface that hotstuff needs to interact with [`HotShot`]
#[derive(Clone)]
struct HotShotConsensusApi<I: NodeImplementation<N>, const N: usize> {
    /// Reference to the [`HotShotInner`]
    inner: Arc<HotShotInner<I, N>>,
}

#[async_trait]
impl<I: NodeImplementation<N>, const N: usize> hotshot_consensus::ConsensusApi<I, N>
    for HotShotConsensusApi<I, N>
{
    fn total_nodes(&self) -> NonZeroUsize {
        self.inner.config.total_nodes
    }

    fn threshold(&self) -> NonZeroUsize {
        self.inner.config.threshold
    }

    fn propose_min_round_time(&self) -> Duration {
        self.inner.config.propose_min_round_time
    }

    fn propose_max_round_time(&self) -> Duration {
        self.inner.config.propose_max_round_time
    }

    fn storage(&self) -> &I::Storage {
        &self.inner.storage
    }

    fn leader_acts_as_replica(&self) -> bool {
        true
    }

    async fn get_leader(&self, view_number: ViewNumber) -> I::SignatureKey {
        let election = &self.inner.election;
        election
            .election
            .get_leader(&election.stake_table, view_number)
    }

    async fn should_start_round(&self, _: ViewNumber) -> bool {
        false
    }

    async fn send_direct_message(
        &self,
        recipient: I::SignatureKey,
        message: <I as TypeMap<N>>::ConsensusMessage,
    ) -> std::result::Result<(), NetworkError> {
        debug!(?message, ?recipient, "send_direct_message");
        self.inner
            .networking
            .message_node(
                Message {
                    sender: self.inner.public_key.clone(),
                    kind: message.into(),
                },
                recipient,
            )
            .await
    }

    async fn send_broadcast_message(
        &self,
        message: <I as TypeMap<N>>::ConsensusMessage,
    ) -> std::result::Result<(), NetworkError> {
        debug!(?message, "send_broadcast_message");
        self.inner
            .networking
            .broadcast_message(Message {
                sender: self.inner.public_key.clone(),
                kind: message.into(),
            })
            .await
    }

    async fn send_event(&self, event: Event<I::Block, I::State, N>) {
        debug!(?event, "send_event");
        let mut event_sender = self.inner.event_sender.write().await;
        if let Some(sender) = &*event_sender {
            if let Err(e) = sender.send_async(event).await {
                error!(?e, "Could not send event to event_sender");
                *event_sender = None;
            }
        }
    }

    fn public_key(&self) -> &I::SignatureKey {
        &self.inner.public_key
    }

    fn private_key(&self) -> &<I::SignatureKey as SignatureKey>::PrivateKey {
        &self.inner.private_key
    }

    async fn notify(&self, blocks: Vec<I::Block>, states: Vec<I::State>) {
        debug!(?blocks, ?states, "notify");
        self.inner
            .stateful_handler
            .lock()
            .await
            .notify(blocks, states);
    }

    fn create_verify_hash(
        &self,
        leaf_hash: &LeafHash<N>,
        view_number: ViewNumber,
    ) -> VerifyHash<32> {
        create_verify_hash(leaf_hash, view_number)
    }

    #[instrument(skip(self))]
    fn validate_qc(&self, qc: &QuorumCertificate<N>, view_number: ViewNumber) -> bool {
        if qc.view_number != view_number {
            warn!(?qc, ?view_number, "Failing on view_number equality check");
        }
        if qc.genesis && qc.view_number == ViewNumber::genesis() {
            return true;
        }
        let hash = create_verify_hash(&qc.leaf_hash, view_number);
        let valid_signatures = self.get_valid_signatures(qc.signatures.clone(), hash);
        match valid_signatures {
            Ok(_) => true,
            Err(_e) => false,
        }
    }

    // s/get_valid_signatures/get_valid_signature(signature, hash)
    fn get_valid_signatures(
        &self,
        signatures: BTreeMap<EncodedPublicKey, EncodedSignature>,
        hash: VerifyHash<32>,
    ) -> Result<BTreeMap<EncodedPublicKey, EncodedSignature>> {
        let mut valid_signatures = BTreeMap::new();

        for (encoded_key, encoded_signature) in signatures {
            if let Some(key) = <I::SignatureKey as SignatureKey>::from_bytes(&encoded_key) {
                let contains = self.inner.cluster_public_keys.contains(&key);
                let valid = key.validate(&encoded_signature, hash.as_ref());
                if contains && valid {
                    valid_signatures.insert(encoded_key, encoded_signature);
                } else {
                    warn!(?contains, ?valid, "Signature failed validity check");
                }
            }
        }
        if valid_signatures.len() < self.inner.config.threshold.get() {
            return Err(HotShotError::InsufficientValidSignatures {
                num_valid_signatures: valid_signatures.len(),
                threshold: self.inner.config.threshold.get(),
            });
        }
        Ok(valid_signatures)
    }
}
