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
    clippy::similar_names
)]
// Temporary
#![allow(clippy::cast_possible_truncation)]
// Temporary, should be disabled after the completion of the NodeImplementation refactor
#![allow(clippy::type_complexity)]
//! Provides a generic rust implementation of the [`PhaseLock`](https://arxiv.org/abs/1803.05069) BFT
//! protocol

/// Contains structures and functions for committee election
pub mod committee;
/// Provides types useful for representing `PhaseLock ()`'s data structures
pub mod data;
/// Contains integration test versions of various demos
#[cfg(any(feature = "demo"))]
pub mod demos;
/// Contains error types used by this library
pub mod error;
/// Contains representations of events
pub mod event;
/// Contains the handle type for interacting with a `PhaseLock` instance
pub mod handle;
/// Contains structures used for representing network messages
pub mod message;
/// Contains traits describing and implementations of networking layers
pub mod networking;
/// Representation of the round logic as state machine
pub mod state_machine;
/// Contains traits consumed by `HotStuff`
pub mod traits;
/// Contains general utility structures and methods
pub mod utility;

use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

use async_std::sync::RwLock;
use async_std::task::{spawn, yield_now, JoinHandle};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use tracing::{debug, error, info, info_span, instrument, trace, warn, Instrument};

use crate::data::Leaf;
use crate::error::{NetworkFault, PhaseLockError};
use crate::event::{Event, EventType};
use crate::handle::PhaseLockHandle;
use crate::message::{Commit, Decide, Message, NewView, PreCommit, Prepare, Vote};
use crate::networking::NetworkingImplementation;
use crate::utility::broadcast::BroadcastSender;
use crate::utility::waitqueue::{WaitOnce, WaitQueue};

pub use rand;
pub use threshold_crypto as tc;

pub use crate::{
    data::{BlockHash, QuorumCertificate, Stage},
    traits::block_contents::BlockContents,
    traits::node_implementation::NodeImplementation,
    traits::storage::{Storage, StorageResult},
};

/// Length, in bytes, of a 512 bit hash
pub const H_512: usize = 64;
/// Length, in bytes, of a 256 bit hash
pub const H_256: usize = 32;

/// Convenience type alias
type Result<T> = std::result::Result<T, PhaseLockError>;

/// Public key type
///
/// Opaque wrapper around `threshold_crypto` key
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct PubKey {
    /// Overall public key set for the network
    set: tc::PublicKeySet,
    /// The public key share that this node holds
    node: tc::PublicKeyShare,
    /// The portion of the KeyShare this node holds
    pub nonce: u64,
}

impl PubKey {
    /// Testing only random key generation
    #[allow(dead_code)]
    pub(crate) fn random(nonce: u64) -> PubKey {
        let sks = tc::SecretKeySet::random(1, &mut rand::thread_rng());
        let set = sks.public_keys();
        let node = set.public_key_share(nonce);
        PubKey { set, node, nonce }
    }
    /// Temporary escape hatch to generate a `PubKey` from a `SecretKeySet` and a node id
    ///
    /// This _will_ be removed when shared secret generation is implemented. For now, it exists to solve the
    /// resulting chicken and egg problem.
    pub fn from_secret_key_set_escape_hatch(sks: &tc::SecretKeySet, node_id: u64) -> Self {
        let pks = sks.public_keys();
        let tc_pub_key = pks.public_key_share(node_id);
        PubKey {
            set: pks,
            node: tc_pub_key,
            nonce: node_id,
        }
    }
}

impl PartialOrd for PubKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.nonce.partial_cmp(&other.nonce)
    }
}

impl Ord for PubKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.nonce.cmp(&other.nonce)
    }
}

impl Debug for PubKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PubKey").field("id", &self.nonce).finish()
    }
}

/// Private key stub type
///
/// Opaque wrapper around `threshold_crypto` key
#[derive(Clone, Debug)]
pub struct PrivKey {
    /// This node's share of the overall secret key
    node: tc::SecretKeyShare,
}

