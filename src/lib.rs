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
pub mod data;
#[cfg(any(feature = "demo"))]
pub mod demos;
/// Contains traits consumed by [`HotShot`]
pub mod traits;
/// Contains types used by the crate
pub mod types;

mod tasks;

use crate::{
    data::{Leaf, QuorumCertificate},
    traits::{Block, NetworkingImplementation, NodeImplementation, Storage},
    types::{Event, HotShotHandle},
};
use async_lock::{Mutex, RwLock, RwLockUpgradableReadGuard, RwLockWriteGuard};
use async_trait::async_trait;
use bincode::Options;
use commit::{Commitment, Committable};
use hotshot_consensus::{
    Consensus, ConsensusApi, SendToTasks, TransactionHashMap, TransactionStorage, View, ViewInner,
    ViewQueue,
};
use hotshot_types::traits::election::VoteToken;
use hotshot_types::{
    constants::genesis_proposer_id,
    data::{fake_commitment, ViewNumber},
    error::StorageSnafu,
    message::{ConsensusMessage, DataMessage, Message},
    traits::{
        election::{
            Checked::{self, Inval, Unchecked, Valid},
            Election, ElectionError,
        },
        network::{NetworkChange, NetworkError},
        node_implementation::TypeMap,
        signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey},
        storage::{StoredView, ViewEntry},
        State,
    },
    HotShotConfig,
};
use hotshot_utils::{art::async_spawn, bincode::bincode_opts, broadcast::BroadcastSender};
use hotshot_utils::{
    art::async_spawn_local,
    channel::{unbounded, UnboundedReceiver, UnboundedSender},
};
use snafu::ResultExt;
use std::{
    collections::{BTreeMap, HashMap},
    num::{NonZeroU64, NonZeroUsize},
    sync::{atomic::Ordering, Arc},
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
/// Holds the state needed to participate in `HotShot` consensus
pub struct HotShotInner<I: NodeImplementation> {
    /// The public key of this node
    public_key: I::SignatureKey,

    /// The private key of this node
    private_key: <I::SignatureKey as SignatureKey>::PrivateKey,

    /// Configuration items for this hotshot instance
    config: HotShotConfig<
        I::SignatureKey,
        <I::Election as Election<I::SignatureKey, ViewNumber>>::ElectionConfigType,
    >,

    /// Networking interface for this hotshot instance
    networking: I::Networking,

    /// This `HotShot` instance's storage backend
    storage: I::Storage,

    /// This `HotShot` instance's election backend
    election: I::Election,

    /// Sender for [`Event`]s
    event_sender: RwLock<Option<BroadcastSender<Event<I::StateType>>>>,

    /// Senders to the background tasks.
    background_task_handle: tasks::TaskHandle,
}

/// Thread safe, shared view of a `HotShot`
#[derive(Clone)]
pub struct HotShot<I: NodeImplementation + Send + Sync + 'static> {
    /// Handle to internal hotshot implementation
    inner: Arc<HotShotInner<I>>,

    /// Transactions
    /// (this is shared btwn hotshot and `Consensus`)
    transactions: TransactionStorage<I>,

    /// The hotstuff implementation
    hotstuff: Arc<RwLock<Consensus<I>>>,

    /// for sending/recv-ing things with the replica task
    replica_channel_map: Arc<RwLock<SendToTasks<I>>>,

    /// for sending/recv-ing things with the next leader task
    next_leader_channel_map: Arc<RwLock<SendToTasks<I>>>,

    /// for sending messages to network lookup task
    send_network_lookup: UnboundedSender<Option<ViewNumber>>,

    /// for receiving messages in the network lookup task
    recv_network_lookup: Arc<Mutex<UnboundedReceiver<Option<ViewNumber>>>>,

    /// uid for instrumentation
    id: u64,
}

