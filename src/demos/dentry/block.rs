use blake3::Hasher;
use serde::{Deserialize, Serialize};
use snafu::{ensure, OptionExt, Snafu};
use std::collections::BTreeMap;
use tracing::{debug, error, info, info_span, instrument, trace, warn, Instrument};

use crate::{BlockContents, BlockHash};

/// An account identifier
pub type Account = String;

/// An account balance
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
}

/// The transaction for the dentry demo
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct Transaction {
    /// An increment to an account balance
    pub add: Addition,
    /// A decrement to an account balance
    pub sub: Subtraction,
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
#[derive(Clone, Debug)]
pub struct State {
    /// Key/value store of accounts and balances
    pub balances: BTreeMap<Account, Balance>,
}

impl State {
    fn hash_state(&self) -> BlockHash {
        // BTreeMap sorts so this will be in a consistent order
        let mut hasher = Hasher::new();
        for (account, balance) in &self.balances {
            // account first
            hasher.update(account.as_bytes());
            // then balance
            hasher.update(&balance.to_be_bytes());
        }
        *hasher.finalize().as_bytes()
    }
}

/// The block for the dentry demo
#[derive(PartialEq, Eq, Default, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct DEntryBlock {
    /// Block state commitment
    pub previous_block: BlockHash,
    /// Transaction vector
    pub transactions: Vec<Transaction>,
}

// returns a new block that does not yet contain any transactions, containing any reference to
// the current state that it will need
//
// TODO Note: api sketch
fn next_block(state: &State) -> DEntryBlock {
    DEntryBlock {
        previous_block: state.hash_state(),
        transactions: Vec::new(),
    }
}

impl BlockContents for DEntryBlock {
    type State = State;

    type Transaction = Transaction;

    type Error = DEntryError;

    fn add_transaction(
        &self,
        state: &Self::State,
        tx: &Self::Transaction,
    ) -> std::result::Result<Self, Self::Error> {
        // first, make sure that the transaction is internally valid
        if tx.validate_independence() {
            // TODO spelling
            // Now add up all the existing transactions from this block involving the subtraction,
            // we don't want to allow an account balance to go below zero
            let total_so_far: i64 = self
                .transactions
                .iter()
                .filter(|x| x.sub.account == tx.sub.account)
                .map(|x| x.sub.amount)
                .sum::<i64>()
                + tx.sub.amount;
            // Look up the current balance for the account, and make sure we aren't subtracting more
            // than they currently have, returning an error if the account does not exist
            let current_balance: i64 = *state
                .balances
                .get(&tx.sub.account)
                .context(NoSuchInputAccount)?;
            // Make sure that the total we are trying to transact in this block is less than or
            // equal to the current account balance, otherwise return an error
            if total_so_far <= current_balance {
                // Make a copy of ourselves to modify (ideally we would be using persistent data
                // structures, so this would be low cost)
                let mut new_block = self.clone();
                // Insert our transaction and return
                new_block.transactions.push(tx.clone());
                Ok(new_block)
            } else {
                Err(DEntryError::InsufficentBalance)
            }
        } else {
            Err(DEntryError::InconsistentTransaction)
        }
    }

    // Note: validate_block is actually somewhat redundant, its meant to be a quick and dirty check
    // for clarity, the logic is duplicated with append_to
    fn validate_block(&self, state: &Self::State) -> bool {
        // A valid block is one in which every transaction is internally consistent, and results in
        // nobody having a negative balance
        //
        // We will check this, in this case, by making a trial copy of our balances map, making
        // trial modifications to it, and then asserting that no balances are negative
        let mut trial_balances = state.balances.clone();
        for tx in &self.transactions {
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
                *input_account = *input_account - tx.sub.amount;
            } else {
                error!("no such input account");
                return false;
            }
            // Find the output account, and add the transfer balance to it, failing if it doesn't
            // exist
            if let Some(output_account) = trial_balances.get_mut(&tx.add.account) {
                *output_account = *output_account + tx.add.amount;
            } else {
                error!("no such output account");
                return false;
            }
        }
        // Loop through our account map and make sure nobody is negative
        for (_account, balance) in &trial_balances {
            if *balance < 0 {
                error!("negative balance");
                return false;
            }
        }
        // This block has now passed all our tests, and thus has not done anything bad, so the block
        // is valid if its previous state hash matches that of the previous state
        let result = &self.previous_block == &state.hash_state();
        if !result {
            error!(
                "hash failure. previous_block: {:?} hash_state: {:?}",
                self.previous_block,
                state.hash_state()
            );
        }
        return result;
    }

    fn append_to(&self, state: &Self::State) -> std::result::Result<Self::State, Self::Error> {
        // A valid block is one in which every transaction is internally consistent, and results in
        // nobody having a negative balance
        //
        // We will check this, in this case, by making a trial copy of our balances map, making
        // trial modifications to it, and then asserting that no balances are negative
        let mut trial_balances = state.balances.clone();
        for tx in &self.transactions {
            // This is a macro from SNAFU that returns an Err if the condition is not satisfied
            //
            // We first check that the transaction is internally consistent, then apply the change
            // to our trial map
            ensure!(tx.validate_independence(), InconsistentTransaction);
            // Find the input account, and subtract the transfer balance from it, failing if it
            // doesn't exist
            if let Some(input_account) = trial_balances.get_mut(&tx.sub.account) {
                *input_account = *input_account - tx.sub.amount;
            } else {
                return Err(DEntryError::NoSuchInputAccount);
            }
            // Find the output account, and add the transfer balance to it, failing if it doesn't
            // exist
            if let Some(output_account) = trial_balances.get_mut(&tx.add.account) {
                *output_account = *output_account + tx.add.amount;
            } else {
                return Err(DEntryError::NoSuchOutputAccount);
            }
        }
        // Loop through our account map and make sure nobody is negative
        for (_account, balance) in &trial_balances {
            if *balance < 0 {
                return Err(DEntryError::InsufficentBalance);
            }
        }
        // Make sure our previous state commitment matches the provided state
        if self.previous_block == state.hash_state() {
            // This block has now passed all our tests, and thus has not done anything bad
            Ok(State {
                balances: trial_balances,
            })
        } else {
            Err(DEntryError::PreviousStateMismatch)
        }
    }

    // Note: this is really used for indexing the block in storage
    fn hash(&self) -> BlockHash {
        let mut hasher = Hasher::new();
        hasher.update(&self.previous_block);
        self.transactions.iter().for_each(|tx| {
            hasher.update(&Self::hash_transaction(tx));
            ()
        });
        *hasher.finalize().as_bytes()
    }

    // Note: This is used for indexing the transaction in storage
    fn hash_transaction(tx: &Self::Transaction) -> BlockHash {
        assert!(tx.validate_independence());
        let mut hasher = Hasher::new();
        hasher.update(&tx.add.account.as_bytes());
        hasher.update(&tx.add.amount.to_be_bytes());
        hasher.update(&tx.sub.account.as_bytes());
        hasher.update(&tx.sub.amount.to_be_bytes());
        *hasher.finalize().as_bytes()
    }
}
