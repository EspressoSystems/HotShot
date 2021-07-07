#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]
#![warn(clippy::missing_docs_in_private_items)]
#![allow(clippy::option_if_let_else)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
// Temporary
#![allow(clippy::cast_possible_truncation)]
// Given that consensus should guarantee the ability to recover from errors, explicit panics should
// be strictly forbidden
#![warn(clippy::panic)]
//! Provides a generic rust implementation of the [`PhaseLock`](https://arxiv.org/abs/1803.05069) BFT
//! protocol

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
/// Contains general utility structures and methods
pub mod utility;

use std::error::Error;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

use async_std::sync::RwLock;
use async_std::task::{spawn, yield_now, JoinHandle};
use dashmap::DashMap;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use snafu::ResultExt;
use tokio::sync::broadcast;

use crate::data::Leaf;
use crate::error::{FailedToBroadcast, FailedToMessageLeader, PhaseLockError, NetworkFault};
use crate::event::{Event, EventType};
use crate::handle::PhaseLockHandle;
use crate::message::{Commit, Decide, Message, NewView, PreCommit, Prepare, Vote};
use crate::networking::NetworkingImplementation;
use crate::utility::waitqueue::{WaitOnce, WaitQueue};
use tracing::{debug, error, info, info_span, instrument, trace, warn, Instrument};

pub use rand;
pub use threshold_crypto as tc;

pub use crate::data::{BlockHash, QuorumCertificate, Stage};

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

/// The block trait
pub trait BlockContents<const N: usize>:
    Serialize + DeserializeOwned + Clone + Debug + Hash + PartialEq + Eq + Send + Sync
{
    /// The type of the state machine we are applying transitions to
    type State: Clone + Send + Sync;
    /// The type of the transitions we are applying
    type Transaction: Clone
        + Serialize
        + DeserializeOwned
        + Debug
        + Hash
        + PartialEq
        + Eq
        + Sync
        + Send;
    /// The error type for this state machine
    type Error: Error + Debug + Send + Sync;

    /// Creates a new block, currently devoid of transactions, given the current state.
    ///
    /// Allows the new block to reference any data from the state that its in.
    fn next_block(state: &Self::State) -> Self;

    /// Attempts to add a transaction, returning an Error if not compatible with the current state
    ///
    /// # Errors
    ///
    /// Should return an error if this transaction leads to an invalid block
    fn add_transaction(
        &self,
        state: &Self::State,
        tx: &Self::Transaction,
    ) -> std::result::Result<Self, Self::Error>;
    /// ensures that the block is append able to the current state
    fn validate_block(&self, state: &Self::State) -> bool;
    /// Appends the block to the state
    ///
    /// # Errors
    ///
    /// Should produce an error if this block leads to an invalid state
    fn append_to(&self, state: &Self::State) -> std::result::Result<Self::State, Self::Error>;
    /// Produces a hash for the contents of the block
    fn hash(&self) -> BlockHash<N>;
    /// Produces a hash for a transaction
    ///
    /// TODO: Abstract out into transaction trait
    fn hash_transaction(tx: &Self::Transaction) -> BlockHash<N>;
    /// Produces a hash for an arbitrary sequence of bytes
    ///
    /// Used to produce hashes for internal `PhaseLock` control structures
    fn hash_bytes(bytes: &[u8]) -> BlockHash<N>;
}

/// Holds configuration for a phaselock
#[derive(Debug, Clone)]
pub struct PhaseLockConfig {
    /// Total number of nodes in the network
    pub total_nodes: u32,
    /// Nodes required to reach a decision
    pub thershold: u32,
    /// Maximum transactions per block
    pub max_transactions: usize,
    /// List of known node's public keys, including own, sorted by nonce ()
    pub known_nodes: Vec<PubKey>,
    /// Base duration for next-view timeout, in milliseconds
    pub next_view_timeout: u64,
    /// The exponential backoff ration for the next-view timeout
    pub timeout_ratio: (u64, u64),
}

