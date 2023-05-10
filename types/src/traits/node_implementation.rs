//! Composite trait for node behavior
//!
//! This module defines the [`NodeImplementation`] trait, which is a composite trait used for
//! describing the overall behavior of a node, as a composition of implementations of the node trait.

use super::{
    block_contents::Transaction,
    consensus_type::ConsensusType,
    election::{
        CommitteeExchangeType, ConsensusExchange, ElectionConfig, QuorumExchangeType, VoteToken,
    },
    network::{CommunicationChannel, NetworkMsg, TestableNetworkingImplementation},
    signature_key::TestableSignatureKey,
    state::{ConsensusTime, TestableBlock, TestableState},
    storage::{StorageError, StorageState, TestableStorage},
    State,
};
use crate::{data::TestableLeaf, message::Message};
use crate::{
    data::{LeafType, SequencingLeaf, ValidatingLeaf},
    message::{ConsensusMessageType, SequencingMessage, ValidatingMessage},
    traits::{
        consensus_type::{
            sequencing_consensus::SequencingConsensus, validating_consensus::ValidatingConsensus,
        },
        network::TestableChannelImplementation,
        signature_key::SignatureKey,
        storage::Storage,
        Block,
    },
};
use async_trait::async_trait;
use commit::Committable;
use serde::{Deserialize, Serialize};
use std::hash::Hash;
use std::{fmt::Debug, marker::PhantomData, sync::Arc};

/// Node implementation aggregate trait
///
/// This trait exists to collect multiple behavior implementations into one type, to allow
/// `HotShot` to avoid annoying numbers of type arguments and type patching.
///
/// It is recommended you implement this trait on a zero sized type, as `HotShot`does not actually
/// store or keep a reference to any value implementing this trait.

pub trait NodeImplementation<TYPES: NodeType>:
    Send + Sync + Debug + Clone + 'static + Serialize + for<'de> Deserialize<'de>
{
    /// Leaf type for this consensus implementation
    type Leaf: LeafType<NodeType = TYPES>;

    /// Storage type for this consensus implementation
    type Storage: Storage<TYPES, Self::Leaf> + Clone;

    /// Consensus message type.
    type ConsensusMessage: ConsensusMessageType<TYPES, Self>
        + Clone
        + Debug
        + Send
        + Sync
        + 'static
        + for<'a> Deserialize<'a>
        + Serialize;

    /// Consensus type selected exchanges.
    ///
    /// Implements either `ValidatingExchangesType` or `SequencingExchangesType`.
    type Exchanges: ExchangesType<TYPES::ConsensusType, TYPES, Self::Leaf, Message<TYPES, Self>>;
}

// TODO (Keyao) move exchange types to election.rs?
/// Contains the protocols for exchanging proposals and votes.
#[async_trait]
pub trait ExchangesType<
    CONSENSUS: ConsensusType,
    TYPES: NodeType<ConsensusType = CONSENSUS>,
    LEAF: LeafType<NodeType = TYPES>,
    MESSAGE: NetworkMsg,
>: Send + Sync
{
    /// Protocol for exchanging consensus proposals and votes.
    type QuorumExchange: QuorumExchangeType<TYPES, LEAF, MESSAGE> + Clone + Debug;

    /// Networking implementations for all exchanges.
    type Networks;

    /// Create all exchanges.
    fn create(
        keys: Vec<TYPES::SignatureKey>,
        config: TYPES::ElectionConfigType,
        networks: Self::Networks,
        pk: TYPES::SignatureKey,
        sk: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
        ek: jf_primitives::aead::KeyPair,
    ) -> Self;

    /// Get the quorum exchange.
    fn quorum_exchange(&self) -> &Self::QuorumExchange;

    /// Block the underlying networking interfaces until node is successfully initialized into the
    /// networks.
    async fn wait_for_networks_ready(&self);

    /// Shut down the the underlying networking interfaces.
    async fn shut_down_networks(&self);
}

/// An [`ExchangesType`] for validating consensus.
pub trait ValidatingExchangesType<
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    MESSAGE: NetworkMsg,
>: ExchangesType<ValidatingConsensus, TYPES, ValidatingLeaf<TYPES>, MESSAGE>
{
}

