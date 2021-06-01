use blake3::hash;
use byteorder::{ByteOrder, LittleEndian};
use serde::{Deserialize, Serialize};
use snafu::Snafu;

use crate::BlockContents;

#[derive(PartialEq, Eq, Default, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct CounterBlock {
    pub tx: Option<CounterTransaction>,
}

#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub enum CounterTransaction {
    Inc { previous: u64 },
    Genesis { state: u64 },
}

#[derive(Snafu, Debug)]
pub enum CounterError {
    PreviousDoesNotMatch,
    AlreadyHasTx,
}

impl BlockContents for CounterBlock {
    type State = u64;

    type Transaction = CounterTransaction;

    type Error = CounterError;

    fn add_transaction(
        &self,
        state: &Self::State,
        tx: &Self::Transaction,
    ) -> std::result::Result<Self, Self::Error> {
        if self.tx.is_some() {
            Err(CounterError::AlreadyHasTx)
        } else {
            match tx {
                CounterTransaction::Inc { previous } => {
                    if previous == state {
                        Ok(CounterBlock {
                            tx: Some(tx.clone()),
                        })
                    } else {
                        Err(CounterError::PreviousDoesNotMatch)
                    }
                }
                CounterTransaction::Genesis { .. } => Ok(CounterBlock {
                    tx: Some(tx.clone()),
                }),
            }
        }
    }

    fn validate_block(&self, state: &Self::State) -> bool {
        if let Some(tx) = &self.tx {
            if let CounterTransaction::Inc { previous } = tx {
                previous == state
            } else {
                true
            }
        } else {
            true
        }
    }

    fn append_to(&self, state: &Self::State) -> std::result::Result<Self::State, Self::Error> {
        if let Some(tx) = &self.tx {
            match tx {
                CounterTransaction::Inc { .. } => Ok(state + 1),
                CounterTransaction::Genesis { state } => Ok(*state),
            }
        } else {
            Ok(*state)
        }
    }

    fn hash(&self) -> crate::BlockHash {
        let mut bytes = [0_u8; 9];
        if let Some(tx) = &self.tx {
            match tx {
                CounterTransaction::Inc { previous } => {
                    LittleEndian::write_u64(&mut bytes[1..], *previous)
                }
                CounterTransaction::Genesis { state } => {
                    LittleEndian::write_u64(&mut bytes[1..], *state)
                }
            }
        } else {
            bytes[0] = 1;
        }
        *hash(&bytes).as_bytes()
    }

    fn hash_transaction(tx: &Self::Transaction) -> crate::BlockHash {
        let mut bytes = [0_u8; 9];
        match tx {
            CounterTransaction::Inc { previous } => {
                LittleEndian::write_u64(&mut bytes[1..], *previous)
            }
            CounterTransaction::Genesis { state } => {
                LittleEndian::write_u64(&mut bytes[1..], *state)
            }
        }

        *hash(&bytes).as_bytes()
    }
}
