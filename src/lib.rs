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

/// Data availability support
// pub mod da;
/// Contains structures and functions for committee election
pub mod certificate;
#[cfg(any(feature = "demo"))]
pub mod demos;
/// Contains traits consumed by [`HotShot`]
pub mod traits;
/// Contains types used by the crate
pub mod types;

pub mod tasks;

use crate::{
    certificate::QuorumCertificate,
    traits::{NetworkingImplementation, NodeImplementation, Storage},
    types::{Event, HotShotHandle},
};
use async_compatibility_layer::{
    art::async_spawn,
    async_primitives::{broadcast::BroadcastSender, subscribable_rwlock::SubscribableRwLock},
};
use async_compatibility_layer::{
    art::async_spawn_local,
    channel::{unbounded, UnboundedReceiver, UnboundedSender},
};
use async_lock::{Mutex, RwLock, RwLockUpgradableReadGuard, RwLockWriteGuard};
use async_trait::async_trait;
use bincode::Options;
use commit::{Commitment, Committable};
use hotshot_consensus::{
    Consensus, ConsensusApi, ConsensusMetrics, SendToTasks, View, ViewInner, ViewQueue,
};
use hotshot_types::{
    data::LeafType,
    error::StorageSnafu,
    message::{ConsensusMessage, DataMessage, Message},
    traits::{
        election::{
            Checked::{self, Inval, Unchecked, Valid},
            Election, ElectionError,
        },
        metrics::Metrics,
        network::{NetworkChange, NetworkError},
        node_implementation::NodeType,
        signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey},
        state::ConsensusTime,
        storage::StoredView,
        State,
    },
    HotShotConfig,
};
use hotshot_types::{message::MessageKind, traits::election::VoteToken};
use hotshot_utils::bincode::bincode_opts;
use snafu::ResultExt;
use std::{
    collections::{BTreeMap, HashMap},
    num::{NonZeroU64, NonZeroUsize},
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tasks::ViewRunner;
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

/// Holds the state needed to participate in `HotShot` consensus
pub struct HotShotInner<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// The public key of this node
    public_key: TYPES::SignatureKey,

    /// The private key of this node
    private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,

    /// Configuration items for this hotshot instance
    config: HotShotConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,

    /// Networking interface for this hotshot instance
    networking: I::Networking,

    /// This `HotShot` instance's storage backend
    storage: I::Storage,

    /// This `HotShot` instance's election backend
    election: I::Election,

    /// Sender for [`Event`]s
    event_sender: RwLock<Option<BroadcastSender<Event<TYPES, I::Leaf>>>>,

    /// Senders to the background tasks.
    background_task_handle: tasks::TaskHandle<TYPES>,

    /// a reference to the metrics that the implementor is using.
    metrics: Box<dyn Metrics>,
}

/// Thread safe, shared view of a `HotShot`
#[derive(Clone)]
pub struct HotShot<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Handle to internal hotshot implementation
    inner: Arc<HotShotInner<TYPES, I>>,

    /// Transactions
    /// (this is shared btwn hotshot and `Consensus`)
    transactions:
        Arc<SubscribableRwLock<HashMap<Commitment<TYPES::Transaction>, TYPES::Transaction>>>,

    /// The hotstuff implementation
    hotstuff: Arc<RwLock<Consensus<TYPES, I::Leaf>>>,

    /// for sending/recv-ing things with the replica task
    replica_channel_map: Arc<RwLock<SendToTasks<TYPES, I::Leaf, I::Proposal>>>,

    /// for sending/recv-ing things with the next leader task
    next_leader_channel_map: Arc<RwLock<SendToTasks<TYPES, I::Leaf, I::Proposal>>>,

    /// for sending messages to network lookup task
    send_network_lookup: UnboundedSender<Option<TYPES::Time>>,

    /// for receiving messages in the network lookup task
    recv_network_lookup: Arc<Mutex<UnboundedReceiver<Option<TYPES::Time>>>>,

    /// uid for instrumentation
    id: u64,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> HotShot<TYPES, I>