/// An [`ExchangesType`] for sequencing consensus.
pub trait SequencingExchangesType<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    MESSAGE: NetworkMsg,
>: ExchangesType<SequencingConsensus, TYPES, SequencingLeaf<TYPES>, MESSAGE>
{
    /// Protocol for exchanging data availability proposals and votes.
    type CommitteeExchange: CommitteeExchangeType<TYPES, MESSAGE> + Clone + Debug;

    /// Get the committee exchange.
    fn committee_exchange(&self) -> &Self::CommitteeExchange;
}

/// Implements [`ValidatingExchangesType`].
#[derive(Clone, Debug)]
pub struct ValidatingExchanges<
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    MESSAGE: NetworkMsg,
    QUORUMEXCHANGE: QuorumExchangeType<TYPES, ValidatingLeaf<TYPES>, MESSAGE>,
> {
    /// Quorum exchange.
    quorum_exchange: QUORUMEXCHANGE,

    /// Phantom data.
    _phantom: PhantomData<(TYPES, MESSAGE)>,
}

impl<TYPES, MESSAGE, QUORUMEXCHANGE> ValidatingExchangesType<TYPES, MESSAGE>
    for ValidatingExchanges<TYPES, MESSAGE, QUORUMEXCHANGE>
where
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    MESSAGE: NetworkMsg,
    QUORUMEXCHANGE: QuorumExchangeType<TYPES, ValidatingLeaf<TYPES>, MESSAGE> + Clone + Debug,
{
}

#[async_trait]
impl<TYPES, MESSAGE, QUORUMEXCHANGE>
    ExchangesType<ValidatingConsensus, TYPES, ValidatingLeaf<TYPES>, MESSAGE>
    for ValidatingExchanges<TYPES, MESSAGE, QUORUMEXCHANGE>
where
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    MESSAGE: NetworkMsg,
    QUORUMEXCHANGE: QuorumExchangeType<TYPES, ValidatingLeaf<TYPES>, MESSAGE> + Clone + Debug,
{
    type QuorumExchange = QUORUMEXCHANGE;
    type Networks = (QUORUMEXCHANGE::Networking, ());

    fn create(
        keys: Vec<TYPES::SignatureKey>,
        config: TYPES::ElectionConfigType,
        networks: Self::Networks,
        pk: TYPES::SignatureKey,
        sk: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
        ek: jf_primitives::aead::KeyPair,
    ) -> Self {
        Self {
            quorum_exchange: QUORUMEXCHANGE::create(keys, config, networks.0, pk, sk, ek),
            _phantom: PhantomData,
        }
    }

    fn quorum_exchange(&self) -> &Self::QuorumExchange {
        &self.quorum_exchange
    }

    async fn wait_for_networks_ready(&self) {
        self.quorum_exchange.network().wait_for_ready().await;
    }

    async fn shut_down_networks(&self) {
        self.quorum_exchange.network().shut_down().await;
    }
}

/// Implementes [`SequencingExchangesType`].
#[derive(Clone, Debug)]
pub struct SequencingExchanges<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    MESSAGE: NetworkMsg,
    QUORUMEXCHANGE: QuorumExchangeType<TYPES, SequencingLeaf<TYPES>, MESSAGE>,
    COMMITTEEEXCHANGE: CommitteeExchangeType<TYPES, MESSAGE>,
> {
    /// Quorum exchange.
    quorum_exchange: QUORUMEXCHANGE,

    /// Committee exchange.
    committee_exchange: COMMITTEEEXCHANGE,

    /// Phantom data.
    _phantom: PhantomData<(TYPES, MESSAGE)>,
}

impl<TYPES, MESSAGE, QUORUMEXCHANGE, COMMITTEEEXCHANGE> SequencingExchangesType<TYPES, MESSAGE>
    for SequencingExchanges<TYPES, MESSAGE, QUORUMEXCHANGE, COMMITTEEEXCHANGE>
where
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    MESSAGE: NetworkMsg,
    QUORUMEXCHANGE: QuorumExchangeType<TYPES, SequencingLeaf<TYPES>, MESSAGE> + Clone + Debug,
    COMMITTEEEXCHANGE: CommitteeExchangeType<TYPES, MESSAGE> + Clone + Debug,
{
    type CommitteeExchange = COMMITTEEEXCHANGE;

    fn committee_exchange(&self) -> &COMMITTEEEXCHANGE {
        &self.committee_exchange
    }
}

#[async_trait]
impl<TYPES, MESSAGE, QUORUMEXCHANGE, COMMITTEEEXCHANGE>
    ExchangesType<SequencingConsensus, TYPES, SequencingLeaf<TYPES>, MESSAGE>
    for SequencingExchanges<TYPES, MESSAGE, QUORUMEXCHANGE, COMMITTEEEXCHANGE>
