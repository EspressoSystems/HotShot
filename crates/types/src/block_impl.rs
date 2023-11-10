//! This module provides an implementation of the `HotShot` suite of traits.
use std::{
    collections::HashSet,
    fmt::{Debug, Display},
};

use crate::{
    data::{test_srs, VidScheme, VidSchemeTrait},
    traits::{
        block_contents::{BlockHeader, Transaction},
        state::TestableBlock,
        BlockPayload,
    },
};
use ark_serialize::CanonicalDeserialize;
use commit::{Commitment, Committable};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use snafu::Snafu;

// TODO <https://github.com/EspressoSystems/HotShot/issues/1693>
/// Number of storage nodes for VID initiation.
pub const NUM_STORAGE_NODES: usize = 8;
// TODO <https://github.com/EspressoSystems/HotShot/issues/1693>
/// Number of chunks for VID initiation.
pub const NUM_CHUNKS: usize = 8;

/// The transaction in a [`VIDBlockPayload`].
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
        "SEQUENCING_TXN".to_string()
    }
}

impl Transaction for VIDTransaction {}

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

/// A [`BlockPayload`] that contains a list of `VIDTransaction`.
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct VIDBlockPayload {
    /// List of transactions.
    pub transactions: Vec<VIDTransaction>,
    /// VID commitment to the block payload.
    pub payload_commitment: <VidScheme as VidSchemeTrait>::Commit,
}

impl VIDBlockPayload {
    /// Create a genesis block payload with transaction bytes `vec![0]`, to be used for
    /// consensus task initiation.
    /// # Panics
    /// If the `VidScheme` construction fails.
    #[must_use]
    pub fn genesis() -> Self {
        // TODO <https://github.com/EspressoSystems/HotShot/issues/1686>
        let srs = test_srs(NUM_STORAGE_NODES);
        // TODO We are using constant numbers for now, but they will change as the quorum size
        // changes.
        // TODO <https://github.com/EspressoSystems/HotShot/issues/1693>
        let vid = VidScheme::new(NUM_CHUNKS, NUM_STORAGE_NODES, srs).unwrap();
        let txn = vec![0];
        let vid_disperse = vid.disperse(&txn).unwrap();
        VIDBlockPayload {
            transactions: vec![VIDTransaction(txn)],
            payload_commitment: vid_disperse.commit,
        }
    }
}

impl Committable for VIDBlockPayload {
    fn commit(&self) -> Commitment<Self> {
        <Commitment<Self> as CanonicalDeserialize>::deserialize(&*self.payload_commitment)
            .expect("conversion from VidScheme::Commit to Commitment should succeed")
    }

    fn tag() -> String {
        "VID_BLOCK_PAYLOAD".to_string()
    }
}

impl Display for VIDBlockPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BlockPayload #txns={}", self.transactions.len())
    }
}

impl TestableBlock for VIDBlockPayload {
    fn genesis() -> Self {
        Self::genesis()
    }

    fn txn_count(&self) -> u64 {
        self.transactions.len() as u64
    }
}

impl BlockPayload for VIDBlockPayload {
    type Error = BlockPayloadError;

    type Transaction = VIDTransaction;

    fn transaction_commitments(&self) -> HashSet<Commitment<Self::Transaction>> {
        self.transactions
            .iter()
            .map(commit::Committable::commit)
            .collect()
    }
}

/// A [`BlockHeader`] that commits to [`VIDBlockPayload`].
#[derive(PartialEq, Eq, Hash, Clone, Debug, Deserialize, Serialize)]
pub struct VIDBlockHeader {
    /// Block number.
    pub block_number: u64,
    /// VID commitment to the payload.
    pub payload_commitment: Commitment<VIDBlockPayload>,
}

impl BlockHeader for VIDBlockHeader {
    type Payload = VIDBlockPayload;

    fn new(payload_commitment: Commitment<Self::Payload>, parent_header: &Self) -> Self {
        Self {
            block_number: parent_header.block_number + 1,
            payload_commitment,
        }
    }

    fn genesis(payload: Self::Payload) -> Self {
        Self {
            block_number: 0,
            payload_commitment: payload.commit(),
        }
    }

    fn block_number(&self) -> u64 {
        self.block_number
    }

    fn payload_commitment(&self) -> Commitment<Self::Payload> {
        self.payload_commitment
    }
}