impl PrivKey {
    /// Uses this private key to produce a partial signature for the given block hash
    #[must_use]
    pub fn partial_sign<const N: usize>(
        &self,
        hash: &BlockHash<N>,
        _stage: Stage,
        _view: u64,
    ) -> tc::SignatureShare {
        self.node.sign(hash)
    }
}

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
    committed_state: RwLock<Arc<<<I as NodeImplementation<N>>::Block as BlockContents<N>>::State>>,
    /// Current committed leaf
    committed_leaf: RwLock<BlockHash<N>>,
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
    prepare_waiter: WaitOnce<Prepare<I::Block, N>>,
    /// Currently pending precommit message
    precommit_waiter: WaitOnce<PreCommit<N>>,
    /// Currently pending Commit message
    commit_waiter: WaitOnce<Commit<N>>,
    /// Currently pending decide message
    decide_waiter: WaitOnce<Decide<N>>,
    /// This `PhaseLock` instance's storage backend
    storage: I::Storage,
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
        starting_state: <<I as NodeImplementation<N>>::Block as BlockContents<N>>::State,
        networking: I::Networking,
        storage: I::Storage,
    ) -> Self {
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
        };
        inner.storage.insert_qc(QuorumCertificate {
            block_hash: genesis_hash,
            leaf_hash: leaf.hash(),
            view_number: 0,
            stage: Stage::Decide,
            signature: None,
            genesis: true,
        });
        inner
            .storage
            .insert_leaf(Leaf {
                parent: [0_u8; { N }].into(),
                item: genesis,
            })
            .await;
        error!("Genesis leaf hash: {:?}", leaf.hash());
        inner
            .storage
            .insert_state(starting_state, leaf.hash())
            .await;
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Returns true if the proposed leaf extends from the given block
    #[instrument(skip(self),fields(id = self.inner.public_key.nonce))]
    pub async fn extends_from(&self, leaf: &Leaf<I::Block, N>, node: &BlockHash<N>) -> bool {
        let mut parent = leaf.parent;
        // Short circuit to enable blocks that don't have parents
        if &parent == node {
            trace!("leaf extends from node through short-circuit");
            return true;
        }
        while parent != BlockHash::from_array([0_u8; { N }]) {
            if &parent == node {
                trace!(?parent, "Leaf extends from");
                return true;
            }
            let next_parent = self.inner.storage.get_leaf(&parent).await;
            if let StorageResult::Some(next_parent) = next_parent {
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
            let extends_from = self.extends_from(leaf, &locked_qc.block_hash).await;
            let view_number = qc.view_number > locked_qc.view_number;
            let result = extends_from || view_number;
            if !result {
                error!("Safe node check failed");
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
    #[instrument(skip(self,channel),fields(id = self.inner.public_key.nonce),err)]
    pub async fn next_view(
        &self,
        current_view: u64,
        channel: Option<
            &BroadcastSender<
                Event<I::Block, <<I as NodeImplementation<N>>::Block as BlockContents<N>>::State>,
            >,
        >,
    ) -> Result<()> {
        let new_leader = self.inner.get_leader(current_view + 1);
        info!(?new_leader, "leader for next view");
        // If we are the new leader, do nothing
        #[allow(clippy::if_not_else)]
        if new_leader != self.inner.public_key {
            info!("Follower for this round");
            let view_message = Message::NewView(NewView {
                current_view,
                justify: self.inner.prepare_qc.read().await.as_ref().unwrap().clone(),
            });
            trace!("View message packed");
            self.inner
                .networking
                .message_node(view_message, new_leader)
                .await
                .context(NetworkFault)?;
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
        send_event::<
            I::Block,
            <<I as NodeImplementation<N>>::Block as BlockContents<N>>::State,
            { N },
        >(
            channel,
            Event {
                view_number: current_view,
                stage: Stage::None,
                event: EventType::NewView {
                    view_number: current_view,
                },
            },
        )
        .await;
        Ok(())
    }

    /// Runs a single round of consensus
    ///
    /// Returns the view number of the round that was completed.
    ///
    /// # Panics
    ///
    /// Panics if consensus hits a bad round
    #[allow(clippy::too_many_lines)]
    #[instrument(skip(self,channel),fields(id = self.inner.public_key.nonce),err)]
    pub async fn run_round(
        &self,
        current_view: u64,
        channel: Option<
            &BroadcastSender<
                Event<I::Block, <<I as NodeImplementation<N>>::Block as BlockContents<N>>::State>,
            >,
        >,
    ) -> Result<u64> {
        let state = state_machine::SequentialRound::new(self.clone(), current_view, channel);
        state.await
    }

    /// Spawns the background tasks for network processin for this instance
    ///
    /// These will process in the background and load items into their designated queues
    ///
    /// # Panics
    ///
    /// Panics if the underlying network implementation incorrectly routes a network request to the
    /// wrong queue
    pub async fn spawn_networking_tasks(&self) {
        let x = self.clone();
        // Spawn broadcast processing task
        spawn(
            async move {
                info!("Launching broadcast processing task");
                let networking = &x.inner.networking;
                let phaselock = &x.inner;
                while let Ok(queue) = networking.broadcast_queue().await {
                    debug!(?queue, "Processing messages");
                    if queue.is_empty() {
                        trace!("No message, yeilding");
                        yield_now().await;
                    } else {
                        for item in queue {
                            trace!(?item, "Processing item");
                            match item {
                                Message::Prepare(p) => {
                                    // Insert block into store
                                    info!(prepare = ?p, "Inserting block and leaf into store");
                                    let leaf = p.leaf.clone();
                                    phaselock.storage.insert_leaf(leaf.clone()).await;
                                    phaselock
                                        .storage
                                        .insert_block(BlockContents::hash(&leaf.item), leaf.item);
                                    phaselock.prepare_waiter.put(p).await;
                                }
                                Message::PreCommit(pc) => phaselock.precommit_waiter.put(pc).await,
                                Message::Commit(c) => phaselock.commit_waiter.put(c).await,
                                Message::Decide(d) => phaselock.decide_waiter.put(d).await,
                                Message::SubmitTransaction(d) => {
                                    phaselock.transaction_queue.write().await.push(d);
                                }
                                _ => {
                                    // Log the exceptional situation and proceed
                                    warn!(?item, "Direct message received over broadcast channel");
                                }
                            }
                        }
                        trace!("Item processed, yeilding");
                        yield_now().await;
                    }
                }
            }
            .instrument(info_span!(
                "PhaseLock Broadcast Task",
                id = self.inner.public_key.nonce
            )),
        );
        let x = self.clone();
        // Spawn direct processing task
        spawn(
            async move {
                info!("Launching direct processing task");
                let phaselock = &x.inner;
                let networking = &x.inner.networking;
                while let Ok(queue) = networking.direct_queue().await {
                    debug!(?queue, "Processing messages");
                    if queue.is_empty() {
                        trace!("No message, yeilding");
                        yield_now().await;
                    } else {
                        for item in queue {
                            trace!(?item, "Processing item");
                            match item {
                                Message::NewView(nv) => phaselock.new_view_queue.push(nv).await,
                                Message::PrepareVote(pv) => {
                                    phaselock.prepare_vote_queue.push(pv).await;
                                }
                                Message::PreCommitVote(pcv) => {
                                    phaselock.precommit_vote_queue.push(pcv).await;
                                }
                                Message::CommitVote(cv) => {
                                    phaselock.commit_vote_queue.push(cv).await;
                                }
                                _ => {
                                    // Log exceptional situation and proceed
                                    warn!(?item, "Broadcast message received over direct channel");
                                }
                            }
                        }
                        trace!("Item processed, yeilding");
                        yield_now().await;
                    }
                }
            }
            .instrument(info_span!(
                "PhaseLock Direct Task",
                id = self.inner.public_key.nonce
            )),
        );
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
        let message = Message::SubmitTransaction(tx);
        self.inner
            .networking
            .broadcast_message(message.clone())
            .await
            .context(NetworkFault)?;
        debug!(?message, "Message broadcasted");
        Ok(())
    }

    /// Returns a copy of the state
    pub async fn get_state(
        &self,
    ) -> Arc<<<I as NodeImplementation<N>>::Block as BlockContents<N>>::State> {
        self.inner.committed_state.read().await.clone()
    }

    /// Initializes a new phaselock and does the work of setting up all the background tasks
    ///
    /// Assumes networking implementation is already primed.
    ///
    /// Underlying `PhaseLock` instance starts out paused, and must be unpaused
    ///
    /// Upon encountering an unrecoverable error, such as a failure to send to a broadcast channel, the
    /// `PhaseLock` instance will log the error and shut down.
    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    pub async fn init(
        genesis: I::Block,
        public_keys: tc::PublicKeySet,
        secret_key_share: tc::SecretKeyShare,
        node_id: u64,
        config: PhaseLockConfig,
        starting_state: <<I as NodeImplementation<N>>::Block as BlockContents<N>>::State,
        networking: I::Networking,
        storage: I::Storage,
    ) -> (JoinHandle<()>, PhaseLockHandle<I, N>) {
        let (input, output) = crate::utility::broadcast::channel();
        // Save a clone of the storage for the handle
        let phaselock = Self::new(
            genesis,
            public_keys,
            secret_key_share,
            node_id,
            config.clone(),
            starting_state,
            networking,
            storage.clone(),
        )
        .await;
        let pause = Arc::new(RwLock::new(true));
        let run_once = Arc::new(RwLock::new(false));
        let shut_down = Arc::new(RwLock::new(false));
        // Spawn the background tasks
        phaselock.spawn_networking_tasks().await;
        let handle = PhaseLockHandle {
            sender_handle: Arc::new(input.clone()),
            phaselock: phaselock.clone(),
            stream_output: output,
            pause: pause.clone(),
            run_once: run_once.clone(),
            shut_down: shut_down.clone(),
            storage,
        };
        let task = spawn(
            async move {
                let channel = input;
                let default_interrupt_duration = phaselock.inner.config.next_view_timeout;
                let (int_mul, int_div) = phaselock.inner.config.timeout_ratio;
                let mut int_duration = default_interrupt_duration;
                let mut view = 0;
                // PhaseLock background handler loop
                loop {
                    // First, check for shutdown signal and break if sent
                    if *shut_down.read().await {
                        break;
                    }
                    // Capture the pause and run_once flags
                    // Reset the run_once flag if its set
                    let p_flag = {
                        let p = pause.read().await;
                        let mut r = run_once.write().await;
                        if *r {
                            *r = false;
                            false
                        } else {
                            *p
                        }
                    };
                    // If we are paused, yield and continue
                    if p_flag {
                        yield_now().await;
                        continue;
                    }
                    // Send the next view
                    let next_view_res = phaselock.next_view(view, Some(&channel)).await;
                    // If we fail to send the next view, broadcast the error and pause
                    if let Err(e) = next_view_res {
                        let x = channel
                            .send_async(Event {
                                view_number: view,
                                stage: e.get_stage().unwrap_or(Stage::None),

                                event: EventType::Error { error: Arc::new(e) },
                            })
                            .await;
                        if x.is_err() {
                            error!("All event streams closed! Shutting down.");
                            break;
                        }
                        *pause.write().await = true;
                        continue;
                    }
                    // Increment the view counter
                    view += 1;
                    // run the next block, with a timeout
                    let t = Duration::from_millis(int_duration);
                    let round_res =
                        async_std::future::timeout(t, phaselock.run_round(view, Some(&channel)))
                            .await;
                    match round_res {
                        // If it succeded, simply reset the timeout
                        Ok(Ok(x)) => {
                            int_duration = default_interrupt_duration;
                            // Check if we completed the same view we started
                            if x != view {
                                info!(?x, ?view, "Round short circuited");
                                view = x;
                            }
                        }
                        // If it errored, broadcast the error, reset the timeout, and continue
                        Ok(Err(e)) => {
                            let x = channel
                                .send_async(Event {
                                    view_number: view,
                                    stage: e.get_stage().unwrap_or(Stage::None),
                                    event: EventType::Error { error: Arc::new(e) },
                                })
                                .await;
                            if x.is_err() {
                                error!("All event streams closed! Shutting down.");
                                break;
                            }
                            continue;
                        }
                        // if we timed out, log it, send the event, and increase the timeout
                        Err(_) => {
                            warn!("Round timed out");
                            let x = channel
                                .send_async(Event {
                                    view_number: view,
                                    stage: Stage::None,
                                    event: EventType::ViewTimeout { view_number: view },
                                })
                                .await;
                            if x.is_err() {
                                error!("All event streams closed! Shutting down.");
                                break;
                            }
                            int_duration = (int_duration * int_mul) / int_div;
                        }
                    }
                }
            }
            .instrument(info_span!("PhaseLock Background Driver", id = node_id)),
        );
        (task, handle)
    }
}

/// Attempts to generate a quorum certificate from the provided signatures
fn generate_qc<'a>(
    signatures: impl IntoIterator<Item = (u64, &'a tc::SignatureShare)>,
    key_set: &tc::PublicKeySet,
) -> std::result::Result<tc::Signature, tc::error::Error> {
    key_set.combine_signatures(signatures)
}

/// Sends an event over a `Some(BroadcastSender<T>)`, does nothing otherwise
async fn send_event<B, S, const N: usize>(
    channel: Option<&BroadcastSender<Event<B, S>>>,
    event: Event<B, S>,
) where
    B: Send + Sync + Clone,
    S: Send + Sync + Clone,
{
    if let Some(c) = channel {
        let _result = c.send_async(event).await;
    }
}
