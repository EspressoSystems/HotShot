#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]
#![warn(clippy::clippy::missing_docs_in_private_items)]
#![allow(dead_code)] // Temporary
#![allow(clippy::unused_self)] // Temporary
#![allow(unreachable_code)] // Temporary
//! Provides a generic rust implementation of the [HotStuff](https://arxiv.org/abs/1803.05069) BFT protocol

mod data;
mod demos;
mod error;
mod message;
mod networking;
mod replica;
mod utility;

use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

use async_std::sync::RwLock;
use dashmap::DashMap;
use parking_lot::Mutex;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use snafu::ResultExt;
use threshold_crypto as tc;

use crate::data::*;
use crate::error::*;
use crate::message::*;
use crate::networking::*;
use crate::utility::waitqueue::{WaitOnce, WaitQueue};

type Result<T> = std::result::Result<T, HotStuffError>;

/// The type used for block hashes
type BlockHash = [u8; 32];

/// Public key type
///
/// Opaque wrapper around threshold_crypto key
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PubKey {
    set: tc::PublicKeySet,
    node: tc::PublicKeyShare,
    /// The portion of the KeyShare this node holds
    pub nonce: u64,
}

impl PubKey {
    /// Testing only random key generation
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

/// Private key stub type
///
/// Opaque wrapper around threshold_crypto key
#[derive(Clone, Debug)]
pub struct PrivKey {
    node: tc::SecretKeyShare,
}

impl PrivKey {
    /// Uses this private key to produce a partial signature for the given block hash
    pub fn partial_sign(&self, hash: &BlockHash, _stage: Stage, _view: u64) -> tc::SignatureShare {
        self.node.sign(hash)
    }
}

/// The block trait
pub trait BlockContents:
    Serialize + DeserializeOwned + Clone + Debug + Hash + Default + PartialEq + Eq + Send
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
    type Error;

    /// Attempts to add a transaction, returning an Error if not compatible with the current state
    fn add_transaction(
        &self,
        state: &Self::State,
        tx: &Self::Transaction,
    ) -> std::result::Result<Self, Self::Error>;
    /// ensures that the block is append able to the current state
    fn validate_block(&self, state: &Self::State) -> bool;
    /// Appends the block to the state
    fn append_to(&self, state: &Self::State) -> std::result::Result<Self::State, Self::Error>;
    /// Produces a hash for the contents of the block
    fn hash(&self) -> BlockHash;
    /// Produces a hash for a transaction
    ///
    /// TODO: Abstract out into transaction trait
    fn hash_transaction(tx: &Self::Transaction) -> BlockHash;
}

/// Type alias for a mutexed, shared owernship block
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct BlockRef<B>(pub Arc<Mutex<Block<B>>>);

impl<B: PartialEq> PartialEq for BlockRef<B> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || &*self.0.lock() == &*other.0.lock()
    }
}

impl<B: Eq> Eq for BlockRef<B> {}

/// Block struct
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Block<B> {
    contents: B,
    qc_ref: Option<BlockRef<B>>,
    extra: Vec<u8>,
    height: u64,
    delivered: bool,
    decision: bool,
}

impl<B: BlockContents> Block<B> {}

/// The type used for quorum certs
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct QuorumCertificate {
    hash: BlockHash,
    view_number: u64,
    stage: Stage,
    signature: Option<tc::Signature>,
    /// Temporary bypass for boostrapping
    genesis: bool,
}