where
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    MESSAGE: NetworkMsg,
    QUORUMEXCHANGE: QuorumExchangeType<TYPES, SequencingLeaf<TYPES>, MESSAGE> + Clone + Debug,
    COMMITTEEEXCHANGE: CommitteeExchangeType<TYPES, MESSAGE> + Clone,
{
    type QuorumExchange = QUORUMEXCHANGE;
    type Networks = (QUORUMEXCHANGE::Networking, COMMITTEEEXCHANGE::Networking);

    fn create(
        keys: Vec<TYPES::SignatureKey>,
        config: TYPES::ElectionConfigType,
        networks: Self::Networks,
        pk: TYPES::SignatureKey,
        sk: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
        ek: jf_primitives::aead::KeyPair,
    ) -> Self {
        let quorum_exchange = QUORUMEXCHANGE::create(
            keys.clone(),
            config.clone(),
            networks.0,
            pk.clone(),
            sk.clone(),
            ek.clone(),
        );
        let committee_exchange = COMMITTEEEXCHANGE::create(keys, config, networks.1, pk, sk, ek);
        Self {
            quorum_exchange,
            committee_exchange,
            _phantom: PhantomData,
        }
    }

    fn quorum_exchange(&self) -> &Self::QuorumExchange {
        &self.quorum_exchange
    }

    async fn wait_for_networks_ready(&self) {
        self.quorum_exchange.network().wait_for_ready().await;
        self.committee_exchange.network().wait_for_ready().await;
    }

    async fn shut_down_networks(&self) {
        self.quorum_exchange.network().shut_down().await;
        self.committee_exchange.network().shut_down().await;
    }
}

/// Alias for the [`QuorumExchange`] type.
pub type QuorumEx<TYPES, I> = <<I as NodeImplementation<TYPES>>::Exchanges as ExchangesType<
    <TYPES as NodeType>::ConsensusType,
    TYPES,
    <I as NodeImplementation<TYPES>>::Leaf,
    Message<TYPES, I>,
>>::QuorumExchange;

/// Alias for the [`CommitteeExchange`] type for validating consensus.
pub type ValidatingQuorumEx<TYPES, I> =
    <<I as NodeImplementation<TYPES>>::Exchanges as ExchangesType<
        ValidatingConsensus,
        TYPES,
        <I as NodeImplementation<TYPES>>::Leaf,
        Message<TYPES, I>,
    >>::QuorumExchange;

/// Alias for the [`CommitteeExchange`] type for sequencing consensus.
pub type SequencingQuorumEx<TYPES, I> =
    <<I as NodeImplementation<TYPES>>::Exchanges as ExchangesType<
        SequencingConsensus,
        TYPES,
        <I as NodeImplementation<TYPES>>::Leaf,
        Message<TYPES, I>,
    >>::QuorumExchange;

/// Alias for the [`CommitteeExchange`] type.
pub type CommitteeEx<TYPES, I> =
    <<I as NodeImplementation<TYPES>>::Exchanges as SequencingExchangesType<
        TYPES,
        Message<TYPES, I>,
    >>::CommitteeExchange;

/// extra functions required on a node implementation to be usable by hotshot-testing
#[allow(clippy::type_complexity)]
#[async_trait]
pub trait TestableNodeImplementation<
    CONSENSUS: ConsensusType,
    TYPES: NodeType<ConsensusType = CONSENSUS>,
