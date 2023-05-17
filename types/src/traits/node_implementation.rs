//! Composite trait for node behavior
//!
//! This module defines the [`NodeImplementation`] trait, which is a composite trait used for
//! describing the overall behavior of a node, as a composition of implementations of the node trait.

use async_trait::async_trait;
use serde::Deserialize;

use super::{
    block_contents::Transaction,
    election::{ConsensusExchange, ElectionConfig, VoteToken},
    network::{
        CommunicationChannel, TestableChannelImplementation, TestableNetworkingImplementation,
    },
    signature_key::TestableSignatureKey,
    state::{ConsensusTime, ConsensusType, TestableBlock, TestableState},
    storage::{StorageError, StorageState, TestableStorage},
    State,
};
use crate::{
    data::LeafType,
    traits::{signature_key::SignatureKey, storage::Storage, Block},
};
use crate::{data::TestableLeaf, message::Message};
use commit::Committable;

use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

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

    /// Protocol for exchanging consensus proposals and votes.
    type QuorumExchange: ConsensusExchange<TYPES, Message<TYPES, Self>>;

    /// Protocol for exchanging data availability proposals and votes.
    type CommitteeExchange: ConsensusExchange<TYPES, Message<TYPES, Self>>;
}

/// extra functions required on a node implementation to be usable by hotshot-testing
#[allow(clippy::type_complexity)]
#[async_trait]
pub trait TestableNodeImplementation<TYPES: NodeType>: NodeImplementation<TYPES> {
    /// generates a network given an expected node count
    fn network_generator(
        expected_node_count: usize,
        num_bootstrap_nodes: usize,
    ) -> Box<dyn Fn(u64) -> NetworkType<TYPES, Self> + 'static>;
    /// generates a committee communication channel given the network
    fn committee_generator() -> Box<
        dyn Fn(
                Arc<NetworkType<TYPES, Self>>,
            ) -> <Self::CommitteeExchange as ConsensusExchange<
                TYPES,
                Message<TYPES, Self>,
            >>::Networking
            + 'static,
    >;

    /// generates a quorum communication channel given the network
    fn quorum_generator() -> Box<
        dyn Fn(
                Arc<NetworkType<TYPES, Self>>,
            ) -> <Self::QuorumExchange as ConsensusExchange<
                TYPES,
                Message<TYPES, Self>,
            >>::Networking
            + 'static,
    >;

    /// Get the number of messages in-flight from quorum exchange.
    ///
    /// Some implementations will not be able to tell how many messages there are in-flight. These implementations should return `None`.
    // fn quorum_in_flight_message_count(
    //     network: &<Self::QuorumExchange as ConsensusExchange<
    //         TYPES,
    //         Message<TYPES, Self>,
    //     >>::Networking,
    // ) -> Option<usize>;

    /// Get the number of messages in-flight from committee exchange.
    ///
    /// Some implementations will not be able to tell how many messages there are in-flight. These implementations should return `None`.
    // fn committee_in_flight_message_count(
    //     network: &<Self::CommitteeExchange as ConsensusExchange<
    //         TYPES,
    //         Message<TYPES, Self>,
    //     >>::Networking,
    // ) -> Option<usize>;

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
impl<TYPES: NodeType, I: NodeImplementation<TYPES>> TestableNodeImplementation<TYPES> for I
where
    <I::CommitteeExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Networking:
        TestableChannelImplementation<
            TYPES,
            Message<TYPES, I>,
            <I::CommitteeExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
            <I::CommitteeExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
            <I::CommitteeExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
            NetworkType<TYPES, I>,
        >,
    <I::QuorumExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Networking:
        TestableChannelImplementation<
            TYPES,
            Message<TYPES, I>,
            <I::QuorumExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
            <I::QuorumExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
            <I::QuorumExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
            NetworkType<TYPES, I>,
        >,
    NetworkType<TYPES, I>: TestableNetworkingImplementation<TYPES, Message<TYPES, I>>,
    TYPES::StateType: TestableState,
    TYPES::BlockType: TestableBlock,
    I::Storage: TestableStorage<TYPES, I::Leaf>,
    TYPES::SignatureKey: TestableSignatureKey,
    I::Leaf: TestableLeaf<NodeType = TYPES>,
{
    fn network_generator(
        expected_node_count: usize,
        num_bootstrap_nodes: usize,
    ) -> Box<dyn Fn(u64) -> NetworkType<TYPES, I> + 'static> {
        <NetworkType<TYPES, I> as TestableNetworkingImplementation<_, _>>::generator(
            expected_node_count,
            num_bootstrap_nodes,
            1,
        )
    }
    fn committee_generator() -> Box<
        dyn Fn(
                Arc<NetworkType<TYPES, I>>,
            )
                -> <I::CommitteeExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Networking
            + 'static,
    > {
        <<I::CommitteeExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Networking as TestableChannelImplementation<_, _, _, _, _, _>>::generate_network()
    }

