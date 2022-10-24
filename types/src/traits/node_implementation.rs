//! Composite trait for node behavior
//!
//! This module defines the [`NodeImplementation`] trait, which is a composite trait used for
//! describing the overall behavior of a node, as a composition of implementations of the node trait.

use super::{
    block_contents::Transaction,
    election::{ElectionConfig, VoteToken},
    network::TestableNetworkingImplementation,
    signature_key::TestableSignatureKey,
    state::{ConsensusTime, TestableBlock, TestableState},
    storage::TestableStorage,
    State,
};
use crate::traits::{
    election::Election, network::NetworkingImplementation, signature_key::SignatureKey,
    storage::Storage, Block,
};
use std::fmt::Debug;
use std::hash::Hash;

/// Node implementation aggregate trait
///
/// This trait exists to collect multiple behavior implementations into one type, to allow
/// `HotShot` to avoid annoying numbers of type arguments and type patching.
///
/// It is recommended you implement this trait on a zero sized type, as `HotShot`does not actually
/// store or keep a reference to any value implementing this trait.
pub trait NodeImplementation<TYPES: NodeTypes>: Send + Sync + Debug + Clone + 'static {
    /// Storage type for this consensus implementation
    type Storage: Storage<TYPES> + Clone;
    /// Networking type for this consensus implementation
    type Networking: NetworkingImplementation<TYPES>;
    /// Election
    /// Time is generic here to allow multiple implementations of election trait for difference
    /// consensus protocols
    type Election: Election<TYPES>;
}

pub trait NodeTypes:
    Clone
    + Copy
    + Debug
    + Hash
    + PartialEq
    + Eq
    + PartialOrd
    + Ord
    + Default
    + serde::Serialize
    + for<'de> serde::Deserialize<'de>
    + Send
    + Sync
    + 'static
{
    type Time: ConsensusTime;
    type BlockType: Block<Transaction = Self::Transaction>;
    type SignatureKey: SignatureKey;
    type VoteTokenType: VoteToken;
    type Transaction: Transaction;
    type ElectionConfigType: ElectionConfig;

    type StateType: State<BlockType = Self::BlockType, Time = Self::Time>;
}

/// testable node implmeentation trait
pub trait TestableNodeImplementation<TYPES: NodeTypes>: NodeImplementation<TYPES>
where
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType, Time = TYPES::Time>,
    TYPES::SignatureKey: TestableSignatureKey,
    <Self as NodeImplementation<TYPES>>::Networking: TestableNetworkingImplementation<TYPES>,
    <Self as NodeImplementation<TYPES>>::Storage: TestableStorage<TYPES>,
{
}
