use blake3::{traits::digest::ExtendableOutput, Hasher};
use serde::{Deserialize, Serialize};
use snafu::{ensure, OptionExt, Snafu};

use std::collections::BTreeMap;

use crate::{BlockContents, BlockHash};

type Account = String;
type Balance = i64;

#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct Subtraction {
    account: Account,
    ammount: Balance,
}

#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct Addition {
    account: Account,
    ammount: Balance,
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
    add: Addition,
    sub: Subtraction,
}

impl Transaction {
    /// Ensures that this transaction is at least consistent with itself
    pub fn validate_independnt(&self) -> bool {
        // Ensure that we are adding to one account exactly as much as we are subtracting from
        // another
        self.add.ammount <= self.sub.ammount
    }
}

/// The state for the dentry demo
#[derive(Clone, Debug)]
pub struct State {
    balances: BTreeMap<Account, Balance>,
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
    previous_block: BlockHash,
    transactions: Vec<Transaction>,
}

impl BlockContents for DEntryBlock {
    type State = State;

    type Transaction = Transaction;

    type Error = DEntryError;

    // returns a new block that does not yet contain any transactions, containing any reference to
    // the current state that it will need
    //
    // Note: api sketch
    fn next_block(state: &Self::State) -> Self {
        Self {
            previous_block: state.hash_state(),
            transactions: Vec::new(),
        }
    }

    fn add_transaction(
        &self,
        state: &Self::State,
        tx: &Self::Transaction,
    ) -> std::result::Result<Self, Self::Error> {
        // first, make sure that the transaction is internally valid
        if tx.validate_independnt() {
            // Now add up all the existing transactions from this block involving the subtraction,
            // we don't want to allow an account balance to go below zero
            let total_so_far: i64 = self
                .transactions
                .iter()
                .filter(|x| x.sub.account == tx.sub.account)
                .map(|x| x.sub.ammount)
                .sum::<i64>()
                + tx.sub.ammount;
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
            if !tx.validate_independnt() {
                return false;
            }
            // Find the input account, and subtract the transfer balance from it, failing if it
            // doesn't exist
            if let Some(input_account) = trial_balances.get_mut(&tx.sub.account) {
                *input_account = *input_account - tx.sub.ammount;
            } else {
                return false;
            }
            // Find the output account, and add the transfer balance to it, failing if it doesn't
            // exist
            if let Some(output_account) = trial_balances.get_mut(&tx.add.account) {
                *output_account = *output_account + tx.add.ammount;
            } else {
                return false;
            }
        }
        // Loop through our account map and make sure nobody is negative
        for (_account, balance) in &trial_balances {
            if *balance < 0 {
                return false;
            }
        }
        // This block has now passed all our tests, and thus has not done anything bad, so the block
        // is valid if its previous state hash matches that of the previous state
        return self.previous_block == state.hash_state();
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
            ensure!(tx.validate_independnt(), InconsistentTransaction);
            // Find the input account, and subtract the transfer balance from it, failing if it
            // doesn't exist
            if let Some(input_account) = trial_balances.get_mut(&tx.sub.account) {
                *input_account = *input_account - tx.sub.ammount;
            } else {
                return Err(DEntryError::NoSuchInputAccount);
            }
            // Find the output account, and add the transfer balance to it, failing if it doesn't
            // exist
            if let Some(output_account) = trial_balances.get_mut(&tx.add.account) {
                *output_account = *output_account + tx.add.ammount;
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
        todo!()
    }

    // Note: this is really used for indexing the transaction in storage
    fn hash_transaction(tx: &Self::Transaction) -> BlockHash {
        todo!()
    }
}
