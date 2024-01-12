//! Types and structs related to the stake table

use ethereum_types::U256;
use serde::{Deserialize, Serialize};

/// Stake table entry
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone, Hash, Eq)]
pub struct StakeTableEntry<K: Clone> {
    /// The public key
    pub stake_key: K,
    /// The associated stake amount
    pub stake_amount: U256,
}

impl<K: Clone> StakeTableEntry<K> {
    /// Get the stake amount
    pub fn get_stake(&self) -> U256 {
        self.stake_amount
    }

    /// Get the public key
    pub fn get_key(&self) -> &K {
        &self.stake_key
    }
}

// TODO(Chengyu): add stake table snapshot here
