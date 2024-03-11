//! Abstractions over the immutable instance-level state and hte global state that blocks modify.
//!
//! This module provides the [`InstanceState`] and [`ValidatedState`] traits, which serve as
//! compatibilities over the current network state, which is modified by the transactions contained
//! within blocks.

use super::block_contents::TestableBlock;
use crate::{
    data::Leaf,
    traits::{
        node_implementation::{ConsensusTime, NodeType},
        BlockPayload,
    },
};
use serde::{de::DeserializeOwned, Serialize};
use std::{error::Error, fmt::Debug, future::Future, hash::Hash};

/// Instance-level state, which allows us to fetch missing validated state.
pub trait InstanceState: Debug + Send + Sync {}

/// Abstraction over the state that blocks modify
///
/// This trait represents the behaviors that the 'global' ledger state must have:
///   * A defined error type ([`Error`](ValidatedState::Error))
///   * The type of block that modifies this type of state ([`BlockPayload`](`ValidatedStates::
/// BlockPayload`))
///   * The ability to validate that a block header is actually a valid extension of this state and
/// produce a new state, with the modifications from the block applied
/// ([`validate_and_apply_header`](`ValidatedState::validate_and_apply_header))
pub trait ValidatedState<TYPES: NodeType>:
    Serialize + DeserializeOwned + Debug + Default + Hash + PartialEq + Eq + Send + Sync
{
    /// The error type for this particular type of ledger state
    type Error: Error + Debug + Send + Sync;
    /// The type of the instance-level state this state is assocaited with
    type Instance: InstanceState;
    /// Time compatibility needed for reward collection
    type Time: ConsensusTime;

    /// Check if the proposed block header is valid and apply it to the state if so.
    ///
    /// Returns the new state.
    ///
    /// # Arguments
    /// * `instance` - Immutable instance-level state.
    ///
    /// # Errors
    ///
    /// If the block header is invalid or appending it would lead to an invalid state.
    fn validate_and_apply_header(
        &self,
        instance: &Self::Instance,
        parent_leaf: &Leaf<TYPES>,
        proposed_header: &TYPES::BlockHeader,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send;

    /// Construct the state with the given block header.
    ///
    /// This can also be used to rebuild the state for catchup.
    fn from_header(block_header: &TYPES::BlockHeader) -> Self;

    /// Construct a genesis validated state.
    #[must_use]
    fn genesis(instance: &Self::Instance) -> Self;

    /// Gets called to notify the persistence backend that this state has been committed
    fn on_commit(&self);
}

/// extra functions required on state to be usable by hotshot-testing
pub trait TestableState<TYPES>: ValidatedState<TYPES>
where
    TYPES: NodeType,
    TYPES::BlockPayload: TestableBlock,
{
    /// Creates random transaction if possible
    /// otherwise panics
    /// `padding` is the bytes of padding to add to the transaction
    fn create_random_transaction(
        state: Option<&Self>,
        rng: &mut dyn rand::RngCore,
        padding: u64,
    ) -> <TYPES::BlockPayload as BlockPayload>::Transaction;
}
