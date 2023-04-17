//! Composite trait for node behavior
//!
//! This module defines the [`NodeImplementation`] trait, which is a composite trait used for
//! describing the overall behavior of a node, as a composition of implementations of the node trait.

use async_trait::async_trait;
use serde::Deserialize;

use super::{
    block_contents::Transaction,
    consensus_type::{
        sequencing_consensus::SequencingConsensusType,
        validating_consensus::ValidatingConsensusType, ConsensusType,
    },
    election::{ConsensusExchange, ElectionConfig, QuorumExchange, VoteToken},
    network::{NetworkMsg, TestableNetworkingImplementation},
    signature_key::TestableSignatureKey,
    state::{ConsensusTime, TestableBlock, TestableState},
    storage::{StorageError, StorageState, TestableStorage},
    State,
};
use crate::{
    data::LeafType,
    traits::{
        consensus_type::{
            sequencing_consensus::SequencingConsensus, validating_consensus::ValidatingConsensus,
        },
        signature_key::SignatureKey,
        storage::Storage,
        Block,
    },
};
use crate::{data::TestableLeaf, message::Message};

use std::hash::Hash;
use std::{fmt::Debug, marker::PhantomData};

/// Node implementation aggregate trait
///
/// This trait exists to collect multiple behavior implementations into one type, to allow
/// `HotShot` to avoid annoying numbers of type arguments and type patching.
///
/// It is recommended you implement this trait on a zero sized type, as `HotShot`does not actually
/// store or keep a reference to any value implementing this trait.

pub trait NodeImplementation<TYPES: NodeType>: Send + Sync + Debug + Clone + 'static {
    // type Message: NetworkMsg;

    /// Leaf type for this consensus implementation
    type Leaf: LeafType<NodeType = TYPES>;

    /// Storage type for this consensus implementation
    type Storage: Storage<TYPES, Self::Leaf> + Clone;

    /// Consensus type selected exchanges.
    ///
    /// Implements either `ValidatingExchangesType` or `SequencingExchangesType`.
    type Exchanges: ExchangesType<TYPES::ConsensusType, TYPES, Self::Leaf, Message<TYPES, Self>>;
}

// TODO (Keyao) move exchange types to election.rs?
/// Contains the protocols for exchanging proposals and votes.
pub trait ExchangesType<
    CONSENSUS: ConsensusType,
    TYPES: NodeType<ConsensusType = CONSENSUS>,
    LEAF: LeafType<NodeType = TYPES>,
    MESSAGE: NetworkMsg,
>: Send + Sync
{
    /// Protocol for exchanging consensus proposals and votes.
    type QuorumExchange: ConsensusExchange<TYPES, LEAF, MESSAGE>;
}

/// An [`ExchangesType`] for validating consensus.
pub trait ValidatingExchangesType<
    CONSENSUS: ValidatingConsensusType,
    TYPES: NodeType<ConsensusType = CONSENSUS>,
    LEAF: LeafType<NodeType = TYPES>,
    MESSAGE: NetworkMsg,
>: ExchangesType<CONSENSUS, TYPES, LEAF, MESSAGE>
{
}

/// An [`ExchangesType`] for sequencing consensus.
pub trait SequencingExchangesType<
    CONSENSUS: SequencingConsensusType,
    TYPES: NodeType<ConsensusType = CONSENSUS>,
    LEAF: LeafType<NodeType = TYPES>,
    MESSAGE: NetworkMsg,
>: ExchangesType<CONSENSUS, TYPES, LEAF, MESSAGE>
{
    /// Protocol for exchanging data availability proposals and votes.
    type CommitteeExchange: ConsensusExchange<TYPES, LEAF, MESSAGE>;
}

/// Implements [`ValidatingExchangesType`].
#[derive(Clone, Debug)]
pub struct ValidatingExchanges<
    CONSENSUS: ValidatingConsensusType,
    TYPES: NodeType<ConsensusType = CONSENSUS>,
    LEAF: LeafType<NodeType = TYPES>,
    MESSAGE: NetworkMsg,
    QUORUMEXCHANGE: ConsensusExchange<TYPES, LEAF, MESSAGE>,
> {
    /// Phantom data.
    _phantom: PhantomData<(TYPES, LEAF, MESSAGE, QUORUMEXCHANGE)>,
}

impl<CONSENSUS, TYPES, LEAF, MESSAGE, QUORUMEXCHANGE>
    ValidatingExchangesType<CONSENSUS, TYPES, LEAF, MESSAGE>
    for ValidatingExchanges<CONSENSUS, TYPES, LEAF, MESSAGE, QUORUMEXCHANGE>
