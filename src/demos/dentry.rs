//! Double entry accounting demo
//!
//! This module provides an implementation of the `PhaseLock` suite of traits that implements a
//! basic demonstration of double entry accounting.
//!
//! These implementations are useful in examples and integration testing, but are not suitable for
//! production use.

use blake3::Hasher;
use phaselock_types::data::{Leaf, QuorumCertificate, Stage};
use rand::{thread_rng, Rng};
use serde::{Deserialize, Serialize};
use snafu::{ensure, Snafu};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
    marker::PhantomData,
};
use tracing::error;

use crate::{
    data::{BlockHash, LeafHash, StateHash, TransactionHash},
    traits::{
        implementations::MemoryStorage, BlockContents, NetworkingImplementation, NodeImplementation,
    },
    types::Message,
    H_256,
};

/// The account identifier type used by the demo
///
/// This is a type alias to [`String`] for simplicity.
pub type Account = String;

/// The account balance type used by the demo
///
/// This is a type alias to [`i64`] for simplicity.
pub type Balance = i64;

/// Records a reduction in an account balance
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct Subtraction {
    /// An account identifier
    pub account: Account,
    /// An account balance
    pub amount: Balance,
}

/// Records an increase in an account balance
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct Addition {
    /// An account identifier
    pub account: Account,
    /// An account balance
    pub amount: Balance,
}

/// The error type for the dentry demo
#[derive(Snafu, Debug)]
pub enum DEntryError {
    /// The subtraction and addition amounts for this transaction were not equal
    InconsistentTransaction,
    /// No such input account exists
    NoSuchInputAccount,
    /// No such output account exists
    NoSuchOutputAccount,
    /// Tried to move more money than was in the account
    InsufficentBalance,
    /// Previous state commitment does not match
    PreviousStateMismatch,
    /// Nonce was reused
    ReusedNonce,
}

/// The transaction for the dentry demo
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct Transaction {
    /// An increment to an account balance
    pub add: Addition,
    /// A decrement to an account balance
    pub sub: Subtraction,
    /// The nonce for a transaction, no two transactions can have the same nonce
    pub nonce: u64,
}

impl Transaction {
    /// Ensures that this transaction is at least consistent with itself
    pub fn validate_independence(&self) -> bool {
        // Ensure that we are adding to one account exactly as much as we are subtracting from
        // another
        self.add.amount <= self.sub.amount // TODO why not strict equality?
    }
}

/// The state for the dentry demo
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct State {
    /// Key/value store of accounts and balances
    pub balances: BTreeMap<Account, Balance>,
    /// Set of previously seen nonces
    pub nonces: BTreeSet<u64>,
}

impl State {
    /// Produces a hash of the current state
    fn hash_state(&self) -> StateHash<H_256> {
        // BTreeMap sorts so this will be in a consistent order
        let mut hasher = Hasher::new();
        for (account, balance) in &self.balances {
            // account first
            hasher.update(account.as_bytes());
            // then balance
            hasher.update(&balance.to_be_bytes());
        }
        // Add nonces to hash
        for nonce in &self.nonces {
            hasher.update(&nonce.to_be_bytes());
        }
        let x = *hasher.finalize().as_bytes();
        x.into()
    }
}

impl crate::traits::State<H_256> for State {
    type Error = DEntryError;

    type Block = DEntryBlock;

    fn next_block(&self) -> Self::Block {
        DEntryBlock {
            previous_block: self.hash_state(),
            transactions: Vec::new(),
        }
    }

