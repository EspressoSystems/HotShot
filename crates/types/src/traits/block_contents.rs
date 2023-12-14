//! Abstraction over the contents of a block
//!
//! This module provides the [`Transaction`], [`BlockPayload`], and [`BlockHeader`] traits, which
//! describe the behaviors that a block is expected to have.

use crate::data::{test_srs, VidCommitment, VidScheme, VidSchemeTrait};
use commit::{Commitment, Committable};
use serde::{de::DeserializeOwned, Serialize};

use std::{
    error::Error,
    fmt::{Debug, Display},
    hash::Hash,
};

/// Abstraction over any type of transaction. Used by [`BlockPayload`].
pub trait Transaction:
    Clone + Serialize + DeserializeOwned + Debug + PartialEq + Eq + Sync + Send + Committable + Hash
{
}

/// Abstraction over the full contents of a block
///
/// This trait encapsulates the behaviors that the transactions of a block must have in order to be
/// used by consensus
///   * Must have a predefined error type ([`BlockPayload::Error`])
///   * Must have a transaction type that can be compared for equality, serialized and serialized,
///     sent between threads, and can have a hash produced of it
///   * Must be hashable
pub trait BlockPayload:
    Serialize + Clone + Debug + Display + Hash + PartialEq + Eq + Send + Sync + DeserializeOwned
{
    /// The error type for this type of block
    type Error: Error + Debug + Send + Sync;

    /// The type of the transitions we are applying
    type Transaction: Transaction;

    /// Data created during block building which feeds into the block header
    type Metadata: Clone + Debug + DeserializeOwned + Eq + Hash + Send + Sync + Serialize;

    /// Encoded payload.
    type Encode<'a>: 'a + Iterator<Item = u8> + Send
    where
        Self: 'a;

    /// Build a payload and associated metadata with the transactions.
    ///
    /// # Errors
    /// If the transaction length conversion fails.
    fn from_transactions(
        transactions: impl IntoIterator<Item = Self::Transaction>,
    ) -> Result<(Self, Self::Metadata), Self::Error>;

    /// Build a payload with the encoded transaction bytes, metadata,
    /// and the associated number of VID storage nodes
    ///
    /// `I` may be, but not necessarily is, the `Encode` type directly from `fn encode`.
    fn from_bytes<I>(encoded_transactions: I, metadata: Self::Metadata) -> Self
    where
        I: Iterator<Item = u8>;

    /// Build the genesis payload and metadata.
    fn genesis() -> (Self, Self::Metadata);

    /// Encode the payload
    ///
    /// # Errors
    /// If the transaction length conversion fails.
    fn encode(&self) -> Result<Self::Encode<'_>, Self::Error>;

    /// List of transaction commitments.
    fn transaction_commitments(&self) -> Vec<Commitment<Self::Transaction>>;
}

/// Compute the VID payload commitment.
/// # Panics
/// If the VID computation fails.
#[must_use]
pub fn vid_commitment(
    encoded_transactions: &Vec<u8>,
    num_storage_nodes: usize,
) -> <VidScheme as VidSchemeTrait>::Commit {
    let num_chunks = 1 << num_storage_nodes.ilog2();

    // TODO <https://github.com/EspressoSystems/HotShot/issues/1686>
    let srs = test_srs(num_storage_nodes);

    let vid = VidScheme::new(num_chunks, num_storage_nodes, srs).unwrap();
    vid.commit_only(encoded_transactions).unwrap()
}

/// Header of a block, which commits to a [`BlockPayload`].
pub trait BlockHeader:
    Serialize + Clone + Debug + Hash + PartialEq + Eq + Send + Sync + DeserializeOwned + Committable
{
    /// Block payload associated with the commitment.
    type Payload: BlockPayload;

    /// Build a header with the payload commitment, metadata, and parent header.
    fn new(
        payload_commitment: VidCommitment,
        metadata: <Self::Payload as BlockPayload>::Metadata,
        parent_header: &Self,
    ) -> Self;

    /// Build the genesis header, payload, and metadata.
    fn genesis() -> (
        Self,
        Self::Payload,
        <Self::Payload as BlockPayload>::Metadata,
    );

    /// Get the block number.
    fn block_number(&self) -> u64;

    /// Get the payload commitment.
    fn payload_commitment(&self) -> VidCommitment;

    /// Get the metadata.
    fn metadata(&self) -> <Self::Payload as BlockPayload>::Metadata;
}