/// Holds the state needed to participate in `PhaseLock` consensus
pub struct PhaseLockInner<B: BlockContents<N> + 'static, const N: usize> {
    /// The public key of this node
    public_key: PubKey,
    /// The private key of this node
    private_key: PrivKey,
    /// The genesis block, used for short-circuiting during bootstrap
    #[allow(dead_code)]
    genesis: B,
    /// Configuration items for this phaselock instance
    config: PhaseLockConfig,
    /// Networking interface for this phaselock instance
    networking: Box<dyn NetworkingImplementation<Message<B, B::Transaction, N>>>,
    /// Pending transactions
    transaction_queue: RwLock<Vec<B::Transaction>>,
    /// Current state
    state: RwLock<Arc<B::State>>,
    /// Block storage
    leaf_store: DashMap<BlockHash<N>, Leaf<B, N>>,
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
    prepare_waiter: WaitOnce<Prepare<B, N>>,
    /// Currently pending precommit message
    precommit_waiter: WaitOnce<PreCommit<N>>,
    /// Currently pending Commit message
    commit_waiter: WaitOnce<Commit<N>>,
    /// Currently pending decide message
    decide_waiter: WaitOnce<Decide<N>>,
    /// Map from a block's hash to its decision QC
    decision_cache: DashMap<BlockHash<N>, QuorumCertificate<N>>,
}

impl<B: BlockContents<N> + 'static, const N: usize> PhaseLockInner<B, N> {
    /// Returns the public key for the leader of this round
    fn get_leader(&self, view: u64) -> PubKey {
        let index = view % u64::from(self.config.total_nodes);
        self.config.known_nodes[index as usize].clone()
    }
}

/// Thread safe, shared view of a `PhaseLock`
#[derive(Clone)]
pub struct PhaseLock<B: BlockContents<N> + Send + Sync + 'static, const N: usize> {
    /// Handle to internal phaselock implementation
    inner: Arc<PhaseLockInner<B, N>>,
}

