#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]
#![warn(clippy::clippy::missing_docs_in_private_items)]
#![allow(dead_code)] // Temporary
#![allow(clippy::unused_self)] // Temporary
#![allow(unreachable_code)] // Temporary
//! Provides a generic rust implementation of the [HotStuff](https://arxiv.org/abs/1803.05069) BFT protocol

mod error;
mod message;
mod networking;
mod replica;

use std::collections::{HashMap, VecDeque};
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

use futures_channel::oneshot::Receiver;
use futures_lite::{future, FutureExt};
use parking_lot::Mutex;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use snafu::{OptionExt, ResultExt};
use threshold_crypto as tc;

use crate::error::*;
use crate::message::*;
use crate::networking::*;
use crate::replica::*;

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
    parent_hashes: Vec<BlockHash>,
    parents: Vec<BlockRef<B>>,
    quorum_certificate: Option<QuorumCertificate>,
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
    signature: tc::Signature,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
/// Represents the stages of consensus
pub enum Stage {
    /// PreCommit Phase
    PreCommit,
    /// Commit Phase
    Commit,
    /// Decide Phase
    Decide,
}

/// Holds the state needed to participate in HotStuff consensus
pub struct HotStuffInner<B: BlockContents + 'static> {
    public_key: PubKey,
    genesis: B,
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
    /// Main run action
    ///
    /// launches the background task
    pub async fn run_consensus(&self) {
        let hotstuff_outer = self.clone();
        let consensus = async move {
            let hotstuff = &hotstuff_outer.inner;
            for current_view in 0.. {
                // Get the leader for the current round
                let leader = hotstuff.get_leader(current_view);
                let is_leader = hotstuff.public_key == leader;
                // Pre-commit
            }
        };
    }
}
