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
    data::{Leaf, LeafHash, QuorumCertificate, Stage},
    traits::{BlockContents, NetworkingImplementation, NodeImplementation, Storage},
    types::{Event, EventType, HotShotHandle},
};

use async_std::sync::{Mutex, RwLock};
use async_trait::async_trait;
use futures::channel::oneshot;
use hotshot_consensus::{Consensus, RoundFinishedEventState};
use hotshot_types::{
    data::{create_verify_hash, VerifyHash, ViewNumber},
    error::{NetworkFaultSnafu, StorageSnafu},
    message::{DataMessage, Message},
    traits::{
        election::Election,
        network::{NetworkChange, NetworkError},
        node_implementation::TypeMap,
        signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey},
        stateful_handler::StatefulHandler,
    },
};
use hotshot_utils::broadcast::BroadcastSender;
use snafu::ResultExt;
use std::{
    collections::{BTreeMap, HashSet},
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
pub use tasks::RoundRunnerState;

/// Length, in bytes, of a 512 bit hash
pub const H_512: usize = 64;
/// Length, in bytes, of a 256 bit hash
pub const H_256: usize = 32;

/// Convenience type alias
type Result<T> = std::result::Result<T, HotShotError>;

/// Holds configuration for a `HotShot`
#[derive(Debug, Clone)]
pub struct HotShotConfig<P: SignatureKey> {
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

    /// The hotstuff implementation
    hotstuff: Mutex<Consensus<I, N>>,
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
}

impl<I: NodeImplementation<N> + Sync + Send + 'static, const N: usize> HotShot<I, N> {
    /// Creates a new hotshot with the given configuration options and sets it up with the given
    /// genesis block
    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    #[instrument(skip(
        genesis,
        private_key,
        cluster_public_keys,
        starting_state,
        networking,
        storage,
        election
    ))]
    pub async fn new(
        genesis: I::Block,
        cluster_public_keys: impl IntoIterator<Item = I::SignatureKey>,
        public_key: I::SignatureKey,
        private_key: <I::SignatureKey as SignatureKey>::PrivateKey,
        nonce: u64,
        config: HotShotConfig<I::SignatureKey>,
        starting_state: I::State,
        networking: I::Networking,
        storage: I::Storage,
        handler: I::StatefulHandler,
        election: I::Election,
    ) -> Result<Self> {
        info!("Creating a new hotshot");
        let genesis_hash = BlockContents::hash(&genesis);
        let leaf = Leaf {
            parent: [0_u8; { N }].into(),
            item: genesis.clone(),
        };
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
            hotstuff: Mutex::default(),
            cluster_public_keys: cluster_public_keys.into_iter().collect(),
        };
        let leaf_hash = leaf.hash();
        trace!("Genesis leaf hash: {:?}", leaf_hash);

        inner
            .storage
            .update(|mut m| async move {
                m.insert_qc(QuorumCertificate {
                    block_hash: genesis_hash,
                    leaf_hash,
                    view_number: ViewNumber::genesis(),
                    stage: Stage::Decide,
                    signatures: BTreeMap::new(),
                    genesis: true,
                })
                .await?;
                m.insert_leaf(Leaf {
                    parent: [0_u8; { N }].into(),
                    item: genesis,
                })
                .await?;
                m.insert_state(starting_state, leaf_hash).await?;
                Ok(())
            })
            .await
            .context(StorageSnafu)?;

        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Returns true if the proposed leaf extends from the given block
    #[instrument(skip(self))]
    pub async fn extends_from(&self, leaf: &Leaf<I::Block, N>, node: &LeafHash<N>) -> bool {
        let mut parent = leaf.parent;
        // Short circuit to enable blocks that don't have parents
        if &parent == node {
            trace!("leaf extends from node through short-circuit");
            return true;
        }
        while parent != LeafHash::from_array([0_u8; { N }]) {
            if &parent == node {
                trace!(?parent, "Leaf extends from");
                return true;
            }
            let next_parent = self.inner.storage.get_leaf(&parent).await;
            if let Ok(Some(next_parent)) = next_parent {
                parent = next_parent.parent;
            } else {
                error!("Leaf does not extend from node");
                return false;
            }
        }
        trace!("Leaf extends from node by default");
        true
    }

    /// Sends out the next view message
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The phase already exists
    /// - INTERNAL: Phases are not properly sorted
    /// - The storage layer returned an error
    /// - There were no QCs in the storage
    /// - A broadcast message could not be send
    #[instrument(skip(self), err)]
    pub async fn next_view(&self, current_view: ViewNumber) -> Result<()> {
        let mut hotstuff = self.inner.hotstuff.lock().await;
        hotstuff
            .next_view(
                current_view,
                &mut HotShotConsensusApi { inner: &self.inner },
            )
            .await
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

    /// Runs a single round of consensus
    ///
    /// Returns the view number of the round that was completed.
    ///
    /// # Panics
    ///
    /// Panics if consensus hits a bad round
    #[instrument(skip(self), err)]
    pub async fn run_round(&self, current_view: ViewNumber) -> Result<ViewNumber> {
        let (sender, receiver) = oneshot::channel();
        self.inner
            .hotstuff
            .lock()
            .await
            .register_round_finished_listener(current_view, sender);
        let result = match receiver.await {
            Ok(result) => result,
            Err(e) => {
                return Err(HotShotError::InvalidState {
                    context: format!("Could not wait for round to end: {:?}", e),
                })
            }
        };
        match result.state {
            RoundFinishedEventState::Success => Ok(result.view_number),
            RoundFinishedEventState::Interrupted(state) => Err(HotShotError::ViewTimeoutError {
                view_number: result.view_number,
                state,
            }),
            x => Err(HotShotError::InvalidState {
                context: format!("Round finished in an unknown state: {:?}", x),
            }),
        }
    }

    /// Marks a given view number as timed out. This should be called a fixed period after a round is started.
    ///
    /// If the round has already ended then this function will essentially be a no-op. Otherwise `run_round` will return shortly after this function is called.
    pub async fn mark_round_as_timed_out(&self, current_view: ViewNumber) {
        self.inner.hotstuff.lock().await.round_timeout(current_view);
    }

    /// Publishes a transaction to the network
    ///
    /// # Errors
    ///
    /// Will generate an error if an underlying network error occurs
    #[instrument(skip(self), err)]
    pub async fn publish_transaction_async(
        &self,
        tx: <<I as NodeImplementation<N>>::Block as BlockContents<N>>::Transaction,
    ) -> Result<()> {
        // Add the transaction to our own queue first
        trace!("Adding transaction to our own queue");
        self.inner
            .hotstuff
            .lock()
            .await
            .add_transaction(tx.clone(), &mut HotShotConsensusApi { inner: &self.inner })
            .await?;
        // Wrap up a message
        let message = DataMessage::SubmitTransaction(tx);
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
    /// # Errors
    ///
    /// Returns an error if an error occured with the storage interface, or if there is no QC or valid State in the storage.
    pub async fn get_state(&self) -> Result<Option<I::State>> {
        let qc = match self
            .inner
            .storage
            .get_newest_qc()
            .await
            .context(StorageSnafu)?
        {
            Some(qc) => qc,
            None => return Ok(None),
        };
        match self
            .inner
            .storage
            .get_state(&qc.leaf_hash)
            .await
            .context(StorageSnafu)?
        {
            Some(state) => Ok(Some(state)),
            None => Ok(None),
        }
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
        genesis: I::Block,
        cluster_public_keys: impl IntoIterator<Item = I::SignatureKey>,
        public_key: I::SignatureKey,
        private_key: <I::SignatureKey as SignatureKey>::PrivateKey,
        node_id: u64,
        config: HotShotConfig<I::SignatureKey>,
        starting_state: I::State,
        networking: I::Networking,
        storage: I::Storage,
        handler: I::StatefulHandler,
        election: I::Election,
    ) -> Result<HotShotHandle<I, N>> {
        // Save a clone of the storage for the handle
        let hotshot = Self::new(
            genesis,
            cluster_public_keys,
            public_key,
            private_key,
            node_id,
            config,
            starting_state,
            networking,
            storage,
            handler,
            election,
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
        sender: I::SignatureKey,
    ) {
        let mut hotstuff = self.inner.hotstuff.lock().await;
        let hotstuff = &mut *hotstuff;
        if let Err(e) = hotstuff
            .add_consensus_message(
                msg.clone(),
                &mut HotShotConsensusApi { inner: &self.inner },
                sender,
            )
            .await
        {
            error!(?e, ?msg, ?hotstuff, "Could not execute hotstuff");
        }
    }

    /// Handle an incoming [`ConsensusMessage`] directed at this node.
    async fn handle_direct_consensus_message(
        &self,
        msg: <I as TypeMap<N>>::ConsensusMessage,
        sender: I::SignatureKey,
    ) {
        let mut hotstuff = self.inner.hotstuff.lock().await;
        let hotstuff = &mut *hotstuff;
        if let Err(e) = hotstuff
            .add_consensus_message(
                msg.clone(),
                &mut HotShotConsensusApi { inner: &self.inner },
                sender,
            )
            .await
        {
            warn!(?e, ?msg, ?hotstuff, "Could not handle direct message");
        }
    }

    /// Handle an incoming [`DataMessage`] that was broadcasted on the network
    async fn handle_broadcast_data_message(
        &self,
        msg: <I as TypeMap<N>>::DataMessage,
        _sender: I::SignatureKey,
    ) {
        match msg {
            DataMessage::SubmitTransaction(transaction) => {
                let mut hotstuff = self.inner.hotstuff.lock().await;
                let hotstuff = &mut *hotstuff;
                if let Err(e) = hotstuff
                    .add_transaction(
                        transaction.clone(),
                        &mut HotShotConsensusApi { inner: &self.inner },
                    )
                    .await
                {
                    error!(?e, ?transaction, ?hotstuff, "Could not add transaction");
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
        msg: <I as TypeMap<N>>::DataMessage,
        _sender: I::SignatureKey,
    ) {
        debug!(?msg, "Incoming direct data message");
        match msg {
            DataMessage::NewestQuorumCertificate {
                quorum_certificate: qc,
                block,
                state,
            } => {
                let own_newest = match self.inner.storage.get_newest_qc().await {
                    Err(e) => {
                        error!(?e, "Could not load QC");
                        return;
                    }
                    Ok(n) => n,
                };
                // TODO: Don't blindly accept the newest QC but make sure it's valid with other nodes too
                // we should be getting multiple data messages soon
                let should_save = if let Some(own) = own_newest {
                    own.view_number < qc.view_number // incoming view is newer
                } else {
                    true // we have no QC yet
                };
                if should_save {
                    let new_view_number = qc.view_number;
                    let block_hash = BlockContents::hash(&block);
                    let leaf_hash = qc.leaf_hash;
                    let leaf = Leaf::new(block.clone(), leaf_hash);
                    debug!(?leaf, ?block, ?state, ?qc, "Saving");

                    if let Err(e) = self
                        .inner
                        .storage
                        .update(|mut m| async move {
                            m.insert_leaf(leaf).await?;
                            m.insert_block(block_hash, block).await?;
                            m.insert_state(state, leaf_hash).await?;
                            m.insert_qc(qc).await?;
                            Ok(())
                        })
                        .await
                    {
                        error!(?e, "Could not insert incoming QC");
                    }
                    // Make sure to update the background round runner
                    if let Err(e) = self
                        .inner
                        .background_task_handle
                        .set_round_runner_view_number(new_view_number)
                        .await
                    {
                        error!(
                            ?e,
                            "Could not update the background round runner of a new view number"
                        );
                    }

                    // Broadcast that we're updated
                    self.send_event(Event {
                        view_number: new_view_number,
                        stage: Stage::None,
                        event: EventType::Synced {
                            view_number: new_view_number,
                        },
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

                match load_latest_state::<I, N>(&self.inner.storage).await {
                    Ok(Some((quorum_certificate, leaf, state))) => {
                        let hotshot = self.clone();
                        let msg = DataMessage::NewestQuorumCertificate {
                            quorum_certificate,
                            state,
                            block: leaf.item,
                        };

                        if let Err(e) = hotshot.send_direct_message(msg, peer.clone()).await {
                            error!(
                                ?e,
                                "Could not send newest quorumcertificate to node {:?}", peer
                            );
                        }
                    }
                    Ok(None) => {
                        error!("Node connected but we have no QC yet");
                    }
                    Err(e) => {
                        error!(?e, "Could not retrieve newest QC");
                    }
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

/// Load the latest [`QuorumCertificate`] and the relevant [`Leaf`] and [`hotshot_types::traits::State`] from the given [`Storage`]
async fn load_latest_state<I: NodeImplementation<N>, const N: usize>(
    storage: &I::Storage,
) -> std::result::Result<
    Option<(QuorumCertificate<N>, Leaf<I::Block, N>, I::State)>,
    Box<dyn std::error::Error + Send + Sync + 'static>,
> {
    let qc = match storage.get_newest_qc().await? {
        Some(qc) => qc,
        None => return Ok(None),
    };
    let leaf = match storage.get_leaf(&qc.leaf_hash).await? {
        Some(leaf) => leaf,
        None => return Ok(None),
    };
    let state = match storage.get_state(&qc.leaf_hash).await? {
        Some(state) => state,
        None => return Ok(None),
    };
    Ok(Some((qc, leaf, state)))
}

/// A handle that is passed to [`hotshot_hotstuff`] with to expose the interface that hotstuff needs to interact with [`HotShot`]
struct HotShotConsensusApi<'a, I: NodeImplementation<N>, const N: usize> {
    /// Reference to the [`HotShotInner`]
    inner: &'a HotShotInner<I, N>,
}

#[async_trait]
impl<'a, I: NodeImplementation<N>, const N: usize> hotshot_consensus::ConsensusApi<I, N>
    for HotShotConsensusApi<'a, I, N>
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

    async fn get_leader(&self, view_number: ViewNumber, stage: Stage) -> I::SignatureKey {
        let election = &self.inner.election;
        election
            .election
            .get_leader(&election.stake_table, view_number, stage)
    }

    async fn should_start_round(&self, _: ViewNumber) -> bool {
        false
    }

    async fn send_direct_message(
        &mut self,
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
        &mut self,
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

    async fn send_event(&mut self, event: Event<I::Block, I::State, N>) {
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
        stage: Stage,
        view_number: ViewNumber,
    ) -> VerifyHash<32> {
        create_verify_hash(leaf_hash, view_number, stage)
    }

    #[instrument(skip(self))]
    fn validate_qc(
        &self,
        qc: &QuorumCertificate<N>,
        view_number: ViewNumber,
        stage: Stage,
    ) -> bool {
        if qc.view_number != view_number || qc.stage != stage {
            warn!(
                ?qc,
                ?view_number,
                ?stage,
                "Failing on stage/view_number equality check"
            );
        }
        let hash = create_verify_hash(&qc.leaf_hash, view_number, stage);
        let valid_signatures = self.get_valid_signatures(qc.signatures.clone(), hash);
        match valid_signatures {
            Ok(_) => true,
            Err(_e) => false,
        }
    }

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
