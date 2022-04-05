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
    clippy::unused_self
)]
// Temporary
#![allow(clippy::cast_possible_truncation)]
// Temporary, should be disabled after the completion of the NodeImplementation refactor
#![allow(clippy::type_complexity)]
//! Provides a generic rust implementation of the `PhaseLock` BFT protocol
//!
//! See the [protocol documentation](documentation) for a protocol description.

// Documentation module
#[cfg(feature = "docs")]
pub mod documentation;

/// Contains structures and functions for committee election
pub mod committee;
pub mod data;
#[cfg(any(feature = "demo"))]
pub mod demos;
pub mod state_machine;
/// Contains traits consumed by [`PhaseLock`]
pub mod traits;
/// Contains types used by the crate
pub mod types;

mod tasks;

use crate::{
    data::{Leaf, LeafHash, QuorumCertificate, Stage},
    traits::{BlockContents, NetworkingImplementation, NodeImplementation, Storage},
    types::{Commit, Decide, Event, EventType, NewView, PhaseLockHandle, PreCommit, Prepare, Vote},
};
use async_std::sync::{Mutex, RwLock};
use phaselock_types::{
    error::{NetworkFaultSnafu, StorageSnafu},
    message::{ConsensusMessage, DataMessage},
    traits::{
        network::{NetworkChange, NetworkError},
        node_implementation::TypeMap,
    },
};
use phaselock_utils::{
    broadcast::BroadcastSender,
    waitqueue::{WaitOnce, WaitQueue},
};
use snafu::ResultExt;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, instrument, trace, warn};

// -- Rexports
// External
/// Reexport rand crate
pub use rand;
/// Reexport threshold crypto crate
pub use threshold_crypto as tc;
// Internal
/// Reexport error type
pub use phaselock_types::error::PhaseLockError;
/// Reexport key types
pub use phaselock_types::{PrivKey, PubKey};

/// Length, in bytes, of a 512 bit hash
pub const H_512: usize = 64;
/// Length, in bytes, of a 256 bit hash
pub const H_256: usize = 32;

/// Convenience type alias
type Result<T> = std::result::Result<T, PhaseLockError>;

/// Holds configuration for a `PhaseLock`
#[derive(Debug, Clone)]
pub struct PhaseLockConfig {
    /// Total number of nodes in the network
    pub total_nodes: u32,
    /// Nodes required to reach a decision
    pub threshold: u32,
    /// Maximum transactions per block
    pub max_transactions: usize,
    /// List of known node's public keys, including own, sorted by nonce ()
    pub known_nodes: Vec<PubKey>,
    /// Base duration for next-view timeout, in milliseconds
    pub next_view_timeout: u64,
    /// The exponential backoff ration for the next-view timeout
    pub timeout_ratio: (u64, u64),
    /// The delay a leader inserts before starting pre-commit, in milliseconds
    pub round_start_delay: u64,
    /// Delay after init before starting consensus, in milliseconds
    pub start_delay: u64,
}

/// Holds the state needed to participate in `PhaseLock` consensus
pub struct PhaseLockInner<I: NodeImplementation<N>, const N: usize> {
    /// The public key of this node
    public_key: PubKey,
    /// The private key of this node
    private_key: PrivKey,
    /// The genesis block, used for short-circuiting during bootstrap
    #[allow(dead_code)]
    genesis: I::Block,
    /// Configuration items for this phaselock instance
    config: PhaseLockConfig,
    /// Networking interface for this phaselock instance
    networking: I::Networking,
    /// Pending transactions
    transaction_queue:
        RwLock<Vec<<<I as NodeImplementation<N>>::Block as BlockContents<N>>::Transaction>>,
    /// Current state
    committed_state: RwLock<Arc<I::State>>,
    /// Current committed leaf
    committed_leaf: RwLock<LeafHash<N>>,
    /// Current locked quorum certificate
    locked_qc: RwLock<Option<QuorumCertificate<N>>>,
    /// Current prepare quorum certificate
    prepare_qc: RwLock<Option<QuorumCertificate<N>>>,
    /// Unprocessed NextView messages
    new_view_queue: WaitQueue<NewView<N>>,
    /// Unprocessed PrepareVote messages
    prepare_vote_queue: WaitQueue<Vote<N>>,
    /// Unprocessed PreCommit messages
    precommit_vote_queue: WaitQueue<Vote<N>>,
    /// Unprocessed CommitVote messages
    commit_vote_queue: WaitQueue<Vote<N>>,
    /// Currently pending Prepare message
    prepare_waiter: WaitOnce<Prepare<I::Block, I::State, N>>,
    /// Currently pending precommit message
    precommit_waiter: WaitOnce<PreCommit<N>>,
    /// Currently pending Commit message
    commit_waiter: WaitOnce<Commit<N>>,
    /// Currently pending decide message
    decide_waiter: WaitOnce<Decide<N>>,
    /// This `PhaseLock` instance's storage backend
    storage: I::Storage,
    /// This `PhaseLock` instance's stateful callback handler
    stateful_handler: Mutex<I::StatefulHandler>,

