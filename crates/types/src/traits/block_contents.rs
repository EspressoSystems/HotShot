//! Abstraction over the contents of a block
//!
//! This module provides the [`Transaction`], [`BlockPayload`], and [`BlockHeader`] traits, which
//! describe the behaviors that a block is expected to have.

use crate::data::{test_srs, VidScheme, VidSchemeTrait};
use ark_serialize::CanonicalDeserialize;
use commit::{Commitment, Committable};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use std::{
    collections::HashSet,
    fmt::{Debug, Display},
    hash::Hash,
};

// TODO <https://github.com/EspressoSystems/HotShot/issues/1693>
/// Number of storage nodes for VID initiation.
pub const NUM_STORAGE_NODES: usize = 8;
// TODO <https://github.com/EspressoSystems/HotShot/issues/1693>
/// Number of chunks for VID initiation.
pub const NUM_CHUNKS: usize = 8;

/// Abstraction over any type of transaction. Used by [`BlockPayload`].
pub trait Transaction:
    Clone + Serialize + Debug + PartialEq + Eq + Sync + Send + Committable + Hash
{
    /// Create a new transaction with transaciton bytes.
    fn new(txn: Vec<u8>) -> Self;
}

/// A [`BlockPayload`] that contains a list of [`Transaction`].
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct BlockPayload<TXN: Transaction> {
    /// List of transactions.
    pub transactions: Vec<TXN>,
    /// VID commitment to the block payload.
    pub payload_commitment: <VidScheme as VidSchemeTrait>::Commit,
}

impl<TXN: Transaction> BlockPayload<TXN> {
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
        let vid = VidScheme::new(NUM_CHUNKS, NUM_STORAGE_NODES, &srs).unwrap();
        let txn = vec![0];
        let vid_disperse = vid.disperse(&txn).unwrap();
        BlockPayload {
            transactions: vec![<TXN as Transaction>::new(txn)],
            payload_commitment: vid_disperse.commit,
        }
    }

    /// Commitments of contained transactions.
    pub fn transaction_commitments(&self) -> HashSet<Commitment<TXN>> {
        self.transactions
            .iter()
            .map(commit::Committable::commit)
            .collect()
    }

    /// Get the number of transactions.
    #[must_use]
    pub fn txn_count(&self) -> u64 {
        self.transactions.len() as u64
    }
}

impl<TXN: Transaction> Committable for BlockPayload<TXN> {
    fn commit(&self) -> Commitment<Self> {
        <Commitment<Self> as CanonicalDeserialize>::deserialize(&*self.payload_commitment)
            .expect("conversion from VidScheme::Commit to Commitment should succeed")
    }

    fn tag() -> String {
        "VID_BLOCK_PAYLOAD".to_string()
    }
}

impl<TXN: Transaction> Display for BlockPayload<TXN> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BlockPayload #txns={}", self.transactions.len())
    }
}

/// Header of a block, which commits to a [`BlockPayload`].
pub trait BlockHeader:
    Serialize + Clone + Debug + Hash + PartialEq + Eq + Send + Sync + DeserializeOwned
{
    /// Transaction associated with the payload commitment.
    type Transaction: Transaction;

    /// Build a header with the payload commitment and parent header.
    fn new(
        payload_commitment: Commitment<BlockPayload<Self::Transaction>>,
        parent_header: &Self,
    ) -> Self;

    /// Build a genesis header with the genesis payload.
    fn genesis(payload: BlockPayload<Self::Transaction>) -> Self;

    /// Get the block number.
    fn block_number(&self) -> u64;

    /// Get the payload commitment.
    fn payload_commitment(&self) -> Commitment<BlockPayload<Self::Transaction>>;
}