where
    CONSENSUS: ValidatingConsensusType,
    TYPES: NodeType<ConsensusType = CONSENSUS>,
    LEAF: LeafType<NodeType = TYPES>,
    MESSAGE: NetworkMsg,
    QUORUMEXCHANGE: ConsensusExchange<TYPES, LEAF, MESSAGE>,
{
}

impl<CONSENSUS, TYPES, LEAF, MESSAGE, QUORUMEXCHANGE> ExchangesType<CONSENSUS, TYPES, LEAF, MESSAGE>
    for ValidatingExchanges<CONSENSUS, TYPES, LEAF, MESSAGE, QUORUMEXCHANGE>
where
    CONSENSUS: ValidatingConsensusType,
    TYPES: NodeType<ConsensusType = CONSENSUS>,
    LEAF: LeafType<NodeType = TYPES>,
    MESSAGE: NetworkMsg,
    QUORUMEXCHANGE: ConsensusExchange<TYPES, LEAF, MESSAGE>,
{
    type QuorumExchange = QUORUMEXCHANGE;
}

/// Implementes [`SequencingExchangesType`].
#[derive(Clone, Debug)]
pub struct SequencingExchanges<
    CONSENSUS: SequencingConsensusType,
    TYPES: NodeType<ConsensusType = CONSENSUS>,
    LEAF: LeafType<NodeType = TYPES>,
    MESSAGE: NetworkMsg,
    QUORUMEXCHANGE: ConsensusExchange<TYPES, LEAF, MESSAGE>,
    COMMITTEEEXCHANGE: ConsensusExchange<TYPES, LEAF, MESSAGE>,
> {
    /// Phantom data.
    _phantom: PhantomData<(TYPES, LEAF, MESSAGE, QUORUMEXCHANGE, COMMITTEEEXCHANGE)>,
}

impl<CONSENSUS, TYPES, LEAF, MESSAGE, QUORUMEXCHANGE, COMMITTEEEXCHANGE>
    SequencingExchangesType<CONSENSUS, TYPES, LEAF, MESSAGE>
    for SequencingExchanges<CONSENSUS, TYPES, LEAF, MESSAGE, QUORUMEXCHANGE, COMMITTEEEXCHANGE>
where
    CONSENSUS: SequencingConsensusType,
    TYPES: NodeType<ConsensusType = CONSENSUS>,
    LEAF: LeafType<NodeType = TYPES>,
    MESSAGE: NetworkMsg,
    QUORUMEXCHANGE: ConsensusExchange<TYPES, LEAF, MESSAGE>,
    COMMITTEEEXCHANGE: ConsensusExchange<TYPES, LEAF, MESSAGE>,
{
    type CommitteeExchange = COMMITTEEEXCHANGE;
}

impl<CONSENSUS, TYPES, LEAF, MESSAGE, QUORUMEXCHANGE, COMMITTEEEXCHANGE>
    ExchangesType<CONSENSUS, TYPES, LEAF, MESSAGE>
    for SequencingExchanges<CONSENSUS, TYPES, LEAF, MESSAGE, QUORUMEXCHANGE, COMMITTEEEXCHANGE>
where
    CONSENSUS: SequencingConsensusType,
    TYPES: NodeType<ConsensusType = CONSENSUS>,
    LEAF: LeafType<NodeType = TYPES>,
    MESSAGE: NetworkMsg,
    QUORUMEXCHANGE: ConsensusExchange<TYPES, LEAF, MESSAGE>,
    COMMITTEEEXCHANGE: ConsensusExchange<TYPES, LEAF, MESSAGE>,
{
    type QuorumExchange = QUORUMEXCHANGE;
}

/// Alias for the [`QuorumExchange`] type.
type QuorumEx<TYPES, I> = <<I as NodeImplementation<TYPES>>::Exchanges as ExchangesType<
    <TYPES as NodeType>::ConsensusType,
    TYPES,
    <I as NodeImplementation<TYPES>>::Leaf,
    Message<TYPES, I>,
>>::QuorumExchange;

/// Alias for the [`CommitteeExchange`] type.
type CommitteeEx<TYPES, I> =
    <<I as NodeImplementation<TYPES>>::Exchanges as SequencingExchangesType<
        SequencingConsensus,
        TYPES,
        <I as NodeImplementation<TYPES>>::Leaf,
        Message<TYPES, I>,
    >>::CommitteeExchange;

/// extra functions required on a node implementation to be usable by hotshot-testing
#[allow(clippy::type_complexity)]
#[async_trait]
pub trait TestableNodeImplementation<TYPES: NodeType>: NodeImplementation<TYPES> {
    /// generates a network given an expected node count
    fn committee_generator(
        expected_node_count: usize,
        num_bootstrap: usize,
        network_id: usize,
    ) -> Box<
        dyn Fn(
                u64,
            ) -> <CommitteeEx<TYPES, Self> as ConsensusExchange<
                TYPES,
                Self::Leaf,
                Message<TYPES, Self>,
            >>::Networking
            + 'static,
    >;