    // Note: validate_block is actually somewhat redundant, its meant to be a quick and dirty check
    // for clarity, the logic is duplicated with append_to
    fn validate_block(&self, block: &Self::Block) -> bool {
        let state = self;
        // A valid block is one in which every transaction is internally consistent, and results in
        // nobody having a negative balance
        //
        // We will check this, in this case, by making a trial copy of our balances map, making
        // trial modifications to it, and then asserting that no balances are negative
        let mut trial_balances = state.balances.clone();
        for tx in &block.transactions {
            // This is a macro from SNAFU that returns an Err if the condition is not satisfied
            //
            // We first check that the transaction is internally consistent, then apply the change
            // to our trial map
            if !tx.validate_independence() {
                error!("validate_independence failed");
                return false;
            }
            // Find the input account, and subtract the transfer balance from it, failing if it
            // doesn't exist
            if let Some(input_account) = trial_balances.get_mut(&tx.sub.account) {
                *input_account -= tx.sub.amount;
            } else {
                error!("no such input account");
                return false;
            }
            // Find the output account, and add the transfer balance to it, failing if it doesn't
            // exist
            if let Some(output_account) = trial_balances.get_mut(&tx.add.account) {
                *output_account += tx.add.amount;
            } else {
                error!("no such output account");
                return false;
            }
            // Check to make sure the nonce isn't used
            if state.nonces.contains(&tx.nonce) {
                return false;
            }
        }
        // Loop through our account map and make sure nobody is negative
        for balance in trial_balances.values() {
            if *balance < 0 {
                error!("negative balance");
                return false;
            }
        }
        // This block has now passed all our tests, and thus has not done anything bad, so the block
        // is valid if its previous state hash matches that of the previous state
        let result = block.previous_block == state.hash_state();
        if !result {
            error!(
                "hash failure. previous_block: {:?} hash_state: {:?}",
                block.previous_block,
                state.hash_state()
            );
        }
        result
    }

    fn append(&self, block: &Self::Block) -> std::result::Result<Self, Self::Error> {
        let state = self;
        // A valid block is one in which every transaction is internally consistent, and results in
        // nobody having a negative balance
        //
        // We will check this, in this case, by making a trial copy of our balances map, making
        // trial modifications to it, and then asserting that no balances are negative
        let mut trial_balances = state.balances.clone();
        for tx in &block.transactions {
            // This is a macro from SNAFU that returns an Err if the condition is not satisfied
            //
            // We first check that the transaction is internally consistent, then apply the change
            // to our trial map
            ensure!(tx.validate_independence(), InconsistentTransactionSnafu);
            // Find the input account, and subtract the transfer balance from it, failing if it
            // doesn't exist
            if let Some(input_account) = trial_balances.get_mut(&tx.sub.account) {
                *input_account -= tx.sub.amount;
            } else {
                return Err(DEntryError::NoSuchInputAccount);
            }
            // Find the output account, and add the transfer balance to it, failing if it doesn't
            // exist
            if let Some(output_account) = trial_balances.get_mut(&tx.add.account) {
                *output_account += tx.add.amount;
            } else {
                return Err(DEntryError::NoSuchOutputAccount);
            }
            // Check for nonce reuse
            if state.nonces.contains(&tx.nonce) {
                return Err(DEntryError::ReusedNonce);
            }
        }
        // Loop through our account map and make sure nobody is negative
        for balance in trial_balances.values() {
            if *balance < 0 {
                return Err(DEntryError::InsufficentBalance);
            }
        }
        // Make sure our previous state commitment matches the provided state
        if block.previous_block == state.hash_state() {
            // This block has now passed all our tests, and thus has not done anything bad
            // Add the nonces from this block
            let mut nonces = state.nonces.clone();
            for tx in &block.transactions {
                nonces.insert(tx.nonce);
            }
            Ok(State {
                balances: trial_balances,
                nonces,
            })
        } else {
            Err(DEntryError::PreviousStateMismatch)
        }
    }

    fn on_commit(&self) {
        // Does nothing in this implementation
    }
}

/// The block for the dentry demo
#[derive(PartialEq, Eq, Default, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct DEntryBlock {
    /// Block state commitment
    pub previous_block: StateHash<H_256>,
    /// Transaction vector
    pub transactions: Vec<Transaction>,
}

impl BlockContents<H_256> for DEntryBlock {
    type Transaction = Transaction;

    type Error = DEntryError;

    fn add_transaction_raw(
        &self,
        tx: &Self::Transaction,
    ) -> std::result::Result<Self, Self::Error> {
        // first, make sure that the transaction is internally valid
        if tx.validate_independence() {
            // Then check the previous transactions in the block
            if self.transactions.iter().any(|x| x.nonce == tx.nonce) {
                return Err(DEntryError::ReusedNonce);
            }
            let mut new_block = self.clone();
            // Insert our transaction and return
            new_block.transactions.push(tx.clone());
            Ok(new_block)
        } else {
            Err(DEntryError::InconsistentTransaction)
        }
    }