>: NodeImplementation<TYPES>
{
    /// Network for communications in the DA committee. Only needed for the sequencing consensus.
    type CommitteeNetwork;
    type CommitteeNetworkType;

    // TODO (Keyao) Is it better to move this where clause to the `TestableNodeImplementation` trait?
    /// Generate a quorum network given an expected node count.
    fn quorum_network_generator(
        expected_node_count: usize,
        num_bootstrap: usize,
    ) -> Box<dyn Fn(u64) -> QuorumNetworkType<TYPES, Self> + 'static>
    where
        QuorumNetwork<TYPES, Self>: CommunicationChannel<
            TYPES,
            Message<TYPES, Self>,
            <QuorumEx<TYPES, Self> as ConsensusExchange<TYPES, Message<TYPES, Self>>>::Proposal,
            <QuorumEx<TYPES, Self> as ConsensusExchange<TYPES, Message<TYPES, Self>>>::Vote,
            <QuorumEx<TYPES, Self> as ConsensusExchange<TYPES, Message<TYPES, Self>>>::Membership,
        >;

    /// Generate a committee network given an expected node count.
    fn committee_network_generator(
        expected_node_count: usize,
        num_bootstrap: usize,
    ) -> Box<dyn Fn(u64) -> Self::CommitteeNetworkType>;

    /// Generate a quorum communication channel given the network.
    fn quorum_comm_channel_generator(
    ) -> Box<dyn Fn(Arc<QuorumNetworkType<TYPES, Self>>) -> QuorumNetwork<TYPES, Self> + 'static>
    where
        QuorumNetwork<TYPES, Self>: CommunicationChannel<
            TYPES,
            Message<TYPES, Self>,
            <QuorumEx<TYPES, Self> as ConsensusExchange<TYPES, Message<TYPES, Self>>>::Proposal,
            <QuorumEx<TYPES, Self> as ConsensusExchange<TYPES, Message<TYPES, Self>>>::Vote,
            <QuorumEx<TYPES, Self> as ConsensusExchange<TYPES, Message<TYPES, Self>>>::Membership,
        >;

    /// Generate a committee communication channel given the network.
    fn committee_comm_channel_generator(
    ) -> Box<dyn Fn(Arc<Self::CommitteeNetworkType>) -> Self::CommitteeNetwork + 'static>;

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
impl<
        TYPES: NodeType<ConsensusType = ValidatingConsensus>,
        I: NodeImplementation<TYPES, ConsensusMessage = ValidatingMessage<TYPES, Self>>,
    > TestableNodeImplementation<ValidatingConsensus, TYPES> for I
where
    <I as NodeImplementation<TYPES>>::Exchanges: ValidatingExchangesType<TYPES, Message<TYPES, I>>,
    QuorumNetworkType<TYPES, I>: TestableNetworkingImplementation<TYPES, Message<TYPES, I>>,
    QuorumNetwork<TYPES, I>: TestableChannelImplementation<
        TYPES,
        Message<TYPES, I>,
        QuorumProposalType<TYPES, I>,
        QuorumVoteType<TYPES, I>,
        QuorumMembership<TYPES, I>,
        QuorumNetworkType<TYPES, I>,
    >,
    TYPES::StateType: TestableState,
    TYPES::BlockType: TestableBlock,
    I::Storage: TestableStorage<TYPES, I::Leaf>,
    TYPES::SignatureKey: TestableSignatureKey,
    I::Leaf: TestableLeaf<NodeType = TYPES>,
{
    type CommitteeNetwork = ();
    type CommitteeNetworkType = ();

    fn quorum_network_generator(
        expected_node_count: usize,
        num_bootstrap: usize,
    ) -> Box<dyn Fn(u64) -> QuorumNetworkType<TYPES, I> + 'static> {
        <QuorumNetworkType<TYPES, I> as TestableNetworkingImplementation<_, _>>::generator(
            expected_node_count,
            num_bootstrap,
            1,
        )
    }

    fn committee_network_generator(
        _expected_node_count: usize,
        _num_bootstrap: usize,
    ) -> Box<dyn Fn(u64) -> Self::CommitteeNetworkType + 'static> {
        Box::new(|_| ())
    }

    fn quorum_comm_channel_generator(
    ) -> Box<dyn Fn(Arc<QuorumNetworkType<TYPES, I>>) -> QuorumNetwork<TYPES, Self> + 'static> {
        < QuorumNetwork::<TYPES, Self> as TestableChannelImplementation<_, _, _, _, _, _>>::generate_network()
    }

    fn committee_comm_channel_generator(
    ) -> Box<dyn Fn(Arc<Self::CommitteeNetworkType>) -> Self::CommitteeNetwork + 'static> {
        Box::new(|_| ())
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

#[async_trait]
impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<TYPES, ConsensusMessage = SequencingMessage<TYPES, Self>>,
    > TestableNodeImplementation<SequencingConsensus, TYPES> for I
