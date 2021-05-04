#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]
#![warn(clippy::clippy::missing_docs_in_private_items)]
#![allow(dead_code)] // Temporary
#![allow(clippy::unused_self)] // Temporary
//! Provides a generic rust implementation of the [HotStuff](https://arxiv.org/abs/1803.05069) BFT protocol

mod error;
mod message;
mod networking;
mod replica;

use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

use futures_channel::oneshot::Receiver;
use futures_lite::{future, FutureExt};
use parking_lot::Mutex;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use snafu::OptionExt;
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
    overall: tc::PublicKey,
    node: tc::PublicKeyShare,
    /// u64 nonce used for sorting
    ///
    /// Used for the leader election kludge
    nonce: u64,
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
    type State: Clone;
    /// The type of the transitions we are applying
    type Transaction: Clone + Serialize + DeserializeOwned + Debug + Hash + PartialEq + Eq;
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
pub enum Stage {
    PreVote,
    Vote,
    Commit,
}

/// Holds the state needed to participate in HotStuff consensus
pub struct HotStuff<B> {
    /// The genesis block for this consensus network
    genesis_block: BlockRef<B>,
    /// Block containing the QC for the highest block having one
    highest_qc: (BlockRef<B>, QuorumCertificate),
    /// The currently locked block
    locked_block: BlockRef<B>,
    /// The last executed block
    last_executed: BlockRef<B>,
    /// Blocks waiting on a QC to come in
    qc_waiting: HashMap<BlockRef<B>, Receiver<QuorumCertificate>>,
    /// Current waiting proposal
    propose_waiting: Option<Proposal<BlockRef<B>>>,
    // receive_propsal_waiting
    // hqc_update_waiting
    /// Causes the node to always vote negatively
    ///
    /// Useful for pacemakers
    vote_disabled: bool,
    /// Identity of this replica
    id: ReplicaId,
    /// Private key for this replica
    priv_key: PrivKey,
    /// Public key for this replica
    pub_key: PubKey,
    /// Block storage
    ///
    /// TODO: Abstract this behind a trait, but use an in-memory hashmap for now
    block_storage: HashMap<BlockHash, BlockRef<B>>,
    /// height of the block last voted for
    last_voted_height: u64,
    /// Network handle
    network: Box<dyn NetworkingImplementation<Message<B>> + Send>,
}

impl<B: BlockContents + 'static> HotStuff<B> {
    /// Create a new `HotStuff` instance
    pub fn new(_id: ReplicaId, _priv_key: PrivKey) -> Self {
        todo!()
    }

    /// Check to see if the sanity check has been delivered for a block
    ///
    /// Will return an error if it has not been
    fn sanity_check_delivered(block: BlockRef<B>) -> Result<()> {
        if block.0.lock().delivered {
            Ok(())
        } else {
            Err(HotStuffError::SanityCheckFailure)
        }
    }

    /// Attempts to get an already delivered block
    fn get_delivered_block(&self, hash: &BlockHash) -> Option<BlockRef<B>> {
        self.block_storage.get(hash).cloned()
    }

    /// Action taken on delivering a block
    fn on_deliver_block(&self, block_ref: BlockRef<B>) -> Result<()> {
        let block: &mut Block<B> = &mut block_ref.0.lock();
        if block.delivered {
            Err(HotStuffError::BlockAlreadyDelivered)
        } else {
            let mut new_parents = Vec::new();
            for hash in &block.parent_hashes {
                let block = self
                    .get_delivered_block(hash)
                    .context(SanityCheckFailure)?
                    .clone();
                new_parents.push(block)
            }
            std::mem::swap(&mut block.parents, &mut new_parents);
            block.height = block.parents[0].0.lock().height + 1;
            // Stuff about erasing tails here?
            block.delivered = true;
            Ok(())
        }
    }

    /// Updates the internally stored HQC
    fn update_hqc(&mut self, block_ref: BlockRef<B>, cert: &QuorumCertificate) -> Result<()> {
        if block_ref.0.lock().height > self.highest_qc.0 .0.lock().height {
            self.highest_qc = (block_ref, cert.clone());
            self.on_hqc_update()?;
            Ok(())
        } else {
            Err(HotStuffError::NewHQCInvalid)
        }
    }

    /// Action to take on updating the hqc
    fn on_hqc_update(&mut self) -> Result<()> {
        todo!()
    }

    /// Update the state of the instance with a new block
    fn update(&mut self, next_block: BlockRef<B>) -> Result<()> {
        // I actually think most of these `return Ok(())`s should be errors, but lets copy the
        // original semantics for now
        let block_2 = next_block.0.lock().qc_ref.clone();
        // this whole section is a big ol spaghetti mess more or less copied directly from the original
        if block_2.is_none() {
            // ???
            // https://github.com/hot-stuff/libhotstuff/blob/88f2ebc8ab988d29c892661367e200e41a0c2723/src/consensus.cpp#L99
            return Ok(());
        }
        let block_2 = block_2.unwrap();
        if block_2.0.lock().decision {
            return Ok(());
        }
        // Fix this unwrap. This code makes _heavy_ use of encoding option-likes with null pointers
        self.update_hqc(
            block_2.clone(),
            block_2.0.lock().quorum_certificate.as_ref().unwrap(),
        )?;

        let block_1 = block_2.0.lock().qc_ref.clone();
        if block_1.is_none() {
            return Ok(());
        }
        let block_1 = block_1.unwrap();
        if block_1.0.lock().decision {
            return Ok(());
        }

        let block = block_1.0.lock().qc_ref.clone();
        if block.is_none() {
            return Ok(());
        }
        let block = block.unwrap();

        let mut commit_queue: Vec<BlockRef<B>> = Vec::new();
        let mut block_ref: BlockRef<B> = block.clone();
        // Ugly translation of c++ loop logic
        // Lots on unneeded cloning, really need to Arc the blocks
        while block_ref.0.lock().height > self.last_executed.0.lock().height {
            commit_queue.push(block_ref.clone());
            // Need to play nicer with the borrow checker here
            let tmp = block_ref.0.lock().parents.get(0).cloned();
            if let Some(x) = tmp {
                block_ref = x.clone();
            } else {
                break;
            }
        }

        for x in commit_queue {
            x.0.lock().decision = true;
            self.do_consensus(x.clone())?;
            self.do_decision(x)?;
        }
        self.last_executed = block;
        Ok(())
    }

    /// Stub method for determining the current leader given the current round
    ///
    /// Uses round robin among the known nodes
    async fn current_leader(&self) -> PubKey {
        let mut nodes = self.network.known_nodes().await;
        nodes.sort();
        let index = self.last_voted_height % nodes.len() as u64;
        nodes[index as usize].clone()
    }

    /// Consensus action
    fn do_consensus(&mut self, block: BlockRef<B>) -> Result<()> {
        todo!()
    }

    /// Decision action
    fn do_decision(&mut self, block: BlockRef<B>) -> Result<()> {
        todo!()
    }

    /// Main run action
    fn run_consensus(self) -> future::Boxed<Result<()>> {
        async move {
            // Get this node's id and start the loop
            let id = self.pub_key.clone();
            todo!()
        }
        .boxed()
    }
}