impl QuorumCertificate {
    /// Verifies a quorum certificate
    pub fn verify(&self, key: &tc::PublicKeySet, stage: Stage, view: u64) -> bool {
        // Temporary, stage and view should be included in signature in future
        if let Some(signature) = &self.signature {
            key.public_key().verify(&signature, &self.hash)
                && self.stage == stage
                && self.view_number == view
        } else {
            self.genesis
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
/// Represents the stages of consensus
pub enum Stage {
    /// Prepare Phase
    Prepare,
    /// PreCommit Phase
    PreCommit,
    /// Commit Phase
    Commit,
    /// Decide Phase
    Decide,
}

/// Holds configuration for a hotstuff
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

/// Holds the state needed to participate in HotStuff consensus
pub struct HotStuffInner<B: BlockContents + 'static> {
    /// The public key of this node
    public_key: PubKey,
    /// The private key of this node
    private_key: PrivKey,
    /// The genesis block, used for short-circuiting during bootstrap
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
        let index = view % self.config.total_nodes as u64;
        self.config.known_nodes[index as usize].clone()
    }
}

/// Thread safe, shared view of a HotStuff
#[derive(Clone)]
pub struct HotStuff<B: BlockContents + Send + Sync + 'static> {
    inner: Arc<HotStuffInner<B>>,
}

