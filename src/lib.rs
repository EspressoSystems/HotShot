#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]
#![warn(clippy::missing_docs_in_private_items)]
#![allow(clippy::option_if_let_else)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::clippy::similar_names)]
// Temporary
#![allow(clippy::cast_possible_truncation)]
// Given that consensus should guarantee the ability to recover from errors, explicit panics should
// be strictly forbidden
#![warn(clippy::panic)]
//! Provides a generic rust implementation of the [`HotStuff`](https://arxiv.org/abs/1803.05069) BFT
//! protocol

/// Provides types useful for representing `HotStuff`'s data structures
pub mod data;
/// Contains integration test versions of various demos
pub mod demos;
/// Contains error types used by this library
pub mod error;
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

use async_std::sync::RwLock;
use dashmap::DashMap;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use snafu::ResultExt;
use threshold_crypto as tc;

use crate::data::Leaf;
use crate::error::{FailedToBroadcast, FailedToMessageLeader, HotStuffError, NetworkFault};
use crate::message::{
    Commit, CommitVote, Decide, Message, NewView, PreCommit, PreCommitVote, Prepare, PrepareVote,
};
use crate::networking::NetworkingImplementation;
use crate::utility::waitqueue::{WaitOnce, WaitQueue};
use tracing::{debug, error, info, info_span, instrument, trace, warn, Instrument};

pub use crate::data::{QuorumCertificate, Stage};

/// Convenience type alias
type Result<T> = std::result::Result<T, HotStuffError>;

/// The type used for block hashes
type BlockHash = [u8; 32];

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
    pub fn partial_sign(&self, hash: &BlockHash, _stage: Stage, _view: u64) -> tc::SignatureShare {
        self.node.sign(hash)
    }
}

/// The block trait
pub trait BlockContents:
    Serialize + DeserializeOwned + Clone + Debug + Hash + PartialEq + Eq + Send
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
    type Error: Error + Debug;

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
    fn hash(&self) -> BlockHash;
    /// Produces a hash for a transaction
    ///
    /// TODO: Abstract out into transaction trait
    fn hash_transaction(tx: &Self::Transaction) -> BlockHash;
}

/// Holds configuration for a hotstuff
#[derive(Debug)]
pub struct HotStuffConfig {
    /// Total number of nodes in the network
    pub total_nodes: u32,
    /// Nodes required to reach a decision
    pub thershold: u32,
    /// Maximum transactions per block
    pub max_transactions: usize,
    /// List of known node's public keys, including own, sorted by nonce
    pub known_nodes: Vec<PubKey>,
}

/// Holds the state needed to participate in `HotStuff` consensus
pub struct HotStuffInner<B: BlockContents + 'static> {
    /// The public key of this node
    public_key: PubKey,
    /// The private key of this node
    private_key: PrivKey,
    /// The genesis block, used for short-circuiting during bootstrap
    #[allow(dead_code)]
    genesis: B,
    /// Configuration items for this hotstuff instance
    config: HotStuffConfig,
    /// Networking interface for this hotstuff instance
    networking: Box<dyn NetworkingImplementation<Message<B, B::Transaction>>>,
    /// Pending transactions
    transaction_queue: RwLock<Vec<B::Transaction>>,
    /// Current state
    state: RwLock<B::State>,
    /// Block storage
    leaf_store: DashMap<BlockHash, Leaf<B>>,
    /// Current locked quorum certificate
    locked_qc: RwLock<Option<QuorumCertificate>>,
    /// Current prepare quorum certificate
    prepare_qc: RwLock<Option<QuorumCertificate>>,
    /// Unprocessed NextView messages
    new_view_queue: WaitQueue<NewView>,
    /// Unprocessed PrepareVote messages
    prepare_vote_queue: WaitQueue<PrepareVote>,
    /// Unprocessed PreCommit messages
    precommit_vote_queue: WaitQueue<PreCommitVote>,
    /// Unprocessed CommitVote messages
    commit_vote_queue: WaitQueue<CommitVote>,
    /// Currently pending Prepare message
    prepare_waiter: WaitOnce<Prepare<B>>,
    /// Currently pending precommit message
    precommit_waiter: WaitOnce<PreCommit>,
    /// Currently pending Commit message
    commit_waiter: WaitOnce<Commit>,
    /// Currently pending decide message
    decide_waiter: WaitOnce<Decide>,
    /// Map from a block's hash to its decision QC
    decision_cache: DashMap<BlockHash, QuorumCertificate>,
}