where
    <I as NodeImplementation<TYPES>>::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    CommitteeNetworkType<TYPES, I>: TestableNetworkingImplementation<TYPES, Message<TYPES, I>>,
    QuorumNetworkType<TYPES, I>: TestableNetworkingImplementation<TYPES, Message<TYPES, I>>,
    QuorumNetwork<TYPES, I>: TestableChannelImplementation<
        TYPES,
        Message<TYPES, I>,
        QuorumProposalType<TYPES, I>,
        QuorumVoteType<TYPES, I>,
        QuorumMembership<TYPES, I>,
        QuorumNetworkType<TYPES, I>,
    >,
    CommitteeNetwork<TYPES, I>: TestableChannelImplementation<
        TYPES,
        Message<TYPES, I>,
        DAProposalType<TYPES, I>,
        CommitteeVote<TYPES, I>,
        CommitteeMembership<TYPES, I>,
        CommitteeNetworkType<TYPES, I>,
    >,
    TYPES::StateType: TestableState,
    TYPES::BlockType: TestableBlock,
    I::Storage: TestableStorage<TYPES, I::Leaf>,
    TYPES::SignatureKey: TestableSignatureKey,
    I::Leaf: TestableLeaf<NodeType = TYPES>,
{
    type CommitteeNetwork = CommitteeNetwork<TYPES, I>;
    type CommitteeNetworkType = CommitteeNetworkType<TYPES, I>;

    fn quorum_network_generator(
        expected_node_count: usize,
        num_bootstrap: usize,
    ) -> Box<dyn Fn(u64) -> QuorumNetworkType<TYPES, I> + 'static> {
        <QuorumNetworkType<TYPES, I> as TestableNetworkingImplementation<_, _>>::generator(
            expected_node_count,
            num_bootstrap,
            1,
        )
    }

    fn committee_network_generator(
        expected_node_count: usize,
        num_bootstrap: usize,
    ) -> Box<dyn Fn(u64) -> CommitteeNetworkType<TYPES, I> + 'static> {
        <CommitteeNetworkType<TYPES, I> as TestableNetworkingImplementation<_, _>>::generator(
            expected_node_count,
            num_bootstrap,
            1,
        )
    }

    fn quorum_comm_channel_generator(
    ) -> Box<dyn Fn(Arc<QuorumNetworkType<TYPES, I>>) -> QuorumNetwork<TYPES, Self> + 'static> {
        < QuorumNetwork::<TYPES, Self> as TestableChannelImplementation<_, _, _, _, _, _>>::generate_network()
    }

    fn committee_comm_channel_generator(
    ) -> Box<dyn Fn(Arc<CommitteeNetworkType<TYPES, I>>) -> Self::CommitteeNetwork + 'static> {
        < CommitteeNetwork<TYPES, I>as TestableChannelImplementation<_, _, _, _, _, _>>::generate_network()
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
pub type QuorumProposalType<TYPES, I> =
    <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal;

/// A proposal to provide data availability for a new leaf.
pub type DAProposalType<TYPES, I> =
    <CommitteeEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal;

/// A vote on a [`QuorumProposalType`].
pub type QuorumVoteType<TYPES, I> =
    <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote;

/// A vote on a [`ComitteeProposal`].
pub type CommitteeVote<TYPES, I> =
    <CommitteeEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote;

/// Networking implementation used to communicate [`QuorumProposalType`] and [`QuorumVote`].
pub type QuorumNetwork<TYPES, I> =
    <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Networking;

/// Networking implementation used to communicate [`DAProposalType`] and [`DAVote`].
pub type CommitteeNetwork<TYPES, I> =
    <CommitteeEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Networking;

/// Protocol for determining membership in a consensus committee.
pub type QuorumMembership<TYPES, I> =
    <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership;

/// Protocol for determining membership in a DA committee.
pub type CommitteeMembership<TYPES, I> =
    <CommitteeEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership;

/// Type for the underlying quorum `ConnectedNetwork` that will be shared (for now) b/t Communication Channels
// TODO (Keyao) Rename
pub type QuorumNetworkType<TYPES, I> = <QuorumNetwork<TYPES, I> as CommunicationChannel<
    TYPES,
    Message<TYPES, I>,
    QuorumProposalType<TYPES, I>,
    QuorumVoteType<TYPES, I>,
    QuorumMembership<TYPES, I>,
>>::NETWORK;

/// Type for the underlying committee `ConnectedNetwork` that will be shared (for now) b/t Communication Channels
// TODO (Keyao) Rename
pub type CommitteeNetworkType<TYPES, I> = <CommitteeNetwork<TYPES, I> as CommunicationChannel<
    TYPES,
    Message<TYPES, I>,
    DAProposalType<TYPES, I>,
    CommitteeVote<TYPES, I>,
    CommitteeMembership<TYPES, I>,
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
    type ConsensusType: ConsensusType + Debug;
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
