use blake3::Hasher;
use serde::{Deserialize, Serialize};
use snafu::Snafu;

use crate::BlockContents;

/// The block format for the counter demo
#[derive(PartialEq, Eq, Default, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct CounterBlock {
    /// A block is composed of a sequence of transactions
    pub tx: Vec<CounterTransaction>,
}

/// Type alias for the state of a counter
type CounterState = u64;

/// The transaction format for a counter demo
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub enum CounterTransaction {
    /// Increments the counter from the specified previous state
    Inc {
        /// The state the counter must currently have for this transaction to be valid
        previous: CounterState,
    },
    /// Force sets the counter to the given state
    Genesis {
        /// State to force the counter to
        state: CounterState,
    },
}

/// Errors that a counter can incur
#[derive(Snafu, Debug)]
pub enum CounterError {
    /// Specified previous state does not match actual previous state
    PreviousDoesNotMatch,
    /// Attempted to add a second transaction to a block
    AlreadyHasTx,
}

/*
Constants to distinguish hash of a non-empty transaction from
an empty one.
 */
/// Constant identifying a transaction with something in it
const TX_SOME: [u8; 1] = [0_u8];
/// Constant identifying an empty transaction
const TX_NONE: [u8; 1] = [1_u8];

impl BlockContents for CounterBlock {
    type State = CounterState;
    type Transaction = CounterTransaction;
    type Error = CounterError;

    /// Add a transation provided either
    ///    - the transaction is an Inc and state matches the tx.previous, or
    ///    - the transaction is a Genesis
    /// Note: Despite the name, this method doesn't do the work of
    /// adding the transaction, rather, it composes a new block.
    fn add_transaction(
        &self,
        state: &Self::State,
        tx: &Self::Transaction,
    ) -> std::result::Result<Self, Self::Error> {
        if self.tx.is_empty() {
            match tx {
                CounterTransaction::Inc { previous } => {
                    if previous == state {
                        Ok(CounterBlock {
                            tx: [tx.clone()].to_vec(),
                        })
                    } else {
                        Err(CounterError::PreviousDoesNotMatch)
                    }
                }
                CounterTransaction::Genesis { .. } => Ok(CounterBlock {
                    tx: [tx.clone()].to_vec(),
                }),
            }
        } else {
            Err(CounterError::AlreadyHasTx)
        }
    }

    /// A block is valid provided one of the following
    ///    - the transaction is an Inc and state matches the tx.previous, or
    ///    - the transaction is a Genesis, or
    ///    - the block's transaction is None
    /// Note: `add_transaction` only accepts valid transactions
    fn validate_block(&self, state: &Self::State) -> bool {
        if self.tx.is_empty() {
            true
        } else {
            match &self.tx[0] {
                CounterTransaction::Inc { previous } => previous == state,
                // TODO !thatonelutenist warning: `state` is being shadowed
                CounterTransaction::Genesis { .. } => true,
            }
        }
    }

    /// The state is nothing other than the counter. This is where the
    /// next state is created.
    ///
    /// Usually, this would be a cryptographic hash of the previous hash
    /// and the current state. Not sure why we're not hashing here.
    #[allow(clippy::clippy::shadow_unrelated)]
    fn append_to(&self, state: &Self::State) -> std::result::Result<Self::State, Self::Error> {
        if self.tx.is_empty() {
            Ok(*state)
        } else {
            match &self.tx[0] {
                CounterTransaction::Inc { .. } => Ok(state + 1),
                CounterTransaction::Genesis { state } => Ok(*state),
            }
        }
    }

    /// Hash a transaction. Include a flag in the hash to distinguish
    /// the hash of a transaction from the hash of an instance with
    /// `tx == None`.
    fn hash_transaction(tx: &Self::Transaction) -> crate::BlockHash {
        let mut hasher = Hasher::new();
        hasher.update(&TX_SOME);
        let bytes = match tx {
            CounterTransaction::Inc { previous } => previous.to_be_bytes(),
            CounterTransaction::Genesis { state } => state.to_be_bytes(),
        };
        hasher.update(&bytes);
        *hasher.finalize().as_bytes()
    }

    /// Hash self's transaction. Prepend a byte flag to distinguish
    /// the empty transaction from the Genesis transaction. Return the
    /// hash of the buffer.
    fn hash(&self) -> crate::BlockHash {
        let mut hasher = Hasher::new();
        if self.tx.is_empty() {
            // Distinguish None from Genesis.
            hasher.update(&TX_NONE);
            hasher.update(&Self::State::default().to_be_bytes());
            *hasher.finalize().as_bytes()
        } else {
            hasher.update(&TX_SOME);
            for tx in &self.tx {
                let bytes = match tx {
                    CounterTransaction::Inc { previous } => previous.to_be_bytes(),
                    CounterTransaction::Genesis { state } => state.to_be_bytes(),
                };
                hasher.update(&bytes);
            }
            *hasher.finalize().as_bytes()
        }
    }
}
