//! Sequencing consensus demo
//!
//! This module provides an implementation of the `HotShot` suite of traits that implements a
//! basic demonstration of sequencing consensus.
//!
//! These implementations are useful in examples and integration testing, but are not suitable for
//! production use.
use std::{collections::HashSet, ops::Deref};

use commit::{Commitment, Committable};
use espresso_systems_common::hotshot::tag;
use hotshot_types::{
    data::{fake_commitment, ViewNumber},
    traits::{
        block_contents::Transaction,
        node_implementation::ApplicationMetadata,
        state::{ConsensusTime, TestableBlock, TestableState, ValidatingConsensus},
        Block, State,
    },
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use snafu::Snafu;

/// The transaction for the sequencing demo
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct SDemoTransaction {
    /// identifier for the transaction
    pub id: u64,
    /// padding to add to txn (to make it larger and thereby more realistic)
    pub padding: Vec<u8>,
}

impl Deref for SDemoTransaction {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.id
    }
}

impl Committable for SDemoTransaction {
    fn commit(&self) -> Commitment<Self> {
        commit::RawCommitmentBuilder::new("SDemo Txn Comm")
            .u64_field("id", self.id)
            .finalize()
    }

    fn tag() -> String {
        tag::DENTRY_TXN.to_string()
    }
}

impl Transaction for SDemoTransaction {}

impl SDemoTransaction {
    /// create a new transaction
    pub fn new(id: u64) -> Self {
        Self {
            id,
            padding: vec![],
        }
    }
}

/// genesis block
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct SDemoGenesisBlock {}

/// Any block after genesis
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct SDemoNormalBlock {
    /// Block state commitment
    pub previous_state: Commitment<SDemoState>,
    /// Transaction vector
    pub transactions: Vec<SDemoTransaction>,
}

/// The block for the sequencing demo
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub enum SDemoBlock {
    /// genesis block
    Genesis(SDemoGenesisBlock),
    /// normal block
    Normal(SDemoNormalBlock),
}

impl Committable for SDemoBlock {
    fn commit(&self) -> Commitment<Self> {
        match &self {
            SDemoBlock::Genesis(_) => {
                commit::RawCommitmentBuilder::new("SDemo Genesis Comm").finalize()
            }
            SDemoBlock::Normal(block) => {
                let mut builder = commit::RawCommitmentBuilder::new("SDemo Normal Comm")
                    .var_size_field("Previous State", block.previous_state.as_ref());
                for txn in &block.transactions {
                    builder = builder.u64_field("transaction", **txn);
                }
                builder.finalize()
            }
        }
    }

    fn tag() -> String {
        tag::DENTRY_BLOCK.to_string()
    }
}

/// sequencing demo entry state
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct SDemoState {
    /// the block height
    block_height: u64,
    /// the view number
    view_number: ViewNumber,
    /// the previous state commitment
    prev_state_commitment: Commitment<Self>,
}

impl Committable for SDemoState {
    fn commit(&self) -> Commitment<Self> {
        commit::RawCommitmentBuilder::new("SDemo State Commit")
            .u64_field("block_height", self.block_height)
            .u64_field("view_number", *self.view_number)
            .field("prev_state_commitment", self.prev_state_commitment)
            .finalize()
    }

    fn tag() -> String {
        tag::DENTRY_STATE.to_string()
    }
}

/// application metadata stub
/// FIXME this is deprecated right?
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct SDemoMetaData {}
impl ApplicationMetadata for SDemoMetaData {}

impl Default for SDemoState {
    fn default() -> Self {
        Self {
            block_height: 0,
            view_number: ViewNumber::genesis(),
            prev_state_commitment: fake_commitment(),
        }
    }
}

/// The error type for the sequencing demo
#[derive(Snafu, Debug)]
pub enum SDemoError {
    /// Previous state commitment does not match
    PreviousStateMismatch,
    /// Nonce was reused
    ReusedTxn,
    /// Genesis failure
    GenesisFailed,
    /// Genesis reencountered after initialization
    GenesisAfterStart,
    /// no transasctions added to genesis
    GenesisCantHaveTransactions,
    /// invalid block
    InvalidBlock,
}

impl TestableBlock for SDemoBlock {
    fn genesis() -> Self {
        SDemoBlock::Genesis(SDemoGenesisBlock {})
    }

    fn txn_count(&self) -> u64 {
        match self {
            SDemoBlock::Genesis(_) => 0,
            SDemoBlock::Normal(n) => n.transactions.len() as u64,
        }
    }
}

impl Block for SDemoBlock {
    type Error = SDemoError;

    type Transaction = SDemoTransaction;

    fn new() -> Self {
        <Self as TestableBlock>::genesis()
    }

    fn add_transaction_raw(
        &self,
        tx: &Self::Transaction,
    ) -> std::result::Result<Self, Self::Error> {
        match self {
            SDemoBlock::Genesis(_) => Err(SDemoError::GenesisCantHaveTransactions),
            SDemoBlock::Normal(n) => {
                let mut new = n.clone();
                new.transactions.push(tx.clone());
                Ok(SDemoBlock::Normal(new))
            }
        }
    }

    fn contained_transactions(&self) -> HashSet<Commitment<Self::Transaction>> {
        match self {
            SDemoBlock::Genesis(_) => HashSet::new(),
            SDemoBlock::Normal(n) => n
                .transactions
                .iter()
                .map(commit::Committable::commit)
                .collect(),
        }
    }
}

impl State for SDemoState {
    type Error = SDemoError;

    type BlockType = SDemoBlock;

    type Time = ViewNumber;

    type ConsensusType = ValidatingConsensus;

    fn next_block(&self) -> Self::BlockType {
        SDemoBlock::Normal(SDemoNormalBlock {
            previous_state: self.commit(),
            transactions: Vec::new(),
        })
    }

    fn validate_block(&self, block: &Self::BlockType, view_number: &Self::Time) -> bool {
        match block {
            SDemoBlock::Genesis(_) => {
                view_number > &ViewNumber::genesis() && *view_number > self.view_number
            }
            SDemoBlock::Normal(n) => {
                n.previous_state == self.commit() && self.view_number < *view_number
            }
        }
    }

    fn append(
        &self,
        block: &Self::BlockType,
        view_number: &Self::Time,
    ) -> Result<Self, Self::Error> {
        if !self.validate_block(block, view_number) {
            return Err(SDemoError::InvalidBlock);
        }

        Ok(SDemoState {
            block_height: self.block_height + 1,
            view_number: *view_number,
            prev_state_commitment: self.commit(),
        })
    }

    fn on_commit(&self) {}
}

impl TestableState for SDemoState {
    fn create_random_transaction(
        &self,
        rng: &mut dyn rand::RngCore,
        padding: u64,
    ) -> <Self::BlockType as Block>::Transaction {
        SDemoTransaction {
            id: rng.gen_range(0..10),
            padding: vec![0; padding as usize],
        }
    }
}