    /// generates a network given an expected node count
    fn quorum_generator(
        expected_node_count: usize,
        num_bootstrap: usize,
        network_id: usize,
    ) -> Box<
        dyn Fn(
                u64,
            ) -> <QuorumEx<TYPES, Self> as ConsensusExchange<
                TYPES,
                Self::Leaf,
                Message<TYPES, Self>,
            >>::Networking
            + 'static,
    >;

    /// Creates random transaction if possible
    /// otherwise panics
    /// `padding` is the bytes of padding to add to the transaction
    fn state_create_random_transaction(
        state: Option<&TYPES::StateType>,
        rng: &mut dyn rand::RngCore,
        padding: u64,
    ) -> <TYPES::BlockType as Block>::Transaction;

    /// Creates random transaction if possible
    /// otherwise panics
    /// `padding` is the bytes of padding to add to the transaction
    fn leaf_create_random_transaction(
        leaf: &Self::Leaf,
        rng: &mut dyn rand::RngCore,
        padding: u64,
    ) -> <TYPES::BlockType as Block>::Transaction;

    /// generate a genesis block
    fn block_genesis() -> TYPES::BlockType;

    /// the number of transactions in a block
    fn txn_count(block: &TYPES::BlockType) -> u64;

    /// Create ephemeral storage
    /// Will be deleted/lost immediately after storage is dropped
    /// # Errors
    /// Errors if it is not possible to construct temporary storage.
    fn construct_tmp_storage() -> Result<Self::Storage, StorageError>;

    /// Return the full internal state. This is useful for debugging.
    async fn get_full_state(storage: &Self::Storage) -> StorageState<TYPES, Self::Leaf>;

    /// The private key of the node `id` in a test.
    fn generate_test_key(id: u64) -> <TYPES::SignatureKey as SignatureKey>::PrivateKey;
}

#[async_trait]
impl<TYPES: NodeType, I: NodeImplementation<TYPES>> TestableNodeImplementation<TYPES>
for I
where
<CommitteeEx<TYPES, I> as ConsensusExchange<TYPES, I::Leaf, Message<TYPES, I>>>::Networking :
TestableNetworkingImplementation<
    TYPES,
    Message<TYPES, I>,
    <CommitteeEx<TYPES, I> as ConsensusExchange<TYPES, I::Leaf, Message<TYPES, I>>>::Proposal,
    <CommitteeEx<TYPES, I> as ConsensusExchange<TYPES, I::Leaf, Message<TYPES, I>>>::Vote,
    <CommitteeEx<TYPES, I> as ConsensusExchange<TYPES, I::Leaf, Message<TYPES, I>>>::Membership,
>,
<QuorumEx<TYPES, I> as ConsensusExchange<TYPES, I::Leaf, Message<TYPES, I>>>::Networking :
TestableNetworkingImplementation<
    TYPES,
    Message<TYPES, I>,
    <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, I::Leaf, Message<TYPES, I>>>::Proposal,
    <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, I::Leaf, Message<TYPES, I>>>::Vote,
    <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, I::Leaf, Message<TYPES, I>>>::Membership,
>,
TYPES::StateType : TestableState,
TYPES::BlockType : TestableBlock,
I::Storage : TestableStorage<TYPES, I::Leaf>,
TYPES::SignatureKey : TestableSignatureKey,
I::Leaf : TestableLeaf<NodeType = TYPES>,
{
    fn committee_generator(
        expected_node_count: usize,
        num_bootstrap: usize,
        network_id: usize,
    ) -> Box<dyn Fn(u64) -> <CommitteeEx<TYPES, I> as ConsensusExchange<TYPES, I::Leaf, Message<TYPES, I>>>::Networking + 'static> {
        <CommitteeEx<TYPES, I>::Networking as TestableNetworkingImplementation<_, _, _, _, _>>::generator(expected_node_count, num_bootstrap, network_id)
    }

    fn quorum_generator(
        expected_node_count: usize,
        num_bootstrap: usize,
        network_id: usize,
    ) -> Box<dyn Fn(u64) -> <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, I::Leaf, Message<TYPES, I>>>::Networking + 'static> {
        <QuorumEx<TYPES, I>::Networking as TestableNetworkingImplementation<_, _, _, _, _>>::generator(expected_node_count, num_bootstrap, network_id)
    }

    fn state_create_random_transaction(
        state: Option<&TYPES::StateType>,
        rng: &mut dyn rand::RngCore,
        padding: u64,
    ) -> <TYPES::BlockType as Block>::Transaction {
        <TYPES::StateType as TestableState>::create_random_transaction(state, rng, padding)
    }

    fn leaf_create_random_transaction(
        leaf: &Self::Leaf,
        rng: &mut dyn rand::RngCore,
        padding: u64,
    ) -> <TYPES::BlockType as Block>::Transaction {
        <Self::Leaf as TestableLeaf>::create_random_transaction(leaf, rng, padding)
    }

    fn block_genesis() -> TYPES::BlockType {
        <TYPES::BlockType as TestableBlock>::genesis()
    }

    fn txn_count(block: &TYPES::BlockType) -> u64 {
        <TYPES::BlockType as TestableBlock>::txn_count(block)

    }

    fn construct_tmp_storage() -> Result<Self::Storage, StorageError> {
        <I::Storage as TestableStorage<TYPES, I::Leaf>>::construct_tmp_storage()

    }

    async fn get_full_state(storage: &Self::Storage) -> StorageState<TYPES, Self::Leaf> {
        <I::Storage as TestableStorage<TYPES, I::Leaf>>::get_full_state(storage).await
    }


    fn generate_test_key(id: u64) -> <TYPES::SignatureKey as SignatureKey>::PrivateKey {
        <TYPES::SignatureKey as TestableSignatureKey>::generate_test_key(id)
    }
}