impl<I: NodeImplementation + Sync + Send + 'static> HotShot<I> {
    /// Creates a new hotshot with the given configuration options and sets it up with the given
    /// genesis block
    #[allow(clippy::too_many_arguments)]
    #[instrument(skip(private_key, networking, storage, election, initializer))]
    pub async fn new(
        public_key: I::SignatureKey,
        private_key: <I::SignatureKey as SignatureKey>::PrivateKey,
        nonce: u64,
        config: HotShotConfig<
            I::SignatureKey,
            <I::Election as Election<I::SignatureKey, ViewNumber>>::ElectionConfigType,
        >,
        networking: I::Networking,
        storage: I::Storage,
        election: I::Election,
        initializer: HotShotInitializer<I::StateType>,
    ) -> Result<Self> {
        info!("Creating a new hotshot");
        let inner: Arc<HotShotInner<I>> = Arc::new(HotShotInner {
            public_key,
            private_key,
            config,
            networking,
            storage,
            election,
            event_sender: RwLock::default(),
            background_task_handle: tasks::TaskHandle::default(),
        });

        let anchored_leaf = initializer.inner;

        // insert to storage
        inner
            .storage
            .append(vec![ViewEntry::Success(anchored_leaf.clone().into())])
            .await
            .context(StorageSnafu)?;

        // insert genesis (or latest block) to state map
        let mut state_map = BTreeMap::default();
        state_map.insert(
            anchored_leaf.view_number,
            View {
                view_inner: ViewInner::Leaf {
                    leaf: anchored_leaf.commit(),
                },
            },
        );

        let mut saved_leaves = HashMap::new();
        saved_leaves.insert(anchored_leaf.commit(), anchored_leaf.clone());

        let start_view = anchored_leaf.view_number;

        let hotstuff = Consensus {
            state_map,
            cur_view: start_view,
            last_decided_view: anchored_leaf.view_number,
            transactions: Arc::default(),
            saved_leaves,
            // TODO this is incorrect
            // https://github.com/EspressoSystems/HotShot/issues/560
            locked_view: anchored_leaf.view_number,
            high_qc: anchored_leaf.justify_qc,
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
        current_view: ViewNumber,
        send_replica: UnboundedSender<ConsensusMessage<I::StateType>>,
        send_next_leader: Option<UnboundedSender<ConsensusMessage<I::StateType>>>,
    ) {
        let msg = ConsensusMessage::<I::StateType>::NextViewInterrupt(current_view);
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
        transaction: <<<I as NodeImplementation>::StateType as State>::BlockType as Block>::Transaction,
    ) -> Result<()> {
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
    pub async fn get_state(&self) -> I::StateType {
        self.hotstuff.read().await.get_decided_leaf().state
    }

    /// Returns a copy of the last decided leaf
    /// # Panics
    /// Panics if internal state for consensus is inconsistent
    pub async fn get_decided_leaf(&self) -> Leaf<I::StateType> {
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
        public_key: I::SignatureKey,
        private_key: <I::SignatureKey as SignatureKey>::PrivateKey,
        node_id: u64,
        config: HotShotConfig<
            I::SignatureKey,
            <I::Election as Election<I::SignatureKey, ViewNumber>>::ElectionConfigType,
        >,
        networking: I::Networking,
        storage: I::Storage,
        election: I::Election,
        initializer: HotShotInitializer<I::StateType>,
    ) -> Result<HotShotHandle<I>> {
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
        kind: impl Into<<I as TypeMap>::MessageKind>,
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
        kind: impl Into<<I as TypeMap>::MessageKind>,
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
    #[instrument(
        skip(self),
        name = "Handle broadcast consensus message",
        level = "error"
    )]
    async fn handle_broadcast_consensus_message(
        &self,
        msg: <I as TypeMap>::ConsensusMessage,
        sender: I::SignatureKey,
    ) {
        // TODO validate incoming data message based on sender signature key
        // <github.com/ExpressoSystems/HotShot/issues/418>
        let msg_view_number = msg.view_number();

        // Skip messages that are not from the leader
        let api = HotShotConsensusApi {
            inner: self.inner.clone(),
        };
        if sender != api.get_leader(msg_view_number).await {
            return;
        }

        match msg {
            // this is ONLY intended for replica
            ConsensusMessage::Proposal(_) => {
                let channel_map = self.replica_channel_map.upgradable_read().await;

                // skip if the proposal is stale
                if msg_view_number < channel_map.cur_view {
                    warn!(
                        "Throwing away proposal for view number: {:?}",
                        msg_view_number
                    );
                    return;
                }

                let chan = create_or_obtain_chan_from_read(msg_view_number, channel_map).await;

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
        msg: <I as TypeMap>::ConsensusMessage,
        _sender: I::SignatureKey,
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
                let msg_view_number = c.view_number();

                let channel_map = self.next_leader_channel_map.upgradable_read().await;

                // check if
                // - is in fact, actually is the next leader
                // - the message is not stale
                let is_leader = ConsensusApi::is_leader(
                    &HotShotConsensusApi {
                        inner: self.inner.clone(),
                    },
                    msg_view_number + 1,
                )
                .await;
                if !is_leader || msg_view_number < channel_map.cur_view {
                    warn!(
                        "Throwing away Vote or TimedOut message for view number: {:?}",
                        msg_view_number
                    );
                    return;
                }

                let chan = create_or_obtain_chan_from_read(msg_view_number, channel_map).await;

                if chan.sender_chan.send(c).await.is_err() {
                    error!("Failed to send to next leader!");
                }
            }
        }
    }

    /// Handle an incoming [`DataMessage`] that was broadcasted on the network
    async fn handle_broadcast_data_message(
        &self,
        msg: <I as TypeMap>::DataMessage,
        _sender: I::SignatureKey,
    ) {
        // TODO validate incoming broadcast message based on sender signature key
        match msg {
            DataMessage::SubmitTransaction(transaction) => {
                // The API contract requires the hash to be unique
                // so we can assume entry == incoming txn
                // even if eq not satisfied
                // so insert is an idempotent operation
                let add_transaction = |txns: &mut TransactionHashMap<I>| {
                    txns.insert(transaction.commit(), transaction);
                };
                self.transactions.modify(add_transaction).await;
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
        msg: <I as TypeMap>::DataMessage,
        _sender: I::SignatureKey,
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
    /// In the future, catchup semantics can be handled here
    async fn handle_network_change(&self, node: NetworkChange<I::SignatureKey>) {
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
}

/// given a view number and a upgradable read lock on a channel map, inserts entry into map if it
/// doesn't exist, or creates entry. Then returns a clone of the entry
pub async fn create_or_obtain_chan_from_read<I: NodeImplementation>(
    view_num: ViewNumber,
    channel_map: RwLockUpgradableReadGuard<'_, SendToTasks<I>>,
) -> ViewQueue<I> {
    // check if we have the entry
    // if we don't, insert
    if let Some(vq) = channel_map.channel_map.get(&view_num) {
        vq.clone()
    } else {
        let mut channel_map =
            RwLockUpgradableReadGuard::<'_, SendToTasks<I>>::upgrade(channel_map).await;
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
pub async fn create_or_obtain_chan_from_write<I: NodeImplementation>(
    view_num: ViewNumber,
    mut channel_map: RwLockWriteGuard<'_, SendToTasks<I>>,
) -> ViewQueue<I> {
    channel_map.channel_map.entry(view_num).or_default().clone()
}

/// A handle that is passed to [`hotshot_hotstuff`] with to expose the interface that hotstuff needs to interact with [`HotShot`]
#[derive(Clone)]
struct HotShotConsensusApi<I: NodeImplementation> {
    /// Reference to the [`HotShotInner`]
    inner: Arc<HotShotInner<I>>,
}

#[async_trait]
impl<I: NodeImplementation> hotshot_consensus::ConsensusApi<I> for HotShotConsensusApi<I> {
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

    fn storage(&self) -> &I::Storage {
        &self.inner.storage
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
        view_number: ViewNumber,
        _next_state: Commitment<Leaf<I::StateType>>,
    ) -> std::result::Result<
        std::option::Option<
            <<I as NodeImplementation>::Election as Election<
                <I as NodeImplementation>::SignatureKey,
                ViewNumber,
            >>::VoteTokenType,
        >,
        ElectionError,
    > {
        self.inner
            .election
            .make_vote_token(view_number, &self.inner.private_key)
    }

    fn get_election(&self) -> &<I as NodeImplementation>::Election {
        &self.inner.election
    }

    async fn get_leader(&self, view_number: ViewNumber) -> I::SignatureKey {
        let election = &self.inner.election;
        election.get_leader(view_number)
    }

    async fn should_start_round(&self, _: ViewNumber) -> bool {
        false
    }

    async fn send_direct_message(
        &self,
        recipient: I::SignatureKey,
        message: <I as TypeMap>::ConsensusMessage,
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
        message: <I as TypeMap>::ConsensusMessage,
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

    async fn send_event(&self, event: Event<I::StateType>) {
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

    #[instrument(skip(self, qc))]
    fn validate_qc(&self, qc: &QuorumCertificate<I::StateType>) -> bool {
        if qc.genesis && qc.view_number == ViewNumber::genesis() {
            return true;
        }
        let hash = qc.leaf_commitment;
        let mut signature_map: BTreeMap<
            EncodedPublicKey,
            (
                EncodedSignature,
                <<I as NodeImplementation>::Election as Election<
                    <I as NodeImplementation>::SignatureKey,
                    ViewNumber,
                >>::VoteTokenType,
            ),
        > = BTreeMap::new();

        for signature in qc.signatures.clone() {
            let decoded_vote_token = bincode_opts().deserialize(&signature.1 .1).unwrap();
            signature_map.insert(signature.0, (signature.1 .0, decoded_vote_token));
        }

        let stake = signature_map
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
        hash: Commitment<Leaf<I::StateType>>,
        view_number: ViewNumber,
        vote_token: Checked<
            <<I as NodeImplementation>::Election as Election<
                <I as NodeImplementation>::SignatureKey,
                ViewNumber,
            >>::VoteTokenType,
        >,
    ) -> bool {
        let mut is_valid_vote_token = false;
        let mut is_valid_signature = false;
        if let Some(key) = <I::SignatureKey as SignatureKey>::from_bytes(encoded_key) {
            is_valid_signature = key.validate(encoded_signature, hash.as_ref());
            let valid_vote_token =
                self.get_election()
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
}

/// initializer struct for creating starting block
pub struct HotShotInitializer<STATE: State> {
    /// the leaf specified initialization
    inner: Leaf<STATE>,
}

impl<STATE: State<Time = ViewNumber>> HotShotInitializer<STATE> {
    /// initialize from genesis
    /// # Errors
    /// If we are unable to apply the genesis block to the default state
    pub fn from_genesis(genesis_block: <STATE as State>::BlockType) -> Result<Self> {
        let state = STATE::default()
            .append(&genesis_block, &ViewNumber::new(0))
            .map_err(|err| HotShotError::Misc {
                context: err.to_string(),
            })?;
        let view_number = ViewNumber::genesis();
        let justify_qc = QuorumCertificate::<STATE>::genesis();

        Ok(Self {
            inner: Leaf {
                view_number,
                justify_qc,
                parent_commitment: fake_commitment(),
                deltas: genesis_block,
                state,
                rejected: Vec::new(),
                timestamp: time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
                proposer_id: genesis_proposer_id(),
            },
        })
    }

    /// reload previous state based on most recent leaf
    pub fn from_reload(anchor_leaf: Leaf<STATE>) -> Self {
        Self { inner: anchor_leaf }
    }
}