    // Note: this is really used for indexing the block in storage
    fn hash(&self) -> BlockHash<H_256> {
        let mut hasher = Hasher::new();
        hasher.update(self.previous_block.as_ref());
        self.transactions.iter().for_each(|tx| {
            hasher.update(Self::hash_transaction(tx).as_ref());
        });
        let x = *hasher.finalize().as_bytes();
        x.into()
    }

    // Note: This is used for indexing the transaction in storage
    fn hash_transaction(tx: &Self::Transaction) -> TransactionHash<H_256> {
        assert!(tx.validate_independence());
        let mut hasher = Hasher::new();
        hasher.update(tx.add.account.as_bytes());
        hasher.update(&tx.add.amount.to_be_bytes());
        hasher.update(tx.sub.account.as_bytes());
        hasher.update(&tx.sub.amount.to_be_bytes());
        hasher.update(&tx.nonce.to_be_bytes());
        let x = *hasher.finalize().as_bytes();
        x.into()
    }

    fn hash_leaf(bytes: &[u8]) -> LeafHash<H_256> {
        let x = *blake3::hash(bytes).as_bytes();
        x.into()
    }
}

/// The node implementation for the dentry demo
#[derive(Clone)]
pub struct DEntryNode<NET> {
    /// Network phantom
    _phantom: PhantomData<NET>,
}

impl<NET> DEntryNode<NET> {
    /// Create a new `DEntryNode`
    pub fn new() -> Self {
        DEntryNode {
            _phantom: PhantomData,
        }
    }
}

impl<NET> Debug for DEntryNode<NET> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DEntryNode")
            .field("_phantom", &"phantom")
            .finish()
    }
}

impl<NET> Default for DEntryNode<NET> {
    fn default() -> Self {
        Self::new()
    }
}

impl<NET> NodeImplementation<H_256> for DEntryNode<NET>
where
    NET: NetworkingImplementation<Message<DEntryBlock, Transaction, State, H_256>>
        + Clone
        + Debug
        + 'static,
{
    type Block = DEntryBlock;

    type State = State;

    type Storage = MemoryStorage<DEntryBlock, State, H_256>;

    type Networking = NET;

    type StatefulHandler = crate::traits::implementations::Stateless<DEntryBlock, State, H_256>;
}

/// Provides a random valid transaction from the current state
/// # Panics
/// panics if state has no balances
pub fn random_transaction<R: rand::Rng>(state: &State, mut rng: &mut R) -> Transaction {
    use rand::seq::IteratorRandom;
    let mut attempts = 0;
    #[allow(clippy::panic)]
    let (input_account, output_account, amount) = loop {
        let input_account = state.balances.keys().choose(&mut rng).unwrap();
        let output_account = state.balances.keys().choose(&mut rng).unwrap();
        let balance = state.balances[input_account];
        if balance != 0 {
            let amount = rng.gen_range(0, state.balances[input_account]);
            break (input_account, output_account, amount);
        }
        attempts += 1;
        assert!(
            attempts <= 10,
            "Could not create a transaction. {:?}",
            state
        );
    };
    Transaction {
        add: Addition {
            account: output_account.to_string(),
            amount,
        },
        sub: Subtraction {
            account: input_account.to_string(),
            amount,
        },
        nonce: rng.gen(),
    }
}

/// Provides a random [`QuorumCertificate`]
pub fn random_quorom_certificate<const N: usize>() -> QuorumCertificate<N> {
    let mut rng = thread_rng();
    let stage = match rng.gen_range(0u8, 5) {
        0 => Stage::None,
        1 => Stage::Prepare,
        2 => Stage::PreCommit,
        3 => Stage::Commit,
        4 => Stage::Decide,
        _ => unreachable!(),
    };
    // TODO: Generate a tc::Signature
    QuorumCertificate {
        block_hash: BlockHash::random(),
        genesis: rng.gen(),
        leaf_hash: LeafHash::random(),
        signature: None,
        stage,
        view_number: rng.gen(),
    }
}

/// Provides a random [`Leaf`]
pub fn random_leaf<const N: usize>(item: DEntryBlock) -> Leaf<DEntryBlock, N> {
    let parent = LeafHash::random();
    Leaf { parent, item }
}