/// A proposal to append a new leaf to the log which is output by consensus.
pub type QuorumProposalType<TYPES, I> = <QuorumEx<TYPES, I> as ConsensusExchange<
    TYPES,
    <I as NodeImplementation<TYPES>>::Leaf,
    Message<TYPES, I>,
>>::Proposal;

/// A proposal to provide data availability for a new leaf.
pub type DAProposalType<TYPES, I> = <CommitteeEx<TYPES, I> as ConsensusExchange<
    TYPES,
    <I as NodeImplementation<TYPES>>::Leaf,
    Message<TYPES, I>,
>>::Proposal;

/// A vote on a [`QuorumProposalType`].
pub type QuorumVoteType<TYPES, I> = <QuorumEx<TYPES, I> as ConsensusExchange<
    TYPES,
    <I as NodeImplementation<TYPES>>::Leaf,
    Message<TYPES, I>,
>>::Vote;

/// A vote on a [`ComitteeProposal`].
pub type CommitteeVote<TYPES, I> = <CommitteeEx<TYPES, I> as ConsensusExchange<
    TYPES,
    <I as NodeImplementation<TYPES>>::Leaf,
    Message<TYPES, I>,
>>::Vote;

/// Networking implementation used to communicate [`QuorumProposalType`] and [`QuorumVote`].
pub type QuorumNetwork<TYPES, I> = <QuorumEx<TYPES, I> as ConsensusExchange<
    TYPES,
    <I as NodeImplementation<TYPES>>::Leaf,
    Message<TYPES, I>,
>>::Networking;

/// Networking implementation used to communicate [`DAProposalType`] and [`CommitteeVote`].
pub type CommitteeNetwork<TYPES, I> = <CommitteeEx<TYPES, I> as ConsensusExchange<
    TYPES,
    <I as NodeImplementation<TYPES>>::Leaf,
    Message<TYPES, I>,
>>::Networking;

/// Protocol for determining membership in a consensus committee.
pub type QuorumMembership<TYPES, I> = <QuorumEx<TYPES, I> as ConsensusExchange<
    TYPES,
    <I as NodeImplementation<TYPES>>::Leaf,
    Message<TYPES, I>,
>>::Membership;

/// Protocol for determining membership in a data availability committee.
pub type CommitteeMembership<TYPES, I> = <CommitteeEx<TYPES, I> as ConsensusExchange<
    TYPES,
    <I as NodeImplementation<TYPES>>::Leaf,
    Message<TYPES, I>,
>>::Membership;

/// Trait with all the type definitions that are used in the current hotshot setup.
pub trait NodeType:
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
    + for<'de> Deserialize<'de>
    + Send
    + Sync
    + 'static
{
    /// the type of consensus (seuqencing or validating)
    type ConsensusType: ConsensusType;
    /// The time type that this hotshot setup is using.
    ///
    /// This should be the same `Time` that `StateType::Time` is using.
    type Time: ConsensusTime;
    /// The block type that this hotshot setup is using.
    ///
    /// This should be the same block that `StateType::BlockType` is using.
    type BlockType: Block<Transaction = Self::Transaction>;
    /// The signature key that this hotshot setup is using.
    type SignatureKey: SignatureKey;
    /// The vote token that this hotshot setup is using.
    type VoteTokenType: VoteToken;
    /// The transaction type that this hotshot setup is using.
    ///
    /// This should be equal to `Block::Transaction`
    type Transaction: Transaction;
    /// The election config type that this hotshot setup is using.
    type ElectionConfigType: ElectionConfig;

    /// The state type that this hotshot setup is using.
    type StateType: State<BlockType = Self::BlockType, Time = Self::Time>;
}
