use blake3::Hasher;
use serde::{Deserialize, Serialize};
use snafu::Snafu;

use crate::BlockContents;

#[derive(PartialEq, Eq, Default, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct CounterBlock {
    pub tx: Vec<CounterTransaction>,
}

type CounterState = u64;

#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub enum CounterTransaction {
    Inc { previous: CounterState },
    Genesis { state: CounterState },
}

#[derive(Snafu, Debug)]
pub enum CounterError {
    PreviousDoesNotMatch,
    AlreadyHasTx,
}

/// Constants to distinguish hash of a non-empty transaction from
/// an empty one.
const TX_SOME: [u8; 1] = [0u8];
const TX_NONE: [u8; 1] = [1u8];

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
        if !self.tx.is_empty() {
            Err(CounterError::AlreadyHasTx)
        } else {
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
        }
    }

    /// A block is valid provided one of the following
    ///    - the transaction is an Inc and state matches the tx.previous, or
    ///    - the transaction is a Genesis, or
    ///    - the block's transaction is None
    /// Note: add_transaction only accepts valid transactions
    fn validate_block(&self, state: &Self::State) -> bool {
        if !&self.tx.is_empty() {
            match &self.tx[0] {
                CounterTransaction::Inc { previous } => previous == state,
                CounterTransaction::Genesis { .. } => true,
            }
        } else {
            true
        }
    }

    /// The state is nothing other than the counter. This is where the
    /// next state is created.
    ///
    /// Usually, this would be a cryptographic hash of the previous hash
    /// and the current state. Not sure why we're not hashing here.
    fn append_to(&self, state: &Self::State) -> std::result::Result<Self::State, Self::Error> {
        if !&self.tx.is_empty() {
            match &self.tx[0] {
                CounterTransaction::Inc { .. } => Ok(state + 1),
                CounterTransaction::Genesis { state } => Ok(*state),
            }
        } else {
            Ok(*state)
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
        if !&self.tx.is_empty() {
            hasher.update(&TX_SOME);
            for tx in &self.tx {
                let bytes = match tx {
                    CounterTransaction::Inc { previous } => previous.to_be_bytes(),
                    CounterTransaction::Genesis { state } => state.to_be_bytes(),
                };
                hasher.update(&bytes);
            }
            *hasher.finalize().as_bytes()
        } else {
            // Distinguish None from Genesis.
            hasher.update(&TX_NONE);
            hasher.update(&Self::State::default().to_be_bytes());
            *hasher.finalize().as_bytes()
        }
    }
}