impl<B: BlockContents + Sync + Send + 'static> HotStuff<B> {
    /// Creates a new hotstuff with the given configuration options and sets it up with the given
    /// genesis block
    pub fn new(
        genesis: B,
        priv_keys: &tc::SecretKeySet,
        nonce: u64,
        config: HotStuffConfig,
        starting_state: B::State,
        networking: impl NetworkingImplementation<Message<B, B::Transaction>> + 'static,
    ) -> Self {
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
    pub async fn extends_from(&self, leaf: &Leaf<B>, node: &BlockHash) -> bool {
        let mut parent = leaf.parent.clone();
        // Short circuit to enable blocks that don't have parents
        if &parent == node {
            return true;
        }
        while parent != [0_u8; 32] {
            if &parent == node {
                return true;
            }
            let next_parent = self.inner.leaf_store.get(&parent);
            if let Some(next_parent) = next_parent {
                parent = next_parent.parent.clone();
            } else {
                return false;
            }
        }
        return true;
    }

    /// Returns true if a proposed leaf satisfies the safety rule
    pub async fn safe_node(&self, leaf: &Leaf<B>, qc: &QuorumCertificate) -> bool {
        if qc.genesis {
            return true;
        }
        if let Some(locked_qc) = self.inner.locked_qc.read().await.as_ref() {
            let extends_from = self.extends_from(leaf, &locked_qc.hash).await;
            let view_number = qc.view_number > locked_qc.view_number;
            // println!(
            //     "extends from: {} view number: {} ({},{})",
            //     extends_from, view_number, qc.view_number, locked_qc.view_number
            // );
            extends_from || view_number
        } else {
            false
        }
    }

    /// Sends out the next view message
    pub async fn next_view(&self, current_view: u64) -> Result<()> {
        let new_leader = self.inner.get_leader(current_view + 1);
        // If we are the new leader, do nothing
        if !(new_leader == self.inner.public_key) {
            let view_message = Message::NewView(NewView {
                current_view,
                justify: self.inner.prepare_qc.read().await.as_ref().unwrap().clone(),
            });
            self.inner
                .networking
                .message_node(view_message, new_leader)
                .await
                .context(NetworkFault)?;
        }
        Ok(())
    }

    /// Runs a single round of consensus    
    pub async fn run_round(&self, current_view: u64) {
        let hotstuff = &self.inner;
        // Get the leader for the current round
        let leader = hotstuff.get_leader(current_view);
        let is_leader = hotstuff.public_key == leader;
        let state: B::State = hotstuff.state.read().await.clone();
        /*
        Prepare phase
         */
        let the_block;
        let the_hash;
        //        println!("Prepare");
        if is_leader {
            // Prepare our block
            let mut block = B::default();
            // spin while the transaction_queue is empty
            while hotstuff.transaction_queue.read().await.is_empty() {
                async_std::task::yield_now().await;
            }
            let mut transaction_queue = hotstuff.transaction_queue.write().await;
            //            println!("Transaction Queue: {:?}", transaction_queue);
            // Iterate through all the transactions, keeping the valid ones and discarding the
            // invalid ones
            for tx in transaction_queue.drain(..) {
                // Make sure the transaction is valid given the current state, otherwise, discard it
                let new_block = block.add_transaction(&state, &tx);
                if let Ok(new_block) = new_block {
                    block = new_block;
                }
            }
            // Wait until we have met the thershold of new-view messages
            let new_views = hotstuff.new_view_queue.wait().await;
            let high_qc = &new_views
                .iter()
                .max_by_key(|x| x.justify.view_number)
                .unwrap() // Unwrap can't fail, as we can't receive an empty Vec from waitqueue
                .justify;
            // Create the Leaf, and add it to the store
            let leaf = Leaf::new(block.clone(), high_qc.hash);
            hotstuff.leaf_store.insert(leaf.hash(), leaf.clone());
            // Broadcast out the new leaf
            hotstuff
                .networking
                .broadcast_message(Message::Prepare(Prepare {
                    current_view,
                    leaf: leaf.clone(),
                    high_qc: high_qc.clone(),
                }))
                .await
                .expect("Failed to broadcast message");
            // Export the block
            the_block = block;
            the_hash = leaf.hash();
        } else {
            // Wait for the leader to send us a prepare message
            let prepare = hotstuff
                .prepare_waiter
                .wait_for(|x| x.current_view == current_view)
                .await;
            // Add the leaf to storage
            let leaf = prepare.leaf;
            let leaf_hash = leaf.hash();
            hotstuff.leaf_store.insert(leaf_hash.clone(), leaf.clone());
            // check that the message is safe, extends from the given qc, and is valid given the
            // current state
            if self.safe_node(&leaf, &prepare.high_qc).await && leaf.item.validate_block(&state) {
                let signature =
                    hotstuff
                        .private_key
                        .partial_sign(&leaf_hash, Stage::Prepare, current_view);
                let vote = PrepareVote {
                    signature,
                    leaf_hash: leaf_hash.clone(),
                    id: hotstuff.public_key.nonce,
                };
                let vote_message = Message::PrepareVote(vote);
                hotstuff
                    .networking
                    .message_node(vote_message, leader.clone())
                    .await
                    .expect(&format!(
                        "Failed to message leader in prepare phase of view {}",
                        current_view
                    ));
                the_block = leaf.item;
                the_hash = leaf_hash;
            } else {
                panic!("Bad block in prepare phase of view {}", current_view);
            }
        }
        /*
        Pre-commit phase
         */
        //        println!("Precommit");
        if is_leader {
            // Collect the votes we have received from the nodes
            let mut vote_queue = hotstuff.prepare_vote_queue.wait().await;
            let votes: Vec<_> = vote_queue
                .drain(..)
                .filter(|x| x.leaf_hash == the_hash)
                .map(|x| (x.id, x.signature))
                .collect();
            // Generate a quorum certificate from those votes
            let signature =
                generate_qc(votes.iter().map(|(x, y)| (*x, y)), &hotstuff.public_key.set).expect(
                    &format!(
                        "Failed to generate QC in pre-commit phase of view {}",
                        current_view
                    ),
                );
            let qc = QuorumCertificate {
                hash: the_hash,
                signature: Some(signature),
                stage: Stage::Prepare,
                view_number: current_view,
                genesis: false,
            };
            // Store the pre-commit qc
            let mut pqc = hotstuff.prepare_qc.write().await;
            *pqc = Some(qc.clone());
            let pc_message = Message::PreCommit(PreCommit {
                leaf_hash: the_hash,
                qc,
                current_view,
            });
            hotstuff
                .networking
                .broadcast_message(pc_message)
                .await
                .expect("Failed to broadcast message");
        } else {
            // Wait for the leader to send us a precommit message
            let precommit = hotstuff
                .precommit_waiter
                .wait_for(|x| x.current_view == current_view)
                .await;
            let prepare_qc = precommit.qc;
            if !(prepare_qc.verify(&hotstuff.public_key.set, Stage::Prepare, current_view)
                && prepare_qc.hash == the_hash)
            {
                panic!(
                    "Bad or forged qc in precommit phase of view {}",
                    current_view
                );
            }
            let signature =
                hotstuff
                    .private_key
                    .partial_sign(&the_hash, Stage::PreCommit, current_view);
            let vote_message = Message::PreCommitVote(PreCommitVote {
                leaf_hash: the_hash.clone(),
                signature,
                id: hotstuff.public_key.nonce,
            });
            // store the prepare qc
            let mut pqc = hotstuff.prepare_qc.write().await;
            *pqc = Some(prepare_qc);
            hotstuff
                .networking
                .message_node(vote_message, leader.clone())
                .await
                .expect(&format!(
                    "Failed to message leader in prepare phase of view {}",
                    current_view
                ));
        }
        /*
        Commit Phase
         */
        if is_leader {
            //            println!("Commit Leader");
            let mut vote_queue = hotstuff.precommit_vote_queue.wait().await;
            let votes: Vec<_> = vote_queue
                .drain(..)
                .filter(|x| x.leaf_hash == the_hash)
                .map(|x| (x.id, x.signature))
                .collect();
            //            println!("leader has votes for commit");
            let signature =
                generate_qc(votes.iter().map(|(x, y)| (*x, y)), &hotstuff.public_key.set).expect(
                    &format!(
                        "Failed to generate QC in commit phase of view {}",
                        current_view
                    ),
                );
            let qc = QuorumCertificate {
                hash: the_hash,
                signature: Some(signature),
                stage: Stage::PreCommit,
                view_number: current_view,
                genesis: false,
            };
            let c_message = Message::Commit(Commit {
                leaf_hash: the_hash,
                qc,
                current_view,
            });
            hotstuff
                .networking
                .broadcast_message(c_message)
                .await
                .expect("Failed to broadcast message");
        } else {
            //            println!("Commit Follower");
            let commit = hotstuff
                .commit_waiter
                .wait_for(|x| x.current_view == current_view)
                .await;
            //            println!("follower has precommit qc");
            let precommit_qc = commit.qc.clone();
            if !(precommit_qc.verify(&hotstuff.public_key.set, Stage::PreCommit, current_view)
                && precommit_qc.hash == the_hash)
            {
                panic!("Bad or forged qc in commit phase of view {}", current_view);
            }
            let mut locked_qc = hotstuff.locked_qc.write().await;
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
            hotstuff
                .networking
                .message_node(vote_message, leader.clone())
                .await
                .expect(&format!(
                    "Failed to message leader in commit phase of view {}",
                    current_view
                ));
        }
        /*
        Decide Phase
         */
        //        println!("Decide");
        if is_leader {
            let mut vote_queue = hotstuff.commit_vote_queue.wait().await;
            let votes: Vec<_> = vote_queue
                .drain(..)
                .filter(|x| x.leaf_hash == the_hash)
                .map(|x| (x.id, x.signature))
                .collect();
            let signature =
                generate_qc(votes.iter().map(|(x, y)| (*x, y)), &hotstuff.public_key.set).expect(
                    &format!(
                        "Failed to generate QC in decide phase of view {}",
                        current_view
                    ),
                );
            let qc = QuorumCertificate {
                hash: the_hash,
                signature: Some(signature),
                stage: Stage::Decide,
                view_number: current_view,
                genesis: false,
            };
            // Add QC to decision cache
            hotstuff.decision_cache.insert(the_hash, qc.clone());
            // Apply the state
            let new_state = the_block.append_to(&state);
            if new_state.is_err() {
                panic!(
                    "Failed to append new block to existing state in view {}",
                    current_view,
                );
            }
            // hack to workaround blockcontents not having a debug bound
            let new_state = new_state.unwrap_or_else(|_| unreachable!());
            // set the new state
            let mut state = hotstuff.state.write().await;
            *state = new_state;
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
                .expect("Failed to broadcast message");
        } else {
            let decide = hotstuff
                .decide_waiter
                .wait_for(|x| x.current_view == current_view)
                .await;
            let decide_qc = decide.qc.clone();
            // println!("Decide QC: {:?}", decide_qc);
            // println!("The hash: {:?}", the_hash);
            // println!(
            //     "Decide verify: {}",
            //     decide_qc.verify(&hotstuff.public_key.set, Stage::Decide, current_view)
            // );
            if !(decide_qc.verify(&hotstuff.public_key.set, Stage::Decide, current_view)
                && decide_qc.hash == the_hash)
            {
                panic!("Bad or forged qc in decide phase of view {}", current_view);
            }
            // Apply new state
            //            println!("The block: {:?}", the_block);
            let new_state = the_block.append_to(&state);
            if new_state.is_err() {
                panic!(
                    "Failed to append new block to existing state in view {}",
                    current_view,
                );
            }
            // hack to workaround blockcontents not having a debug bound
            let new_state = new_state.unwrap_or_else(|_| unreachable!());
            // set the new state
            let mut state = hotstuff.state.write().await;
            *state = new_state;
        }
        // Clear the transaction queue, temporary, need background task to clear already included
        // transactions

        self.inner.transaction_queue.write().await.clear();
    }

    /// Spawns the background tasks for network processing for this instance
    pub async fn spawn_networking_tasks(&self) {
        let x = self.clone();
        // Spawn broadcast processing task
        async_std::task::spawn(async move {
            let networking = &x.inner.networking;
            let hotstuff = &x.inner;
            while let Ok(queue) = networking.broadcast_queue().await {
                if queue.is_empty() {
                    async_std::task::yield_now().await;
                } else {
                    for item in queue {
                        match item {
                            Message::Prepare(p) => hotstuff.prepare_waiter.put(p).await,
                            Message::PreCommit(pc) => hotstuff.precommit_waiter.put(pc).await,
                            Message::Commit(c) => hotstuff.commit_waiter.put(c).await,
                            Message::Decide(d) => hotstuff.decide_waiter.put(d).await,
                            Message::SubmitTransaction(d) => {
                                hotstuff.transaction_queue.write().await.push(d)
                            }
                            _ => panic!("Non-broadcast transaction sent over broadcast"),
                        }
                    }
                    async_std::task::yield_now().await;
                }
            }
        });
        let x = self.clone();
        // Spawn direct processing task
        async_std::task::spawn(async move {
            let hotstuff = &x.inner;
            let networking = &x.inner.networking;
            while let Ok(queue) = networking.direct_queue().await {
                if queue.is_empty() {
                    async_std::task::yield_now().await;
                } else {
                    for item in queue {
                        match item {
                            Message::NewView(nv) => hotstuff.new_view_queue.push(nv).await,
                            Message::PrepareVote(pv) => hotstuff.prepare_vote_queue.push(pv).await,
                            Message::PreCommitVote(pcv) => {
                                hotstuff.precommit_vote_queue.push(pcv).await
                            }
                            Message::CommitVote(cv) => hotstuff.commit_vote_queue.push(cv).await,
                            _ => panic!("Broadcast transaction sent over non-broadcast"),
                        }
                    }
                }
            }
        });
    }

    /// Publishes a transaction to the network
    pub async fn publish_transaction_async(&self, tx: B::Transaction) -> Result<()> {
        // Add the transaction to our own queue first
        self.inner.transaction_queue.write().await.push(tx.clone());
        // Wrap up a message
        let message = Message::SubmitTransaction(tx);
        self.inner
            .networking
            .broadcast_message(message)
            .await
            .context(NetworkFault)?;
        Ok(())
    }

    /// Returns a copy of the state
    pub async fn get_state(&self) -> B::State {
        self.inner.state.read().await.clone()
    }
}

fn generate_qc<'a>(
    signatures: impl IntoIterator<Item = (u64, &'a tc::SignatureShare)>,
    key_set: &tc::PublicKeySet,
) -> Option<tc::Signature> {
    key_set.combine_signatures(signatures).ok()
}
