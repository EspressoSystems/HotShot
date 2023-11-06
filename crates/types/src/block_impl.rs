//! This module provides an implementation of the `HotShot` suite of traits.
use std::fmt::Debug;

use crate::traits::{
    block_contents::{BlockHeader, Transaction},
    BlockPayload,
};
use commit::{Commitment, Committable};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use snafu::Snafu;

/// The transaction that contains a list of bytes only.
#[derive(Default, PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct VIDTransaction(pub Vec<u8>);

impl Committable for VIDTransaction {
    fn commit(&self) -> Commitment<Self> {
        let builder = commit::RawCommitmentBuilder::new("Txn Comm");
        let mut hasher = Keccak256::new();
        hasher.update(&self.0);
        let generic_array = hasher.finalize();
        builder.generic_byte_array(&generic_array).finalize()
    }

    fn tag() -> String {
        "TXN".to_string()
    }
}

impl Transaction for VIDTransaction {
    fn new(txn: Vec<u8>) -> Self {
        Self(txn)
    }
}

/// The error type for block payload.
#[derive(Snafu, Debug)]
pub enum BlockPayloadError {
    /// Previous state commitment does not match
    PreviousStateMismatch,
    /// Nonce was reused
    ReusedTxn,
    /// Genesis failure
    GenesisFailed,
    /// Genesis reencountered after initialization
    GenesisAfterStart,
    /// invalid block
    InvalidBlock,
}

/// A [`BlockHeader`] that commits to a [`BlockPayload`] associated with [`VIDTransaction`].
#[derive(PartialEq, Eq, Hash, Clone, Debug, Deserialize, Serialize)]
pub struct VIDBlockHeader {
    /// Block number.
    pub block_number: u64,
    /// VID commitment to the payload.
    pub payload_commitment: Commitment<BlockPayload<VIDTransaction>>,
}

impl BlockHeader for VIDBlockHeader {
    type Transaction = VIDTransaction;

    fn new(
        payload_commitment: Commitment<BlockPayload<Self::Transaction>>,
        parent_header: &Self,
    ) -> Self {
        Self {
            block_number: parent_header.block_number + 1,
            payload_commitment,
        }
    }

    fn genesis(payload: BlockPayload<Self::Transaction>) -> Self {
        Self {
            block_number: 0,
            payload_commitment: payload.commit(),
        }
    }

    fn block_number(&self) -> u64 {
        self.block_number
    }

    fn payload_commitment(&self) -> Commitment<BlockPayload<Self::Transaction>> {
        self.payload_commitment
    }
}