impl<B: BlockContents<N> + Sync + Send + 'static, const N: usize> PhaseLock<B, N> {
    /// Creates a new phaselock with the given configuration options and sets it up with the given
    /// genesis block
    #[instrument(skip(genesis, priv_keys, starting_state, networking))]
    pub fn new(
        genesis: B,
        priv_keys: &tc::SecretKeySet,
        nonce: u64,
        config: PhaseLockConfig,
        starting_state: B::State,
        networking: impl NetworkingImplementation<Message<B, B::Transaction, N>> + 'static,
    ) -> Self {
        info!("Creating a new phaselock");
        let pub_key_set = priv_keys.public_keys();
        let node_priv_key = priv_keys.secret_key_share(nonce);
        let node_pub_key = node_priv_key.public_key_share();
        let genesis_hash = BlockContents::hash(&genesis);
        let t = config.thershold as usize;
        let inner = PhaseLockInner {
            public_key: PubKey {
                set: pub_key_set,
                node: node_pub_key,
                nonce,
            },
            private_key: PrivKey {
                node: node_priv_key,
            },
            genesis: genesis.clone(),
            config,
            networking: Box::new(networking),
            transaction_queue: RwLock::new(Vec::new()),
            state: RwLock::new(Arc::new(starting_state)),
            leaf_store: DashMap::new(),
            locked_qc: RwLock::new(Some(QuorumCertificate {
                hash: genesis_hash,
                view_number: 0,
                stage: Stage::Decide,
                signature: None,
                genesis: true,
            })),
            prepare_qc: RwLock::new(Some(QuorumCertificate {
                hash: genesis_hash,
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
            decision_cache: DashMap::new(),
        };
        inner.decision_cache.insert(
            genesis_hash,
            QuorumCertificate {
                hash: genesis_hash,
                view_number: 0,
                stage: Stage::Decide,
                signature: None,
                genesis: true,
            },
        );
        inner.leaf_store.insert(
            genesis_hash,
            Leaf {
                parent: [0_u8; { N }].into(),
                item: genesis,
            },
        );
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Returns true if the proposed leaf extends from the given block
    #[instrument(skip(self),fields(id = self.inner.public_key.nonce))]
    pub async fn extends_from(&self, leaf: &Leaf<B, N>, node: &BlockHash<N>) -> bool {
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
            let next_parent = self.inner.leaf_store.get(&parent);
            if let Some(next_parent) = next_parent {
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
    pub async fn safe_node(&self, leaf: &Leaf<B, N>, qc: &QuorumCertificate<N>) -> bool {
        if qc.genesis {
            info!("Safe node check bypassed due to genesis flag");
            return true;
        }
        if let Some(locked_qc) = self.inner.locked_qc.read().await.as_ref() {
            let extends_from = self.extends_from(leaf, &locked_qc.hash).await;
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
    #[instrument(skip(self),fields(id = self.inner.public_key.nonce),err)]
    pub async fn next_view(
        &self,
        current_view: u64,
        channel: Option<&broadcast::Sender<Event<B, B::State>>>,
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
        send_event::<B, B::State, { N }>(
            channel,
            Event {
                view_number: current_view,
                stage: Stage::None,
                event: EventType::NewView {
                    view_number: current_view,
                },
            },
        );
        Ok(())
    }

    /// Runs a single round of consensus
    ///
    /// # Panics
    ///
    /// Panics if consensus hits a bad round
    #[allow(clippy::too_many_lines)]
    #[instrument(skip(self),fields(id = self.inner.public_key.nonce),err)]
    pub async fn run_round(
        &self,
        current_view: u64,
        channel: Option<&broadcast::Sender<Event<B, B::State>>>,
    ) -> Result<()> {
        let phaselock = &self.inner;
        // Get the leader for the current round
        let leader = phaselock.get_leader(current_view);
        let is_leader = phaselock.public_key == leader;
        if is_leader {
            info!("Node is leader for current view");
        } else {
            info!("Node is follower for current view");
        }
        let state: Arc<B::State> = phaselock.state.read().await.clone();
        trace!("State copy made");
        /*
        Prepare phase
         */
        info!("Entering prepare phase");
        let the_block;
        let the_hash;
        if is_leader {
            // Prepare our block
            let mut block = B::next_block(&state);
            // spin while the transaction_queue is empty
            trace!("Entering spin while we wait for transactions");
            while phaselock.transaction_queue.read().await.is_empty() {
                trace!("executing transaction queue spin cycle");
                yield_now().await;
            }
            debug!("Unloading transactions");
            let mut transaction_queue = phaselock.transaction_queue.write().await;
            // Iterate through all the transactions, keeping the valid ones and discarding the
            // invalid ones
            for tx in transaction_queue.drain(..) {
                // Make sure the transaction is valid given the current state, otherwise, discard it
                let new_block = block.add_transaction(&state, &tx);
                if let Ok(new_block) = new_block {
                    block = new_block;
                    debug!(?tx, "Added transaction to block");
                } else {
                    warn!(?tx, "Invalid transaction rejected");
                }
            }
            // Wait until we have met the thershold of new-view messages
            debug!("Waiting for minimum number of new view messages to arrive");
            let new_views = phaselock.new_view_queue.wait().await;
            trace!("New view messages arrived");
            let high_qc = &new_views
                .iter()
                .max_by_key(|x| x.justify.view_number)
                .unwrap() // Unwrap can't fail, as we can't receive an empty Vec from waitqueue
                .justify;
            // Create the Leaf, and add it to the store
            let leaf = Leaf::new(block.clone(), high_qc.hash);
            phaselock.leaf_store.insert(leaf.hash(), leaf.clone());
            debug!(?leaf, "Leaf created and added to store");
            // Broadcast out the new leaf
            phaselock
                .networking
                .broadcast_message(Message::Prepare(Prepare {
                    current_view,
                    leaf: leaf.clone(),
                    high_qc: high_qc.clone(),
                }))
                .await
                .context(FailedToBroadcast {
                    stage: Stage::Prepare,
                })?;
            debug!("Leaf broadcasted to network");
            // Export the block
            the_block = block;
            the_hash = leaf.hash();
            send_event::<B, B::State, { N }>(
                channel,
                Event {
                    view_number: current_view,
                    stage: Stage::Prepare,
                    event: EventType::Propose {
                        block: Arc::new(the_block.clone()),
                    },
                },
            );
            // Make a prepare signature and send it to ourselves
            let signature =
                phaselock
                    .private_key
                    .partial_sign(&the_hash, Stage::Prepare, current_view);
            let vote = Vote {
                signature,
                leaf_hash: the_hash,
                id: phaselock.public_key.nonce,
                current_view,
            };
            phaselock.prepare_vote_queue.push(vote).await;
        } else {
            trace!("Waiting for prepare message to come in");
            // Wait for the leader to send us a prepare message
            let prepare = phaselock
                .prepare_waiter
                .wait_for(|x| x.current_view == current_view)
                .await;
            debug!(?prepare, "Prepare message received from leader");
            // Add the leaf to storage
            let leaf = prepare.leaf;
            let leaf_hash = leaf.hash();
            phaselock.leaf_store.insert(leaf_hash, leaf.clone());
            trace!(?leaf, "Leaf added to storage");
            // check that the message is safe, extends from the given qc, and is valid given the
            // current state
            let is_safe_node = self.safe_node(&leaf, &prepare.high_qc).await;
            if is_safe_node && leaf.item.validate_block(&state) {
                let signature =
                    phaselock
                        .private_key
                        .partial_sign(&leaf_hash, Stage::Prepare, current_view);
                let vote = Vote {
                    signature,
                    leaf_hash,
                    id: phaselock.public_key.nonce,
                    current_view,
                };
                let vote_message = Message::PrepareVote(vote);
                phaselock
                    .networking
                    .message_node(vote_message, leader.clone())
                    .await
                    .context(FailedToMessageLeader {
                        stage: Stage::Prepare,
                    })?;
                debug!("Prepare message successfully processed");
                the_block = leaf.item;
                the_hash = leaf_hash;
                send_event::<B, B::State, { N }>(
                    channel,
                    Event {
                        view_number: current_view,
                        stage: Stage::Prepare,
                        event: EventType::Propose {
                            block: Arc::new(the_block.clone()),
                        },
                    },
                );
            } else {
                error!("is_safe_node: {}", is_safe_node);
                error!(?leaf, "Leaf failed safe_node predicate");
                return Err(PhaseLockError::BadBlock {
                    stage: Stage::Prepare,
                });
            }
        }
        /*
        Pre-commit phase
         */
        info!("Entering pre-commit phase");
        if is_leader {
            // Collect the votes we have received from the nodes
            trace!("Waiting for threshold number of incoming votes to arrive");
            let mut vote_queue = phaselock
                .prepare_vote_queue
                .wait_for(|x| x.current_view == current_view)
                .await;
            debug!("Received threshold number of votes");
            let votes: Vec<_> = vote_queue
                .drain(..)
                .filter(|x| x.leaf_hash == the_hash)
                .map(|x| (x.id, x.signature))
                .collect();
            // Generate a quorum certificate from those votes
            let signature =
                generate_qc(votes.iter().map(|(x, y)| (*x, y)), &phaselock.public_key.set).map_err(
                    |e| PhaseLockError::FailedToAssembleQC {
                        stage: Stage::PreCommit,
                        source: e,
                    },
                )?;
            let qc = QuorumCertificate {
                hash: the_hash,
                signature: Some(signature),
                stage: Stage::Prepare,
                view_number: current_view,
                genesis: false,
            };
            debug!(?qc, "Pre-commit QC generated");
            // Store the pre-commit qc
            let mut pqc = phaselock.prepare_qc.write().await;
            trace!("Pre-commit qc stored in prepare_qc");
            *pqc = Some(qc.clone());
            let pc_message = Message::PreCommit(PreCommit {
                leaf_hash: the_hash,
                qc,
                current_view,
            });
            trace!("Precommit message packed, sending");
            phaselock
                .networking
                .broadcast_message(pc_message)
                .await
                .context(FailedToBroadcast {
                    stage: Stage::PreCommit,
                })?;
            debug!("Precommit message sent");
            // Make a pre commit vote and send it to ourselves
            let signature =
                phaselock
                    .private_key
                    .partial_sign(&the_hash, Stage::PreCommit, current_view);
            let vote_message = Vote {
                leaf_hash: the_hash,
                signature,
                id: phaselock.public_key.nonce,
                current_view,
            };
            phaselock.precommit_vote_queue.push(vote_message).await;
        } else {
            trace!("Waiting for precommit message to arrive from leader");
            // Wait for the leader to send us a precommit message
            let precommit = phaselock
                .precommit_waiter
                .wait_for(|x| x.current_view == current_view)
                .await;
            debug!(?precommit, "Received precommit message from leader");
            let prepare_qc = precommit.qc;
            if !(prepare_qc.verify(&phaselock.public_key.set, Stage::Prepare, current_view)
                && prepare_qc.hash == the_hash)
            {
                error!(?prepare_qc, "Bad or forged QC prepare_qc");
                return Err(PhaseLockError::BadOrForgedQC {
                    stage: Stage::PreCommit,
                    bad_qc: prepare_qc.to_vec_cert(),
                });
            }
            debug!("Precommit qc validated");
            let signature =
                phaselock
                    .private_key
                    .partial_sign(&the_hash, Stage::PreCommit, current_view);
            let vote_message = Message::PreCommitVote(Vote {
                leaf_hash: the_hash,
                signature,
                id: phaselock.public_key.nonce,
                current_view,
            });
            // store the prepare qc
            let mut pqc = phaselock.prepare_qc.write().await;
            *pqc = Some(prepare_qc);
            trace!("Prepare QC stored");
            phaselock
                .networking
                .message_node(vote_message, leader.clone())
                .await
                .context(FailedToMessageLeader {
                    stage: Stage::PreCommit,
                })?;
            debug!("Precommit vote sent");
        }
        /*
        Commit Phase
         */
        info!("Entering commit phase");
        if is_leader {
            trace!("Waiting for threshold of precommit votes to arrive");
            let mut vote_queue = phaselock
                .precommit_vote_queue
                .wait_for(|x| x.current_view == current_view)
                .await;
            debug!("Threshold of precommit votes recieved");
            let votes: Vec<_> = vote_queue
                .drain(..)
                .filter(|x| x.leaf_hash == the_hash)
                .map(|x| (x.id, x.signature))
                .collect();
            let signature =
                generate_qc(votes.iter().map(|(x, y)| (*x, y)), &phaselock.public_key.set).map_err(
                    |e| PhaseLockError::FailedToAssembleQC {
                        stage: Stage::Commit,
                        source: e,
                    },
                )?;
            let qc = QuorumCertificate {
                hash: the_hash,
                signature: Some(signature),
                stage: Stage::PreCommit,
                view_number: current_view,
                genesis: false,
            };
            debug!(?qc, "Commit QC generated");
            let c_message = Message::Commit(Commit {
                leaf_hash: the_hash,
                qc,
                current_view,
            });
            trace!(?c_message, "Commit message packed");
            phaselock
                .networking
                .broadcast_message(c_message)
                .await
                .context(FailedToBroadcast {
                    stage: Stage::Commit,
                })?;
            debug!("Commit message broadcasted");
            // Make a commit vote and send it to ourselves
            let signature =
                phaselock
                    .private_key
                    .partial_sign(&the_hash, Stage::Commit, current_view);
            let vote_message = Vote {
                leaf_hash: the_hash,
                signature,
                id: phaselock.public_key.nonce,
                current_view,
            };
            phaselock.commit_vote_queue.push(vote_message).await;
        } else {
            trace!("Waiting for commit message to arrive from leader");
            let commit = phaselock
                .commit_waiter
                .wait_for(|x| x.current_view == current_view)
                .await;
            debug!(?commit, "Received commit message from leader");
            let precommit_qc = commit.qc.clone();
            if !(precommit_qc.verify(&phaselock.public_key.set, Stage::PreCommit, current_view)
                && precommit_qc.hash == the_hash)
            {
                error!(?precommit_qc, "Bad or forged precommit qc");
                return Err(PhaseLockError::BadOrForgedQC {
                    stage: Stage::Commit,
                    bad_qc: precommit_qc.to_vec_cert(),
                });
            }
            let mut locked_qc = phaselock.locked_qc.write().await;
            trace!("precommit qc written to locked_qc");
            *locked_qc = Some(commit.qc);
            let signature =
                phaselock
                    .private_key
                    .partial_sign(&the_hash, Stage::Commit, current_view);
            let vote_message = Message::CommitVote(Vote {
                leaf_hash: the_hash,
                signature,
                id: phaselock.public_key.nonce,
                current_view,
            });
            trace!("Commit vote packed");
            phaselock
                .networking
                .message_node(vote_message, leader.clone())
                .await
                .context(FailedToMessageLeader {
                    stage: Stage::Commit,
                })?;
            debug!("Commit vote sent to leader");
        }
        /*
        Decide Phase
         */
        info!("Entering decide phase");
        if is_leader {
            trace!(
                ?current_view,
                "Waiting for threshold number of commit votes to arrive"
            );
            let mut vote_queue = phaselock
                .commit_vote_queue
                .wait_for(|x| x.current_view == current_view)
                .await;
            debug!("Received threshold number of commit votes");
            let votes: Vec<_> = vote_queue
                .drain(..)
                .filter(|x| x.leaf_hash == the_hash)
                .map(|x| (x.id, x.signature))
                .collect();
            let signature =
                generate_qc(votes.iter().map(|(x, y)| (*x, y)), &phaselock.public_key.set).map_err(
                    |e| PhaseLockError::FailedToAssembleQC {
                        stage: Stage::Decide,
                        source: e,
                    },
                )?;
            let qc = QuorumCertificate {
                hash: the_hash,
                signature: Some(signature),
                stage: Stage::Decide,
                view_number: current_view,
                genesis: false,
            };
            debug!(?qc, "Commit qc generated");
            // Add QC to decision cache
            phaselock.decision_cache.insert(the_hash, qc.clone());
            trace!("Commit qc added to decision cache");
            // Apply the state
            let new_state = the_block.append_to(&state).map_err(|error| {
                error!(
                    ?error,
                    ?the_block,
                    "Failed to append block to existing state"
                );
                PhaseLockError::InconsistentBlock {
                    stage: Stage::Decide,
                }
            })?;
            // set the new state
            let mut state = phaselock.state.write().await;
            *state = Arc::new(new_state);
            trace!("New state written");
            // Broadcast the decision
            let d_message = Message::Decide(Decide {
                leaf_hash: the_hash,
                qc,
                current_view,
            });
            phaselock
                .networking
                .broadcast_message(d_message)
                .await
                .context(FailedToBroadcast {
                    stage: Stage::Decide,
                })?;
            debug!("Decision broadcasted");
        } else {
            trace!(?current_view, "Waiting on decide QC to come in");
            let decide = phaselock
                .decide_waiter
                .wait_for(|x| x.current_view == current_view)
                .await;
            debug!(?decide, "Decision message arrived");
            let decide_qc = decide.qc.clone();
            if !(decide_qc.verify(&phaselock.public_key.set, Stage::Decide, current_view)
                && decide_qc.hash == the_hash)
            {
                error!(?decide_qc, "Bad or forged commit qc");
                return Err(PhaseLockError::BadOrForgedQC {
                    stage: Stage::Decide,
                    bad_qc: decide_qc.to_vec_cert(),
                });
            }
            // Apply new state
            trace!("Applying new state");
            let new_state = the_block.append_to(&state).map_err(|error| {
                error!(
                    ?error,
                    ?the_block,
                    "Failed to append block to existing state"
                );
                PhaseLockError::InconsistentBlock {
                    stage: Stage::Decide,
                }
            })?;
            // set the new state
            let mut state = phaselock.state.write().await;
            *state = Arc::new(new_state);
            debug!("New state set, round finished");
        }
        // Clear the transaction queue, temporary, need background task to clear already included
        // transactions
        send_event::<B, B::State, { N }>(
            channel,
            Event {
                view_number: current_view,
                stage: Stage::Decide,
                event: EventType::Decide {
                    block: Arc::new(the_block.clone()),
                    state: self.inner.state.read().await.clone(),
                },
            },
        );

        self.inner.transaction_queue.write().await.clear();
        trace!("Transaction queue cleared");
        Ok(())
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
                                Message::Prepare(p) => phaselock.prepare_waiter.put(p).await,
                                Message::PreCommit(pc) => phaselock.precommit_waiter.put(pc).await,
                                Message::Commit(c) => phaselock.commit_waiter.put(c).await,
                                Message::Decide(d) => phaselock.decide_waiter.put(d).await,
                                Message::SubmitTransaction(d) => {
                                    phaselock.transaction_queue.write().await.push(d)
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
                                    phaselock.prepare_vote_queue.push(pv).await
                                }
                                Message::PreCommitVote(pcv) => {
                                    phaselock.precommit_vote_queue.push(pcv).await
                                }
                                Message::CommitVote(cv) => {
                                    phaselock.commit_vote_queue.push(cv).await
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
    pub async fn publish_transaction_async(&self, tx: B::Transaction) -> Result<()> {
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
    pub async fn get_state(&self) -> Arc<B::State> {
        self.inner.state.read().await.clone()
    }

    /// Initializes a new phaselock and does the work of setting up all the background tasks
    ///
    /// Assumes networking implementation is already primed.
    ///
    /// Underlying `PhaseLock` instance starts out paused, and must be unpaused
    ///
    /// Upon encountering an unrecoverable error, such as a failure to send to a broadcast channel, the
    /// `PhaseLock` instance will log the error and shut down.
    pub async fn init(
        genesis: B,
        priv_keys: &tc::SecretKeySet,
        node_id: u64,
        config: PhaseLockConfig,
        starting_state: B::State,
        networking: impl NetworkingImplementation<Message<B, B::Transaction, N>> + 'static,
    ) -> (JoinHandle<()>, PhaseLockHandle<B, N>) {
        // TODO: Arbitrary channel capacity, investigate improving this
        let (input, output) = tokio::sync::broadcast::channel(128);
        let phaselock = Self::new(
            genesis,
            priv_keys,
            node_id,
            config.clone(),
            starting_state,
            networking,
        );
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
                        let x = channel.send(Event {
                            view_number: view,
                            stage: e.get_stage().unwrap_or(Stage::None),

                            event: EventType::Error { error: Arc::new(e) },
                        });
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
                        Ok(Ok(_)) => {
                            int_duration = default_interrupt_duration;
                        }
                        // If it errored, broadcast the error, reset the timeout, and continue
                        Ok(Err(e)) => {
                            let x = channel.send(Event {
                                view_number: view,
                                stage: e.get_stage().unwrap_or(Stage::None),
                                event: EventType::Error { error: Arc::new(e) },
                            });
                            if x.is_err() {
                                error!("All event streams closed! Shutting down.");
                                break;
                            }
                            continue;
                        }
                        // if we timed out, log it, send the event, and increase the timeout
                        Err(_) => {
                            warn!("Round timed out");
                            let x = channel.send(Event {
                                view_number: view,
                                stage: Stage::None,
                                event: EventType::ViewTimeout { view_number: view },
                            });
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

/// Sends an event over a `Some(broadcast::Sender<T>)`, does nothing otherwise
fn send_event<B, S, const N: usize>(
    channel: Option<&broadcast::Sender<Event<B, S>>>,
    event: Event<B, S>,
) where
    B: Send + Sync,
    S: Send + Sync,
{
    if let Some(c) = channel {
        let _result = c.send(event);
    }
}