    fn quorum_generator() -> Box<
        dyn Fn(
                Arc<NetworkType<TYPES, I>>,
            )
                -> <I::QuorumExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Networking
            + 'static,
    > {
        <<I::QuorumExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Networking as TestableChannelImplementation<_, _, _, _, _, _>>::generate_network()
    }

    // fn quorum_in_flight_message_count(
    //     network: &<I::QuorumExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Networking,
    // ) -> Option<usize> {
    //     <<I::QuorumExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Networking as TestableNetworkingImplementation<_, _, _, _, _>>::in_flight_message_count(network)
    // }

    // fn committee_in_flight_message_count(
    //     network: &<I::CommitteeExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Networking,
    // ) -> Option<usize> {
    //     <<I::CommitteeExchange as ConsensusExchange<TYPES, Message<TYPES, I>>>::Networking as TestableNetworkingImplementation<_, _, _, _, _>>::in_flight_message_count(network)
    // }

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
pub type QuorumProposalType<TYPES, I> =
    <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
    >>::Proposal;

/// A proposal to provide data availability for a new leaf.
pub type CommitteeProposal<TYPES, I> =
    <<I as NodeImplementation<TYPES>>::CommitteeExchange as ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
    >>::Proposal;

/// A vote on a [`QuorumProposal`].
pub type QuorumVoteType<TYPES, I> =
    <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
    >>::Vote;

/// A vote on a [`ComitteeProposal`].
pub type CommitteeVote<TYPES, I> =
    <<I as NodeImplementation<TYPES>>::CommitteeExchange as ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
    >>::Vote;

/// Networking implementation used to communicate [`QuorumProposal`] and [`QuorumVote`].
pub type QuorumNetwork<TYPES, I> =
    <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
    >>::Networking;

/// Networking implementation used to communicate [`CommitteeProposal`] and [`CommitteeVote`].
pub type CommitteeNetwork<TYPES, I> =
    <<I as NodeImplementation<TYPES>>::CommitteeExchange as ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
    >>::Networking;

/// Protocol for determining membership in a consensus committee.
pub type QuorumMembership<TYPES, I> =
    <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
    >>::Membership;

/// Protocol for determining membership in a data availability committee.
pub type CommitteeMembership<TYPES, I> =
    <<I as NodeImplementation<TYPES>>::CommitteeExchange as ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
    >>::Membership;
// TODO remove this really ugly type once we have `ConsensusExchanges` impl and can use that to do all generation
/// Type for the underlying `ConnectedNetwork` that will be shared (for now) b/t Communication Channels
pub type NetworkType<TYPES, I> =
    <<<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
    >>::Networking as CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
            TYPES,
            Message<TYPES, I>,
        >>::Proposal,
        <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
            TYPES,
            Message<TYPES, I>,
        >>::Vote,
        <<I as NodeImplementation<TYPES>>::QuorumExchange as ConsensusExchange<
            TYPES,
            Message<TYPES, I>,
        >>::Membership,
    >>::NETWORK;
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
    type Time: ConsensusTime + Committable;
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
