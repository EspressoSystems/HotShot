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
    // overall: tc::PublicKey,
    // node: tc::PublicKeyShare,
    /// u64 nonce used for sorting
    ///
    /// Used for the leader election kludge
    pub nonce: u64,
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
    node: tc::SecretKey,
}

impl PrivKey {
    pub fn partial_sign(&self, hash: &BlockHash, stage: Stage, view: u64) -> tc::SignatureShare {
        todo!()
    }
}

/// The block trait
pub trait BlockContents:
    Serialize + DeserializeOwned + Clone + Debug + Hash + Default + PartialEq + Eq + Send
{
    /// The type of the state machine we are applying transitions to
    type State: Clone + Send;
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
    signature: tc::Signature,
}

impl QuorumCertificate {
    pub fn verify(&self, stage: Stage, view: u64) -> bool {
        todo!()
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

pub struct HotStuffConfig {
    total_nodes: u32,
    thershold: u32,
    max_transactions: usize,
}

/// Holds the state needed to participate in HotStuff consensus
pub struct HotStuffInner<B: BlockContents + 'static> {
    public_key: PubKey,
    private_key: PrivKey,
    genesis: B,
    config: RwLock<HotStuffConfig>,
    networking: Box<dyn NetworkingImplementation<Message<B>>>,
    transaction_queue: RwLock<Vec<B::Transaction>>,
    state: RwLock<B::State>,
    leaf_store: DashMap<BlockHash, Leaf<B>>,
    locked_qc: RwLock<Option<QuorumCertificate>>,
    prepare_qc: RwLock<Option<QuorumCertificate>>,
    new_view_queue: WaitQueue<NewView>,
    prepare_vote_queue: RwLock<Vec<PrepareVote>>,
    precommit_vote_queue: RwLock<Vec<PreCommitVote>>,
    commit_vote_queue: RwLock<Vec<CommitVote>>,
    prepare_waiter: WaitOnce<Prepare<B>>,
    precommit_waiter: WaitOnce<PreCommit>,
    commit_waiter: WaitOnce<Commit>,
    decide_waiter: WaitOnce<Decide>,
    decision_cache: DashMap<BlockHash, QuorumCertificate>,
}

impl<B: BlockContents + 'static> HotStuffInner<B> {
    /// Returns the public key for the leader of this round
    fn get_leader(&self, view: u64) -> PubKey {
        todo!()
    }
}

/// Thread safe, shared view of a HotStuff
#[derive(Clone)]
pub struct HotStuff<B: BlockContents + 'static> {
    inner: Arc<HotStuffInner<B>>,
}

impl<B: BlockContents + 'static> HotStuff<B> {
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
    pub async fn safe_node(&self, leaf: &Leaf<B>, qc: &QuorumCertificate) -> bool {
        if let Some(locked_qc) = self.inner.locked_qc.read().await.as_ref() {
            self.extends_from(leaf, &locked_qc.hash).await && qc.view_number > locked_qc.view_number
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
        let hotstuff_outer = self.clone();
        let consensus = async move {
            let hotstuff = &hotstuff_outer.inner;
            // Get the leader for the current round
            let leader = hotstuff.get_leader(current_view);
            let is_leader = hotstuff.public_key == leader;
            let mut state: B::State = hotstuff.state.read().await.clone();
            /*
            Prepare phase
             */
            let the_block;
            let the_hash;
            if is_leader {
                // Prepare our block
                let mut block = B::default();
                let mut transaction_queue = hotstuff.transaction_queue.write().await;
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
                if self.safe_node(&leaf, &prepare.high_qc).await && leaf.item.validate_block(&state)
                {
                    let signature =
                        hotstuff
                            .private_key
                            .partial_sign(&leaf_hash, Stage::Prepare, current_view);
                    let vote = PrepareVote {
                        signature,
                        leaf_hash: leaf_hash.clone(),
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
            if is_leader {
                // Collect the votes we have received from the nodes
                let mut vote_queue = hotstuff.prepare_vote_queue.write().await;
                let votes = vote_queue
                    .drain(..)
                    .filter(|x| x.leaf_hash == the_hash)
                    .map(|x| x.signature);
                // Generate a quorum certificate from those votes
                let signature = generate_qc(votes, &hotstuff.private_key).expect(&format!(
                    "Failed to generate QC in pre-commit phase of view {}",
                    current_view
                ));
                let qc = QuorumCertificate {
                    hash: the_hash,
                    signature,
                    stage: Stage::Prepare,
                    view_number: current_view,
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
                if !(prepare_qc.verify(Stage::Prepare, current_view) && prepare_qc.hash == the_hash)
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
                let mut vote_queue = hotstuff.prepare_vote_queue.write().await;
                let votes = vote_queue
                    .drain(..)
                    .filter(|x| x.leaf_hash == the_hash)
                    .map(|x| x.signature);
                let signature = generate_qc(votes, &hotstuff.private_key).expect(&format!(
                    "Failed to generate QC in commit phase of view {}",
                    current_view
                ));
                let qc = QuorumCertificate {
                    hash: the_hash,
                    signature,
                    stage: Stage::PreCommit,
                    view_number: current_view,
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
                let commit = hotstuff
                    .commit_waiter
                    .wait_for(|x| x.current_view == current_view)
                    .await;
                let precommit_qc = commit.qc.clone();
                if !(precommit_qc.verify(Stage::PreCommit, current_view)
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
            if is_leader {
                let mut vote_queue = hotstuff.commit_vote_queue.write().await;
                let votes = vote_queue
                    .drain(..)
                    .filter(|x| x.leaf_hash == the_hash)
                    .map(|x| x.signature);
                let signature = generate_qc(votes, &hotstuff.private_key).expect(&format!(
                    "Failed to generate QC in decide phase of view {}",
                    current_view
                ));
                let qc = QuorumCertificate {
                    hash: the_hash,
                    signature,
                    stage: Stage::Decide,
                    view_number: current_view,
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
                if !(decide_qc.verify(Stage::Commit, current_view) && decide_qc.hash == the_hash) {
                    panic!("Bad or forged qc in decide phase of view {}", current_view);
                }
                // Apply new state
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
        };
    }
}

fn generate_qc(
    signatures: impl IntoIterator<Item = tc::SignatureShare>,
    key: &PrivKey,
) -> Option<tc::Signature> {
    todo!()
}