    /// Sender for [`Event`]s
    event_sender: RwLock<Option<BroadcastSender<Event<I::Block, I::State>>>>,

    /// Senders to the background tasks.
    background_task_handle: tasks::TaskHandle,
}

impl<I: NodeImplementation<N>, const N: usize> PhaseLockInner<I, N> {
    /// Returns the public key for the leader of this round
    fn get_leader(&self, view: u64) -> PubKey {
        let index = view % u64::from(self.config.total_nodes);
        self.config.known_nodes[index as usize].clone()
    }
}

/// Thread safe, shared view of a `PhaseLock`
#[derive(Clone)]
pub struct PhaseLock<I: NodeImplementation<N> + Send + Sync + 'static, const N: usize> {
    /// Handle to internal phaselock implementation
    inner: Arc<PhaseLockInner<I, N>>,
}

impl<I: NodeImplementation<N> + Sync + Send + 'static, const N: usize> PhaseLock<I, N> {
    /// Creates a new phaselock with the given configuration options and sets it up with the given
    /// genesis block
    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    #[instrument(skip(genesis, secret_key_share, starting_state, networking, storage))]
    pub async fn new(
        genesis: I::Block,
        public_keys: tc::PublicKeySet,
        secret_key_share: tc::SecretKeyShare,
        nonce: u64,
        config: PhaseLockConfig,
        starting_state: I::State,
        networking: I::Networking,
        storage: I::Storage,
        handler: I::StatefulHandler,
    ) -> Result<Self> {
        info!("Creating a new phaselock");
        let node_pub_key = secret_key_share.public_key_share();
        let genesis_hash = BlockContents::hash(&genesis);
        let t = config.threshold as usize;
        let leaf = Leaf {
            parent: [0_u8; { N }].into(),
            item: genesis.clone(),
        };
        let inner: PhaseLockInner<I, N> = PhaseLockInner {
            public_key: PubKey {
                set: public_keys,
                node: node_pub_key,
                nonce,
            },
            private_key: PrivKey {
                node: secret_key_share,
            },
            genesis: genesis.clone(),
            config,
            networking,
            transaction_queue: RwLock::new(Vec::new()),
            committed_state: RwLock::new(Arc::new(starting_state.clone())),
            committed_leaf: RwLock::new(leaf.hash()),
            locked_qc: RwLock::new(Some(QuorumCertificate {
                block_hash: genesis_hash,
                leaf_hash: leaf.hash(),
                view_number: 0,
                stage: Stage::Decide,
                signature: None,
                genesis: true,
            })),
            prepare_qc: RwLock::new(Some(QuorumCertificate {
                block_hash: genesis_hash,
                leaf_hash: leaf.hash(),
                view_number: 0,
                stage: Stage::Prepare,
                signature: None,
                genesis: true,
            })),
            new_view_queue: WaitQueue::new(t),
            prepare_vote_queue: WaitQueue::new(t),
            precommit_vote_queue: WaitQueue::new(t),
            commit_vote_queue: WaitQueue::new(t),
            prepare_waiter: WaitOnce::new(),
            precommit_waiter: WaitOnce::new(),
            commit_waiter: WaitOnce::new(),
            decide_waiter: WaitOnce::new(),
            storage,
            stateful_handler: Mutex::new(handler),
            event_sender: RwLock::default(),
            background_task_handle: tasks::TaskHandle::default(),
        };
        let leaf_hash = leaf.hash();
        trace!("Genesis leaf hash: {:?}", leaf_hash);

        inner
            .storage
            .update(|mut m| async move {
                m.insert_qc(QuorumCertificate {
                    block_hash: genesis_hash,
                    leaf_hash,
                    view_number: 0,
                    stage: Stage::Decide,
                    signature: None,
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
    #[instrument(skip(self),fields(id = self.inner.public_key.nonce))]
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

    /// Returns true if a proposed leaf satisfies the safety rule
    #[instrument(skip(self),fields(id = self.inner.public_key.nonce))]
    pub async fn safe_node(&self, leaf: &Leaf<I::Block, N>, qc: &QuorumCertificate<N>) -> bool {
        if qc.genesis {
            info!("Safe node check bypassed due to genesis flag");
            return true;
        }
        if let Some(locked_qc) = self.inner.locked_qc.read().await.as_ref() {
            let extends_from = self.extends_from(leaf, &locked_qc.leaf_hash).await;
            let view_number = qc.view_number > locked_qc.view_number;
            let result = extends_from || view_number;
            if !result {
                error!(?locked_qc, ?leaf, ?qc, "Safe node check failed");
            }
            result
        } else {
            error!("Safe node check failed");
            false
        }
    }

    /// Sends out the next view message
    ///
    /// # Panics
    ///
    /// Panics if we there is no `prepare_qc`
    ///
    /// # Errors
    ///
    /// Returns an error if an underlying networking error occurs
    #[instrument(skip(self),fields(id = self.inner.public_key.nonce),err)]
    pub async fn next_view(&self, current_view: u64) -> Result<()> {
        let new_leader = self.inner.get_leader(current_view + 1);
        info!(?new_leader, "leader for next view");
        // If we are the new leader, do nothing
        #[allow(clippy::if_not_else)]
        if new_leader != self.inner.public_key {
            info!("Follower for this round");
            let view_message = ConsensusMessage::NewView(NewView {
                current_view,
                justify: self.inner.prepare_qc.read().await.as_ref().unwrap().clone(),
            });
            trace!("View message packed");
            let network_result = self
                .send_direct_message(view_message, new_leader)
                .await
                .context(NetworkFaultSnafu);
            if let Err(e) = network_result {
                warn!(?e, "Failed to send new view message to node");
            };
            trace!("View change message sent");
        } else {
            info!("Leader for this round, sending self new_view");
            let view_message = NewView {
                current_view,
                justify: self.inner.prepare_qc.read().await.as_ref().unwrap().clone(),
            };
            trace!("NewView packed");
            self.inner.new_view_queue.push(view_message).await;
        }
        self.send_event(Event {
            view_number: current_view,
            stage: Stage::None,
            event: EventType::NewView {
                view_number: current_view,
            },
        })
        .await;
        Ok(())
    }

    /// Sends an event over an event broadcaster if one is registered, does nothing otherwise
    ///
    /// Returns `true` if the event was send, `false` otherwise
    pub async fn send_event(&self, event: Event<I::Block, I::State>) -> bool {
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
    #[allow(clippy::too_many_lines)]
    #[instrument(skip(self),fields(id = self.inner.public_key.nonce),err)]
    pub async fn run_round(&self, current_view: u64) -> Result<u64> {
        let state = state_machine::SequentialRound::new(self.clone(), current_view).await;
        // Do waitup
        let time = Instant::now();
        let duration = Duration::from_millis(self.inner.config.round_start_delay);
        while Instant::now().duration_since(time) < duration {
            async_std::task::sleep(Duration::from_millis(1)).await;
        }
        state.await
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
        self.inner.transaction_queue.write().await.push(tx.clone());
        // Wrap up a message
        let message = ConsensusMessage::SubmitTransaction(tx);
        let network_result = self
            .send_broadcast_message(message.clone())
            .await
            .context(NetworkFaultSnafu);
        if let Err(e) = network_result {
            warn!(?e, "Failed to publish a transaction");
        };
        debug!(?message, "Message broadcasted");
        Ok(())
    }

    /// Returns a copy of the state
    pub async fn get_state(&self) -> Arc<I::State> {
        self.inner.committed_state.read().await.clone()
    }

    /// Initializes a new phaselock and does the work of setting up all the background tasks
    ///
    /// Assumes networking implementation is already primed.
    ///
    /// Underlying `PhaseLock` instance starts out paused, and must be unpaused
    ///
    /// Upon encountering an unrecoverable error, such as a failure to send to a broadcast channel,
    /// the `PhaseLock` instance will log the error and shut down.
    ///
    /// # Errors
    ///
    /// Will return an error when the storage failed to insert the first `QuorumCertificate`
    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    pub async fn init(
        genesis: I::Block,
        public_keys: tc::PublicKeySet,
        secret_key_share: tc::SecretKeyShare,
        node_id: u64,
        config: PhaseLockConfig,
        starting_state: I::State,
        networking: I::Networking,
        storage: I::Storage,
        handler: I::StatefulHandler,
    ) -> Result<PhaseLockHandle<I, N>> {
        // Save a clone of the storage for the handle
        let phaselock = Self::new(
            genesis,
            public_keys,
            secret_key_share,
            node_id,
            config,
            starting_state,
            networking,
            storage,
            handler,
        )
        .await?;
        let handle = tasks::spawn_all(&phaselock).await;

        Ok(handle)
    }

    /// Send a broadcast message.
    ///
    /// This is an alias for `phaselock.inner.networking.broadcast_message(msg.into())`.
    ///
    /// # Errors
    ///
    /// Will return any errors that the underlying `broadcast_message` can return.
    pub async fn send_broadcast_message(
        &self,
        msg: impl Into<<I as TypeMap<N>>::Message>,
    ) -> std::result::Result<(), NetworkError> {
        self.inner.networking.broadcast_message(msg.into()).await
    }

    /// Send a direct message to a given recipient.
    ///
    /// This is an alias for `phaselock.inner.networking.message_node(msg.into(), recipient)`.
    ///
    /// # Errors
    ///
    /// Will return any errors that the underlying `message_node` can return.
    pub async fn send_direct_message(
        &self,
        msg: impl Into<<I as TypeMap<N>>::Message>,
        recipient: PubKey,
    ) -> std::result::Result<(), NetworkError> {
        self.inner
            .networking
            .message_node(msg.into(), recipient)
            .await
    }

    /// Handle an incoming [`ConsensusMessage`] that was broadcasted on the network.
    async fn handle_broadcast_consensus_message(&self, msg: <I as TypeMap<N>>::ConsensusMessage) {
        match msg {
            ConsensusMessage::Prepare(p) => {
                // Insert block into store
                info!(prepare = ?p, "Inserting block and leaf into store");

                if let Err(e) = self
                    .inner
                    .storage
                    .update(|mut m| {
                        let leaf = p.leaf.clone();
                        let state = p.state.clone();
                        async move {
                            m.insert_leaf(leaf.clone()).await?;
                            m.insert_block(BlockContents::hash(&leaf.item), leaf.item.clone())
                                .await?;
                            m.insert_state(state.clone(), leaf.hash()).await?;
                            Ok(())
                        }
                    })
                    .await
                {
                    error!(?e, "Error inserting leaf into storage");
                    return;
                }

                self.inner.prepare_waiter.put(p).await;
            }
            ConsensusMessage::PreCommit(pc) => self.inner.precommit_waiter.put(pc).await,
            ConsensusMessage::Commit(c) => self.inner.commit_waiter.put(c).await,
            ConsensusMessage::Decide(d) => self.inner.decide_waiter.put(d).await,
            ConsensusMessage::SubmitTransaction(d) => {
                self.inner.transaction_queue.write().await.push(d);
            }
            _ => {
                // Log the exceptional situation and proceed
                warn!(?msg, "Direct message received over broadcast channel");
            }
        }
    }

    /// Handle an incoming [`ConsensusMessage`] directed at this node.
    async fn handle_direct_consensus_message(&self, msg: <I as TypeMap<N>>::ConsensusMessage) {
        match msg {
            ConsensusMessage::NewView(nv) => self.inner.new_view_queue.push(nv).await,
            ConsensusMessage::PrepareVote(pv) => {
                self.inner.prepare_vote_queue.push(pv).await;
            }
            ConsensusMessage::PreCommitVote(pcv) => {
                self.inner.precommit_vote_queue.push(pcv).await;
            }
            ConsensusMessage::CommitVote(cv) => {
                self.inner.commit_vote_queue.push(cv).await;
            }
            _ => {
                // Log exceptional situation and proceed
                warn!(?msg, "Broadcast message received over direct channel");
            }
        }
    }

    /// Handle an incoming [`DataMessage`] that was broadcasted on the network
    async fn handle_broadcast_data_message(&self, msg: <I as TypeMap<N>>::DataMessage) {
        match msg {
            DataMessage::NewestQuorumCertificate { .. } => {}
        }
    }

    /// Handle an incoming [`DataMessage`] that directed at this node
    async fn handle_direct_data_message(&self, msg: <I as TypeMap<N>>::DataMessage) {
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

                    // And make sure to update the phaselock `committed_leaf`
                    *self.inner.committed_leaf.write().await = leaf_hash;

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
        }
    }

    /// Handle a change in the network
    async fn handle_network_change(&self, node: NetworkChange) {
        match node {
            NetworkChange::NodeConnected(peer) => {
                info!("Connected to node {:?}", peer);

                match load_latest_state::<I, N>(&self.inner.storage).await {
                    Ok(Some((quorum_certificate, leaf, state))) => {
                        let phaselock = self.clone();
                        if let Err(e) = phaselock
                            .send_direct_message(
                                DataMessage::NewestQuorumCertificate {
                                    quorum_certificate,
                                    state,
                                    block: leaf.item,
                                },
                                peer.clone(),
                            )
                            .await
                        {
                            error!(
                                ?e,
                                "Could not send newest quorumcertificate to node {:?}", peer
                            );
                        }
                    }
                    Ok(None) => {
                        info!("Node connected but we have no QC yet");
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
}

/// Attempts to generate a quorum certificate from the provided signatures
fn generate_qc<'a>(
    signatures: impl IntoIterator<Item = (u64, &'a tc::SignatureShare)>,
    key_set: &tc::PublicKeySet,
) -> std::result::Result<tc::Signature, tc::error::Error> {
    key_set.combine_signatures(signatures)
}

/// Load the latest [`QuorumCertificate`] and the relevant [`Leaf`] and [`State`] from the given [`Storage`]
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