impl<B: BlockContents + 'static> HotStuffInner<B> {
    /// Returns the public key for the leader of this round
    fn get_leader(&self, view: u64) -> PubKey {
        let index = view % u64::from(self.config.total_nodes);
        self.config.known_nodes[index as usize].clone()
    }
}

/// Thread safe, shared view of a `HotStuff`
#[derive(Clone)]
pub struct HotStuff<B: BlockContents + Send + Sync + 'static> {
    /// Handle to internal hotstuff implementation
    inner: Arc<HotStuffInner<B>>,
}

impl<B: BlockContents + Sync + Send + 'static> HotStuff<B> {
    /// Creates a new hotstuff with the given configuration options and sets it up with the given
    /// genesis block
    #[instrument(skip(genesis, priv_keys, starting_state, networking))]
    pub fn new(
        genesis: B,
        priv_keys: &tc::SecretKeySet,
        nonce: u64,
        config: HotStuffConfig,
        starting_state: B::State,
        networking: impl NetworkingImplementation<Message<B, B::Transaction>> + 'static,
    ) -> Self {
        info!("Creating a new hotstuff");
        let pub_key_set = priv_keys.public_keys();
        let node_priv_key = priv_keys.secret_key_share(nonce);
        let node_pub_key = node_priv_key.public_key_share();
        let genesis_hash = BlockContents::hash(&genesis);
        let t = config.thershold as usize;
        let inner = HotStuffInner {
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
            state: RwLock::new(starting_state),
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
                parent: [0_u8; 32],
                item: genesis,
            },
        );
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Returns true if the proposed leaf extends from the given block
    #[instrument(skip(self),fields(id = self.inner.public_key.nonce))]
    pub async fn extends_from(&self, leaf: &Leaf<B>, node: &BlockHash) -> bool {
        let mut parent = leaf.parent;
        // Short circuit to enable blocks that don't have parents
        if &parent == node {
            trace!("leaf extends from node through short-circuit");
            return true;
        }
        while parent != [0_u8; 32] {
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
    pub async fn safe_node(&self, leaf: &Leaf<B>, qc: &QuorumCertificate) -> bool {
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
    pub async fn next_view(&self, current_view: u64) -> Result<()> {
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
            info!("Leader for this round");
        }
        Ok(())
    }

    /// Runs a single round of consensus
    ///
    /// # Panics
    ///
    /// Panics if consensus hits a bad round
    #[allow(clippy::too_many_lines)]
    #[instrument(skip(self),fields(id = self.inner.public_key.nonce),err)]
    pub async fn run_round(&self, current_view: u64) -> Result<()> {
        let hotstuff = &self.inner;
        // Get the leader for the current round
        let leader = hotstuff.get_leader(current_view);
        let is_leader = hotstuff.public_key == leader;
        if is_leader {
            info!("Node is leader for current view");
        } else {
            info!("Node is follower for current view");
        }
        let state: B::State = hotstuff.state.read().await.clone();
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
            while hotstuff.transaction_queue.read().await.is_empty() {
                async_std::task::yield_now().await;
            }
            debug!("Unloading transactions");
            let mut transaction_queue = hotstuff.transaction_queue.write().await;
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
            let new_views = hotstuff.new_view_queue.wait().await;
            trace!("New view messages arrived");
            let high_qc = &new_views
                .iter()
                .max_by_key(|x| x.justify.view_number)
                .unwrap() // Unwrap can't fail, as we can't receive an empty Vec from waitqueue
                .justify;
            // Create the Leaf, and add it to the store
            let leaf = Leaf::new(block.clone(), high_qc.hash);
            hotstuff.leaf_store.insert(leaf.hash(), leaf.clone());
            debug!(?leaf, "Leaf created and added to store");
            // Broadcast out the new leaf
            hotstuff
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
        } else {
            trace!("Waiting for prepare message to come in");
            // Wait for the leader to send us a prepare message
            let prepare = hotstuff
                .prepare_waiter
                .wait_for(|x| x.current_view == current_view)
                .await;
            debug!(?prepare, "Prepare message received from leader");
            // Add the leaf to storage
            let leaf = prepare.leaf;
            let leaf_hash = leaf.hash();
            hotstuff.leaf_store.insert(leaf_hash, leaf.clone());
            trace!(?leaf, "Leaf added to storage");
            // check that the message is safe, extends from the given qc, and is valid given the
            // current state
            if self.safe_node(&leaf, &prepare.high_qc).await && leaf.item.validate_block(&state) {
                let signature =
                    hotstuff
                        .private_key
                        .partial_sign(&leaf_hash, Stage::Prepare, current_view);
                let vote = PrepareVote {
                    signature,
                    leaf_hash,
                    id: hotstuff.public_key.nonce,
                };
                let vote_message = Message::PrepareVote(vote);
                hotstuff
                    .networking
                    .message_node(vote_message, leader.clone())
                    .await
                    .context(FailedToMessageLeader {
                        stage: Stage::Prepare,
                    })?;
                debug!("Prepare message successfully processed");
                the_block = leaf.item;
                the_hash = leaf_hash;
            } else {
                error!(?leaf, "Leaf failed safe_node predicate");
                return Err(HotStuffError::BadBlock {
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
            let mut vote_queue = hotstuff.prepare_vote_queue.wait().await;
            debug!("Received threshold number of votes");
            let votes: Vec<_> = vote_queue
                .drain(..)
                .filter(|x| x.leaf_hash == the_hash)
                .map(|x| (x.id, x.signature))
                .collect();
            // Generate a quorum certificate from those votes
            let signature =
                generate_qc(votes.iter().map(|(x, y)| (*x, y)), &hotstuff.public_key.set).map_err(
                    |e| HotStuffError::FailedToAssembleQC {
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
            let mut pqc = hotstuff.prepare_qc.write().await;
            trace!("Pre-commit qc stored in prepare_qc");
            *pqc = Some(qc.clone());
            let pc_message = Message::PreCommit(PreCommit {
                leaf_hash: the_hash,
                qc,
                current_view,
            });
            trace!("Precommit message packed, sending");
            hotstuff
                .networking
                .broadcast_message(pc_message)
                .await
                .context(FailedToBroadcast {
                    stage: Stage::PreCommit,
                })?;
            debug!("Precommit message sent");
        } else {
            trace!("Waiting for precommit message to arrive from leader");
            // Wait for the leader to send us a precommit message
            let precommit = hotstuff
                .precommit_waiter
                .wait_for(|x| x.current_view == current_view)
                .await;
            debug!(?precommit, "Received precommit message from leader");
            let prepare_qc = precommit.qc;
            if !(prepare_qc.verify(&hotstuff.public_key.set, Stage::Prepare, current_view)
                && prepare_qc.hash == the_hash)
            {
                error!(?prepare_qc, "Bad or forged QC prepare_qc");
                return Err(HotStuffError::BadOrForgedQC {
                    stage: Stage::PreCommit,
                    bad_qc: prepare_qc,
                });
            }
            debug!("Precommit qc validated");
            let signature =
                hotstuff
                    .private_key
                    .partial_sign(&the_hash, Stage::PreCommit, current_view);
            let vote_message = Message::PreCommitVote(PreCommitVote {
                leaf_hash: the_hash,
                signature,
                id: hotstuff.public_key.nonce,
            });
            // store the prepare qc
            let mut pqc = hotstuff.prepare_qc.write().await;
            *pqc = Some(prepare_qc);
            trace!("Prepare QC stored");
            hotstuff
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
            let mut vote_queue = hotstuff.precommit_vote_queue.wait().await;
            debug!("Threshold of precommit votes recieved");
            let votes: Vec<_> = vote_queue
                .drain(..)
                .filter(|x| x.leaf_hash == the_hash)
                .map(|x| (x.id, x.signature))
                .collect();
            let signature =
                generate_qc(votes.iter().map(|(x, y)| (*x, y)), &hotstuff.public_key.set).map_err(
                    |e| HotStuffError::FailedToAssembleQC {
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
            hotstuff
                .networking
                .broadcast_message(c_message)
                .await
                .context(FailedToBroadcast {
                    stage: Stage::Commit,
                })?;
            debug!("Commit message broadcasted");
        } else {
            trace!("Waiting for commit message to arrive from leader");
            let commit = hotstuff
                .commit_waiter
                .wait_for(|x| x.current_view == current_view)
                .await;
            debug!(?commit, "Received commit message from leader");
            let precommit_qc = commit.qc.clone();
            if !(precommit_qc.verify(&hotstuff.public_key.set, Stage::PreCommit, current_view)
                && precommit_qc.hash == the_hash)
            {
                error!(?precommit_qc, "Bad or forged precommit qc");
                return Err(HotStuffError::BadOrForgedQC {
                    stage: Stage::Commit,
                    bad_qc: precommit_qc,
                });
            }
            let mut locked_qc = hotstuff.locked_qc.write().await;
            trace!("precommit qc written to locked_qc");
            *locked_qc = Some(commit.qc);
            let signature =
                hotstuff
                    .private_key
                    .partial_sign(&the_hash, Stage::Commit, current_view);
            let vote_message = Message::CommitVote(CommitVote {
                leaf_hash: the_hash,
                signature,
                id: hotstuff.public_key.nonce,
            });
            trace!("Commit vote packed");
            hotstuff
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
            trace!("Waiting for threshold number of commit votes to arrive");
            let mut vote_queue = hotstuff.commit_vote_queue.wait().await;
            debug!("Received threshold number of commit votes");
            let votes: Vec<_> = vote_queue
                .drain(..)
                .filter(|x| x.leaf_hash == the_hash)
                .map(|x| (x.id, x.signature))
                .collect();
            let signature =
                generate_qc(votes.iter().map(|(x, y)| (*x, y)), &hotstuff.public_key.set).map_err(
                    |e| HotStuffError::FailedToAssembleQC {
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
            hotstuff.decision_cache.insert(the_hash, qc.clone());
            trace!("Commit qc added to decision cache");
            // Apply the state
            let new_state = the_block.append_to(&state).map_err(|error| {
                error!(
                    ?error,
                    ?the_block,
                    "Failed to append block to existing state"
                );
                HotStuffError::InconsistentBlock {
                    stage: Stage::Decide,
                }
            })?;
            // set the new state
            let mut state = hotstuff.state.write().await;
            *state = new_state;
            trace!("New state written");
            // Broadcast the decision
            let d_message = Message::Decide(Decide {
                leaf_hash: the_hash,
                qc,
                current_view,
            });
            hotstuff
                .networking
                .broadcast_message(d_message)
                .await
                .context(FailedToBroadcast {
                    stage: Stage::Decide,
                })?;
            debug!("Decision broadcasted");
        } else {
            trace!("Waiting on decide QC to come in");
            let decide = hotstuff
                .decide_waiter
                .wait_for(|x| x.current_view == current_view)
                .await;
            debug!(?decide, "Decision message arrived");
            let decide_qc = decide.qc.clone();
            if !(decide_qc.verify(&hotstuff.public_key.set, Stage::Decide, current_view)
                && decide_qc.hash == the_hash)
            {
                error!(?decide_qc, "Bad or forged commit qc");
                return Err(HotStuffError::BadOrForgedQC {
                    stage: Stage::Decide,
                    bad_qc: decide_qc,
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
                HotStuffError::InconsistentBlock {
                    stage: Stage::Decide,
                }
            })?;
            // set the new state
            let mut state = hotstuff.state.write().await;
            *state = new_state;
            debug!("New state set, round finished");
        }
        // Clear the transaction queue, temporary, need background task to clear already included
        // transactions

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
        async_std::task::spawn(
            async move {
                info!("Launching broadcast processing task");
                let networking = &x.inner.networking;
                let hotstuff = &x.inner;
                while let Ok(queue) = networking.broadcast_queue().await {
                    debug!(?queue, "Processing messages");
                    if queue.is_empty() {
                        trace!("No message, yeilding");
                        async_std::task::yield_now().await;
                    } else {
                        for item in queue {
                            trace!(?item, "Processing item");
                            match item {
                                Message::Prepare(p) => hotstuff.prepare_waiter.put(p).await,
                                Message::PreCommit(pc) => hotstuff.precommit_waiter.put(pc).await,
                                Message::Commit(c) => hotstuff.commit_waiter.put(c).await,
                                Message::Decide(d) => hotstuff.decide_waiter.put(d).await,
                                Message::SubmitTransaction(d) => {
                                    hotstuff.transaction_queue.write().await.push(d)
                                }
                                _ => {
                                    // Log the exceptional situation and proceed
                                    warn!(?item, "Direct message received over broadcast channel");
                                }
                            }
                        }
                        trace!("Item processed, yeilding");
                        async_std::task::yield_now().await;
                    }
                }
            }
            .instrument(info_span!(
                "Hotstuff Broadcast Task",
                id = self.inner.public_key.nonce
            )),
        );
        let x = self.clone();
        // Spawn direct processing task
        async_std::task::spawn(
            async move {
                info!("Launching direct processing task");
                let hotstuff = &x.inner;
                let networking = &x.inner.networking;
                while let Ok(queue) = networking.direct_queue().await {
                    debug!(?queue, "Processing messages");
                    if queue.is_empty() {
                        trace!("No message, yeilding");
                        async_std::task::yield_now().await;
                    } else {
                        for item in queue {
                            trace!(?item, "Processing item");
                            match item {
                                Message::NewView(nv) => hotstuff.new_view_queue.push(nv).await,
                                Message::PrepareVote(pv) => {
                                    hotstuff.prepare_vote_queue.push(pv).await
                                }
                                Message::PreCommitVote(pcv) => {
                                    hotstuff.precommit_vote_queue.push(pcv).await
                                }
                                Message::CommitVote(cv) => {
                                    hotstuff.commit_vote_queue.push(cv).await
                                }
                                _ => {
                                    // Log exceptional situation and proceed
                                    warn!(?item, "Broadcast message received over direct channel");
                                }
                            }
                        }
                        trace!("Item processed, yeilding");
                        async_std::task::yield_now().await;
                    }
                }
            }
            .instrument(info_span!(
                "Hotstuff Direct Task",
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
    pub async fn get_state(&self) -> B::State {
        self.inner.state.read().await.clone()
    }
}

/// Attempts to generate a quorum certificate from the provided signatures
fn generate_qc<'a>(
    signatures: impl IntoIterator<Item = (u64, &'a tc::SignatureShare)>,
    key_set: &tc::PublicKeySet,
) -> std::result::Result<tc::Signature, tc::error::Error> {
    key_set.combine_signatures(signatures)
}