where
    ViewRunner<<TYPES as NodeType>::ConsensusType>: tasks::ViewRunnerType<TYPES, I>,
{
    /// Creates a new hotshot with the given configuration options and sets it up with the given
    /// genesis block
    #[allow(clippy::too_many_arguments)]
    #[instrument(skip(private_key, networking, storage, election, initializer, metrics))]
    pub async fn new(
        public_key: TYPES::SignatureKey,
        private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
        nonce: u64,
        config: HotShotConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
        networking: I::Networking,
        storage: I::Storage,
        election: I::Election,
        initializer: HotShotInitializer<TYPES, I::Leaf>,
        metrics: Box<dyn Metrics>,
    ) -> Result<Self, HotShotError<TYPES>> {
        info!("Creating a new hotshot");
        let inner: Arc<HotShotInner<TYPES, I>> = Arc::new(HotShotInner {
            public_key,
            private_key,
            config,
            networking,
            storage,
            election,
            event_sender: RwLock::default(),
            background_task_handle: tasks::TaskHandle::default(),
            metrics,
        });

        let anchored_leaf = initializer.inner;

        // insert to storage
        inner
            .storage
            .append(vec![anchored_leaf.clone().into()])
            .await
            .context(StorageSnafu)?;

        // insert genesis (or latest block) to state map
        let mut state_map = BTreeMap::default();
        state_map.insert(
            anchored_leaf.get_view_number(),
            View {
                view_inner: ViewInner::Leaf {
                    leaf: anchored_leaf.commit(),
                },
            },
        );

        let mut saved_leaves = HashMap::new();
        saved_leaves.insert(anchored_leaf.commit(), anchored_leaf.clone());

        let start_view = anchored_leaf.get_view_number();

        let hotstuff = Consensus {
            state_map,
            cur_view: start_view,
            last_decided_view: anchored_leaf.get_view_number(),
            transactions: Arc::default(),
            saved_leaves,
            // TODO this is incorrect
            // https://github.com/EspressoSystems/HotShot/issues/560
            locked_view: anchored_leaf.get_view_number(),
            high_qc: anchored_leaf.get_justify_qc(),

            metrics: Arc::new(ConsensusMetrics::new(
                inner.metrics.subgroup("consensus".to_string()),
            )),
            invalid_qc: 0,
        };
        let hotstuff = Arc::new(RwLock::new(hotstuff));
        let txns = hotstuff.read().await.get_transactions();

        let (send_network_lookup, recv_network_lookup) = unbounded();

        Ok(Self {
            id: nonce,
            inner,
            transactions: txns,
            hotstuff,
            replica_channel_map: Arc::new(RwLock::new(SendToTasks::new(start_view))),
            next_leader_channel_map: Arc::new(RwLock::new(SendToTasks::new(start_view))),
            send_network_lookup,
            recv_network_lookup: Arc::new(Mutex::new(recv_network_lookup)),
        })
    }

    /// Marks a given view number as timed out. This should be called a fixed period after a round is started.
    ///
    /// If the round has already ended then this function will essentially be a no-op. Otherwise `run_round` will return shortly after this function is called.
    /// # Panics
    /// Panics if the current view is not in the channel map
    #[instrument(
        skip_all,
        fields(id = self.id, view = *current_view),
        name = "Timeout consensus tasks",
        level = "warn"
    )]
    pub async fn timeout_view(
        &self,
        current_view: TYPES::Time,
        send_replica: UnboundedSender<ConsensusMessage<TYPES, I::Leaf, I::Proposal>>,
        send_next_leader: Option<UnboundedSender<ConsensusMessage<TYPES, I::Leaf, I::Proposal>>>,
    ) {
        let msg = ConsensusMessage::<TYPES, I::Leaf, I::Proposal>::NextViewInterrupt(current_view);
        if let Some(chan) = send_next_leader {
            if chan.send(msg.clone()).await.is_err() {
                warn!("Error timing out next leader task");
            }
        };
        // NOTE this should always exist
        if send_replica.send(msg).await.is_err() {
            warn!("Error timing out replica task");
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
        transaction: TYPES::Transaction,
    ) -> Result<(), HotShotError<TYPES>> {
        // Add the transaction to our own queue first
        trace!("Adding transaction to our own queue");
        // Wrap up a message
        let message = DataMessage::SubmitTransaction(transaction);

        let api = self.clone();
        async_spawn(async move {
            let _result = api.send_broadcast_message(message).await.is_err();
        });
        Ok(())
    }

    /// Returns a copy of the state
    ///
    /// # Panics
    ///
    /// Panics if internal state for consensus is inconsistent
    pub async fn get_state(&self) -> <I::Leaf as LeafType>::StateCommitmentType {
        self.hotstuff.read().await.get_decided_leaf().get_state()
    }

    /// Returns a copy of the last decided leaf
    /// # Panics
    /// Panics if internal state for consensus is inconsistent
    pub async fn get_decided_leaf(&self) -> I::Leaf {
        self.hotstuff.read().await.get_decided_leaf()
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
        public_key: TYPES::SignatureKey,
        private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
        node_id: u64,
        config: HotShotConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
        networking: I::Networking,
        storage: I::Storage,
        election: I::Election,
        initializer: HotShotInitializer<TYPES, I::Leaf>,
        metrics: Box<dyn Metrics>,
    ) -> Result<HotShotHandle<TYPES, I>, HotShotError<TYPES>> {
        // Save a clone of the storage for the handle
        let hotshot = Self::new(
            public_key,
            private_key,
            node_id,
            config,
            networking,
            storage,
            election,
            initializer,
            metrics,
        )
        .await?;
        let handle = tasks::spawn_all(&hotshot).await;

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
        kind: impl Into<MessageKind<TYPES, I::Leaf, I::Proposal>>,
    ) -> std::result::Result<(), NetworkError> {
        let inner = self.inner.clone();
        let pk = self.inner.public_key.clone();
        let kind = kind.into();
        async_spawn_local(async move {
            if inner
                .networking
                .broadcast_message(Message { sender: pk, kind })
                .await
                .is_err()
            {
                warn!("Failed to broadcast message");
            };
        });
        Ok(())
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
        kind: impl Into<MessageKind<TYPES, I::Leaf, I::Proposal>>,
        recipient: TYPES::SignatureKey,
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
    #[instrument(
        skip(self),
        name = "Handle broadcast consensus message",
        level = "error"
    )]
    async fn handle_broadcast_consensus_message(
        &self,
        msg: ConsensusMessage<TYPES, I::Leaf, I::Proposal>,
        sender: TYPES::SignatureKey,
    ) {
        // TODO validate incoming data message based on sender signature key
        // <github.com/ExpressoSystems/HotShot/issues/418>
        let msg_time = msg.view_number();

        // Skip messages that are not from the leader
        let api = HotShotConsensusApi {
            inner: self.inner.clone(),
        };
        if sender != api.get_leader(msg_time).await {
            return;
        }

        match msg {
            // this is ONLY intended for replica
            ConsensusMessage::Proposal(_) => {
                let channel_map = self.replica_channel_map.upgradable_read().await;

                // skip if the proposal is stale
                if msg_time < channel_map.cur_view {
                    warn!("Throwing away proposal for view number: {:?}", msg_time);
                    return;
                }

                let chan: ViewQueue<TYPES, I::Leaf, I::Proposal> =
                    Self::create_or_obtain_chan_from_read(msg_time, channel_map).await;

                if !chan.has_received_proposal.swap(true, Ordering::Relaxed)
                    && chan.sender_chan.send(msg).await.is_err()
                {
                    warn!("Failed to send to next leader!");
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
    #[instrument(
        skip(self, _sender),
        name = "Handle direct consensus message",
        level = "error"
    )]
    async fn handle_direct_consensus_message(
        &self,
        msg: ConsensusMessage<TYPES, I::Leaf, I::Proposal>,
        _sender: TYPES::SignatureKey,
    ) {
        // TODO validate incoming data message based on sender signature key

        // We can only recv from a replicas
        // replicas should only send votes or if they timed out, timeouts
        match msg {
            ConsensusMessage::Proposal(_) | ConsensusMessage::NextViewInterrupt(_) => {
                warn!("Received a direct message for a proposal. This shouldn't be possible.");
            }
            // this is ONLY intended for next leader
            c @ (ConsensusMessage::Vote(_) | ConsensusMessage::TimedOut(_)) => {
                let msg_time = c.view_number();

                let channel_map = self.next_leader_channel_map.upgradable_read().await;

                // check if
                // - is in fact, actually is the next leader
                // - the message is not stale
                let is_leader = ConsensusApi::is_leader(
                    &HotShotConsensusApi {
                        inner: self.inner.clone(),
                    },
                    msg_time + 1,
                )
                .await;
                if !is_leader || msg_time < channel_map.cur_view {
                    warn!(
                        "Throwing away Vote or TimedOut message for view number: {:?}",
                        msg_time
                    );
                    return;
                }

                let chan = Self::create_or_obtain_chan_from_read(msg_time, channel_map).await;

                if chan.sender_chan.send(c).await.is_err() {
                    error!("Failed to send to next leader!");
                }
            }
        }
    }

    /// Handle an incoming [`DataMessage`] that was broadcasted on the network
    async fn handle_broadcast_data_message(
        &self,
        msg: DataMessage<TYPES, I::Leaf>,
        _sender: TYPES::SignatureKey,
    ) {
        // TODO validate incoming broadcast message based on sender signature key
        match msg {
            DataMessage::SubmitTransaction(transaction) => {
                let size = bincode_opts().serialized_size(&transaction).unwrap_or(0);

                // The API contract requires the hash to be unique
                // so we can assume entry == incoming txn
                // even if eq not satisfied
                // so insert is an idempotent operation
                let mut new = false;
                self.transactions
                    .modify(|txns| {
                        new = txns.insert(transaction.commit(), transaction).is_none();
                    })
                    .await;

                if new {
                    // If this is a new transaction, update metrics.
                    let consensus = self.hotstuff.read().await;
                    consensus.metrics.outstanding_transactions.update(1);
                    #[allow(clippy::cast_possible_wrap)]
                    consensus
                        .metrics
                        .outstanding_transactions_memory_size
                        .update(size as i64);
                }
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
        msg: DataMessage<TYPES, I::Leaf>,
        _sender: TYPES::SignatureKey,
    ) {
        // TODO validate incoming data message based on sender signature key
        debug!(?msg, "Incoming direct data message");
        match msg {
            DataMessage::NewestQuorumCertificate {
                quorum_certificate: qc,
                block,
                state,
                parent_commitment,
                rejected,
                proposer_id,
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
                // <https://github.com/EspressoSystems/HotShot/issues/454>
                let should_save = anchored.view_number < qc.view_number; // incoming view is newer
                if should_save {
                    let new_view = StoredView::from_qc_block_and_state(
                        qc,
                        block,
                        state,
                        // We don't have enough information in this message to validate the height
                        // of the new QC. We would need the full parent leaf, so we can check that
                        // its commitment matches `qc.leaf_commitment` and extract the height from
                        // the leaf. But this message is no longer used and the whole catchup
                        // protocol needs to be redesigned.
                        0,
                        parent_commitment,
                        rejected,
                        proposer_id,
                    );

                    if let Err(e) = self.inner.storage.append_single_view(new_view).await {
                        error!(?e, "Could not insert incoming QC");
                    }
                }
            }

            DataMessage::SubmitTransaction(_) => {
                // Log exceptional situation and proceed
                warn!(?msg, "Broadcast message received over direct channel");
            }
        }
    }

    /// Handle a change in the network
    async fn handle_network_change(&self, node: NetworkChange<TYPES::SignatureKey>) {
        match node {
            NetworkChange::NodeConnected(peer) => {
                info!("Connected to node {:?}", peer);
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

    /// given a view number and a upgradable read lock on a channel map, inserts entry into map if it
    /// doesn't exist, or creates entry. Then returns a clone of the entry
    pub async fn create_or_obtain_chan_from_read(
        view_num: TYPES::Time,
        channel_map: RwLockUpgradableReadGuard<'_, SendToTasks<TYPES, I::Leaf, I::Proposal>>,
    ) -> ViewQueue<TYPES, I::Leaf, I::Proposal> {
        // check if we have the entry
        // if we don't, insert
        if let Some(vq) = channel_map.channel_map.get(&view_num) {
            vq.clone()
        } else {
            let mut channel_map = RwLockUpgradableReadGuard::<
                '_,
                SendToTasks<TYPES, I::Leaf, I::Proposal>,
            >::upgrade(channel_map)
            .await;
            let new_view_queue = ViewQueue::default();
            let vq = new_view_queue.clone();
            // NOTE: the read lock is held until all other read locks are DROPPED and
            // the read lock may be turned into a write lock.
            // This means that the `channel_map` will not change. So we don't need
            // to check again to see if a channel was added

            channel_map.channel_map.insert(view_num, new_view_queue);
            vq
        }
    }

    /// given a view number and a write lock on a channel map, inserts entry into map if it
    /// doesn't exist, or creates entry. Then returns a clone of the entry
    pub async fn create_or_obtain_chan_from_write(
        view_num: TYPES::Time,
        mut channel_map: RwLockWriteGuard<'_, SendToTasks<TYPES, I::Leaf, I::Proposal>>,
    ) -> ViewQueue<TYPES, I::Leaf, I::Proposal> {
        channel_map.channel_map.entry(view_num).or_default().clone()
    }
}

/// A handle that is passed to [`hotshot_hotstuff`] with to expose the interface that hotstuff needs to interact with [`HotShot`]
#[derive(Clone)]
struct HotShotConsensusApi<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Reference to the [`HotShotInner`]
    inner: Arc<HotShotInner<TYPES, I>>,
}

#[async_trait]
impl<TYPES: NodeType, I: NodeImplementation<TYPES>>
    hotshot_consensus::ConsensusApi<TYPES, I::Leaf, I::Proposal> for HotShotConsensusApi<TYPES, I>
{
    fn total_nodes(&self) -> NonZeroUsize {
        self.inner.config.total_nodes
    }

    fn threshold(&self) -> NonZeroU64 {
        self.inner.election.get_threshold()
    }

    fn propose_min_round_time(&self) -> Duration {
        self.inner.config.propose_min_round_time
    }

    fn propose_max_round_time(&self) -> Duration {
        self.inner.config.propose_max_round_time
    }

    fn max_transactions(&self) -> NonZeroUsize {
        self.inner.config.max_transactions
    }

    fn min_transactions(&self) -> usize {
        self.inner.config.min_transactions
    }

    /// Generates and encodes a vote token
    fn generate_vote_token(
        &self,
        view_number: TYPES::Time,
        _next_state: Commitment<I::Leaf>,
    ) -> std::result::Result<std::option::Option<TYPES::VoteTokenType>, ElectionError> {
        self.inner
            .election
            .make_vote_token(view_number, &self.inner.private_key)
    }

    async fn get_leader(&self, view_number: TYPES::Time) -> TYPES::SignatureKey {
        let election = &self.inner.election;
        election.get_leader(view_number)
    }

    async fn should_start_round(&self, _: TYPES::Time) -> bool {
        false
    }

    async fn send_direct_message(
        &self,
        recipient: TYPES::SignatureKey,
        message: ConsensusMessage<TYPES, I::Leaf, I::Proposal>,
    ) -> std::result::Result<(), NetworkError> {
        let inner = self.inner.clone();
        debug!(?message, ?recipient, "send_direct_message");
        async_spawn_local(async move {
            inner
                .networking
                .message_node(
                    Message {
                        sender: inner.public_key.clone(),
                        kind: message.into(),
                    },
                    recipient,
                )
                .await
        });
        Ok(())
    }

    async fn send_broadcast_message(
        &self,
        message: ConsensusMessage<TYPES, I::Leaf, I::Proposal>,
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

    async fn send_event(&self, event: Event<TYPES, I::Leaf>) {
        debug!(?event, "send_event");
        let mut event_sender = self.inner.event_sender.write().await;
        if let Some(sender) = &*event_sender {
            if let Err(e) = sender.send_async(event).await {
                error!(?e, "Could not send event to event_sender");
                *event_sender = None;
            }
        }
    }

    fn public_key(&self) -> &TYPES::SignatureKey {
        &self.inner.public_key
    }

    fn private_key(&self) -> &<TYPES::SignatureKey as SignatureKey>::PrivateKey {
        &self.inner.private_key
    }

    #[instrument(skip(self, qc))]
    fn validate_qc(&self, qc: &QuorumCertificate<TYPES, I::Leaf>) -> bool {
        if qc.genesis && qc.view_number == TYPES::Time::genesis() {
            return true;
        }
        let hash = qc.leaf_commitment;

        let stake = qc
            .signatures
            .iter()
            .filter(|signature| {
                self.is_valid_signature(
                    signature.0,
                    &signature.1 .0,
                    hash,
                    qc.view_number,
                    Unchecked(signature.1 .1.clone()),
                )
            })
            .fold(0, |acc, x| (acc + u64::from(x.1 .1.vote_count())));

        if stake >= u64::from(self.threshold()) {
            return true;
        }
        false
    }

    fn is_valid_signature(
        &self,
        encoded_key: &EncodedPublicKey,
        encoded_signature: &EncodedSignature,
        hash: Commitment<I::Leaf>,
        view_number: TYPES::Time,
        vote_token: Checked<TYPES::VoteTokenType>,
    ) -> bool {
        let mut is_valid_vote_token = false;
        let mut is_valid_signature = false;
        if let Some(key) = <TYPES::SignatureKey as SignatureKey>::from_bytes(encoded_key) {
            is_valid_signature = key.validate(encoded_signature, hash.as_ref());
            let valid_vote_token =
                self.inner
                    .election
                    .validate_vote_token(view_number, key, vote_token);
            is_valid_vote_token = match valid_vote_token {
                Err(_) => {
                    error!("Vote token was invalid");
                    false
                }
                Ok(Valid(_)) => true,
                Ok(Inval(_) | Unchecked(_)) => false,
            };
        }
        is_valid_signature && is_valid_vote_token
    }

    async fn store_leaf(
        &self,
        old_anchor_view: TYPES::Time,
        leaf: I::Leaf,
    ) -> std::result::Result<(), hotshot_types::traits::storage::StorageError> {
        let view_to_insert = StoredView::from(leaf);
        let storage = &self.inner.storage;
        storage.append_single_view(view_to_insert).await?;
        storage.cleanup_storage_up_to_view(old_anchor_view).await?;
        storage.commit().await?;
        Ok(())
    }
}

/// initializer struct for creating starting block
pub struct HotShotInitializer<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> {
    /// the leaf specified initialization
    inner: LEAF,
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> HotShotInitializer<TYPES, LEAF> {
    /// initialize from genesis
    /// # Errors
    /// If we are unable to apply the genesis block to the default state
    pub fn from_genesis(genesis_block: TYPES::BlockType) -> Result<Self, HotShotError<TYPES>> {
        let state = TYPES::StateType::default()
            .append(&genesis_block, &TYPES::Time::new(0))
            .map_err(|err| HotShotError::Misc {
                context: err.to_string(),
            })?;
        let time = TYPES::Time::genesis();
        let justify_qc = QuorumCertificate::<TYPES, LEAF>::genesis();

        Ok(Self {
            inner: LEAF::new(time, justify_qc, genesis_block, state),
        })
    }

    /// reload previous state based on most recent leaf
    pub fn from_reload(anchor_leaf: LEAF) -> Self {
        Self { inner: anchor_leaf }
    }
}
