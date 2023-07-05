//! Composite trait for node behavior
//!
//! This module defines the [`NodeImplementation`] trait, which is a composite trait used for
//! describing the overall behavior of a node, as a composition of implementations of the node trait.

use super::{
    block_contents::Transaction,
    consensus_type::ConsensusType,
    election::{
        CommitteeExchangeType, ConsensusExchange, ElectionConfig, QuorumExchangeType,
        ViewSyncExchange, ViewSyncExchangeType, VoteToken,
    },
    network::{CommunicationChannel, NetworkMsg, TestableNetworkingImplementation},
    signature_key::TestableSignatureKey,
    state::{ConsensusTime, TestableBlock, TestableState},
    storage::{StorageError, StorageState, TestableStorage},
    State,
};
use crate::vote::ViewSyncVote;
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
use async_compatibility_layer::channel::{unbounded, UnboundedReceiver, UnboundedSender};
use async_lock::{Mutex, RwLock};
use async_trait::async_trait;
use commit::Committable;
use serde::{Deserialize, Serialize};
use std::hash::Hash;
use std::{
    collections::BTreeMap,
    fmt::Debug,
    marker::PhantomData,
    sync::{atomic::AtomicBool, Arc},
};

/// Alias for the [`ProcessedConsensusMessage`] type of a [`NodeImplementation`].
type ProcessedConsensusMessageType<TYPES, I> = <<I as NodeImplementation<TYPES>>::ConsensusMessage as ConsensusMessageType<TYPES, I>>::ProcessedConsensusMessage;

/// struct containing messages for a view to send to a replica or DA committee member.
#[derive(Clone)]
pub struct ViewQueue<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// to send networking events to a replica or DA committee member.
    pub sender_chan: UnboundedSender<ProcessedConsensusMessageType<TYPES, I>>,

    /// to recv networking events for a replica or DA committee member.
    pub receiver_chan: Arc<Mutex<UnboundedReceiver<ProcessedConsensusMessageType<TYPES, I>>>>,

    /// `true` if this queue has already received a proposal
    pub has_received_proposal: Arc<AtomicBool>,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> Default for ViewQueue<TYPES, I> {
    /// create new view queue
    fn default() -> Self {
        let (s, r) = unbounded();
        ViewQueue {
            sender_chan: s,
            receiver_chan: Arc::new(Mutex::new(r)),
            has_received_proposal: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// metadata for sending information to the leader, replica, or DA committee member.
pub struct SendToTasks<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// the current view number
    /// this should always be in sync with `Consensus`
    pub cur_view: TYPES::Time,

    /// a map from view number to ViewQueue
    /// one of (replica|next leader)'s' task for view i will be listening on the channel in here
    pub channel_map: BTreeMap<TYPES::Time, ViewQueue<TYPES, I>>,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> SendToTasks<TYPES, I> {
    /// create new sendtosasks
    #[must_use]
    pub fn new(view_num: TYPES::Time) -> Self {
        SendToTasks {
            cur_view: view_num,
            channel_map: BTreeMap::default(),
        }
    }
}

/// Channels for sending/recv-ing proposals and votes.
#[derive(Clone)]
pub struct ChannelMaps<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Channel for the next consensus leader or DA leader.
    pub proposal_channel: Arc<RwLock<SendToTasks<TYPES, I>>>,

    /// Channel for the replica or DA committee member.
    pub vote_channel: Arc<RwLock<SendToTasks<TYPES, I>>>,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> ChannelMaps<TYPES, I> {
    /// Create channels starting from a given view.
    pub fn new(start_view: TYPES::Time) -> Self {
        Self {
            proposal_channel: Arc::new(RwLock::new(SendToTasks::new(start_view))),
            vote_channel: Arc::new(RwLock::new(SendToTasks::new(start_view))),
        }
    }
}

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

    /// Create channels for sending/recv-ing proposals and votes for quorum and committee
    /// exchanges, the latter of which is only applicable for sequencing consensus.
    fn new_channel_maps(
        start_view: TYPES::Time,
    ) -> (ChannelMaps<TYPES, Self>, Option<ChannelMaps<TYPES, Self>>);
}

/// Contains the protocols for exchanging proposals and votes.
#[async_trait]
pub trait ExchangesType<
    CONSENSUS: ConsensusType,
    TYPES: NodeType<ConsensusType = CONSENSUS>,
    LEAF: LeafType<NodeType = TYPES>,
    MESSAGE: NetworkMsg,
>: Send + Sync
{
    /// Protocol for exchanging quorum proposals and votes.
    type QuorumExchange: QuorumExchangeType<TYPES, LEAF, MESSAGE> + Clone + Debug;

    type ViewSyncExchange: ViewSyncExchangeType<TYPES, MESSAGE> + Clone + Debug;

    /// Networking implementations for quorum and committee exchanges, the latter of which is only
    /// applicable for sequencing consensus.
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

    fn view_sync_exchange(&self) -> &Self::ViewSyncExchange;

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
    VIEWSYNCEXCHANGE: ViewSyncExchangeType<TYPES, MESSAGE>,
> {
    /// Quorum exchange.
    quorum_exchange: QUORUMEXCHANGE,

    view_sync_exchange: VIEWSYNCEXCHANGE,

    /// Phantom data.
    _phantom: PhantomData<(TYPES, MESSAGE)>,
}

impl<TYPES, MESSAGE, QUORUMEXCHANGE, VIEWSYNCEXCHANGE> ValidatingExchangesType<TYPES, MESSAGE>
    for ValidatingExchanges<TYPES, MESSAGE, QUORUMEXCHANGE, VIEWSYNCEXCHANGE>
where
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    MESSAGE: NetworkMsg,
    QUORUMEXCHANGE: QuorumExchangeType<TYPES, ValidatingLeaf<TYPES>, MESSAGE> + Clone + Debug,
    VIEWSYNCEXCHANGE: ViewSyncExchangeType<TYPES, MESSAGE> + Clone + Debug,
{
}

#[async_trait]
impl<TYPES, MESSAGE, QUORUMEXCHANGE, VIEWSYNCEXCHANGE>
    ExchangesType<ValidatingConsensus, TYPES, ValidatingLeaf<TYPES>, MESSAGE>
    for ValidatingExchanges<TYPES, MESSAGE, QUORUMEXCHANGE, VIEWSYNCEXCHANGE>
where
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    MESSAGE: NetworkMsg,
    QUORUMEXCHANGE: QuorumExchangeType<TYPES, ValidatingLeaf<TYPES>, MESSAGE> + Clone + Debug,
    VIEWSYNCEXCHANGE: ViewSyncExchangeType<TYPES, MESSAGE> + Clone + Debug,
{
    type QuorumExchange = QUORUMEXCHANGE;
    type ViewSyncExchange = VIEWSYNCEXCHANGE;
    type Networks = (QUORUMEXCHANGE::Networking, VIEWSYNCEXCHANGE::Networking, ());

    fn create(
        keys: Vec<TYPES::SignatureKey>,
        config: TYPES::ElectionConfigType,
        networks: Self::Networks,
        pk: TYPES::SignatureKey,
        sk: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
        ek: jf_primitives::aead::KeyPair,
    ) -> Self {
        Self {
            quorum_exchange: QUORUMEXCHANGE::create(
                keys.clone(),
                config.clone(),
                networks.0,
                pk.clone(),
                sk.clone(),
                ek.clone(),
            ),
            view_sync_exchange: VIEWSYNCEXCHANGE::create(keys, config, networks.1, pk, sk, ek),
            _phantom: PhantomData,
        }
    }

    fn quorum_exchange(&self) -> &Self::QuorumExchange {
        &self.quorum_exchange
    }

    fn view_sync_exchange(&self) -> &Self::ViewSyncExchange {
        &self.view_sync_exchange
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
    VIEWSYNCEXCHANGE: ViewSyncExchangeType<TYPES, MESSAGE>,
> {
    /// Quorum exchange.
    quorum_exchange: QUORUMEXCHANGE,

    view_sync_exchange: VIEWSYNCEXCHANGE,

    /// Committee exchange.
    committee_exchange: COMMITTEEEXCHANGE,

    /// Phantom data.
    _phantom: PhantomData<(TYPES, MESSAGE)>,
}

impl<TYPES, MESSAGE, QUORUMEXCHANGE, COMMITTEEEXCHANGE, VIEWSYNCEXCHANGE>
    SequencingExchangesType<TYPES, MESSAGE>
    for SequencingExchanges<TYPES, MESSAGE, QUORUMEXCHANGE, COMMITTEEEXCHANGE, VIEWSYNCEXCHANGE>
where
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    MESSAGE: NetworkMsg,
    QUORUMEXCHANGE: QuorumExchangeType<TYPES, SequencingLeaf<TYPES>, MESSAGE> + Clone + Debug,
    COMMITTEEEXCHANGE: CommitteeExchangeType<TYPES, MESSAGE> + Clone + Debug,
    VIEWSYNCEXCHANGE: ViewSyncExchangeType<TYPES, MESSAGE> + Clone + Debug,
{
    type CommitteeExchange = COMMITTEEEXCHANGE;

    fn committee_exchange(&self) -> &COMMITTEEEXCHANGE {
        &self.committee_exchange
    }
}

#[async_trait]
impl<TYPES, MESSAGE, QUORUMEXCHANGE, COMMITTEEEXCHANGE, VIEWSYNCEXCHANGE>
    ExchangesType<SequencingConsensus, TYPES, SequencingLeaf<TYPES>, MESSAGE>
    for SequencingExchanges<TYPES, MESSAGE, QUORUMEXCHANGE, COMMITTEEEXCHANGE, VIEWSYNCEXCHANGE>
where
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    MESSAGE: NetworkMsg,
    QUORUMEXCHANGE: QuorumExchangeType<TYPES, SequencingLeaf<TYPES>, MESSAGE> + Clone + Debug,
    COMMITTEEEXCHANGE: CommitteeExchangeType<TYPES, MESSAGE> + Clone,
    VIEWSYNCEXCHANGE: ViewSyncExchangeType<TYPES, MESSAGE> + Clone + Debug,
{
    type QuorumExchange = QUORUMEXCHANGE;
    type ViewSyncExchange = VIEWSYNCEXCHANGE;
    type Networks = (
        QUORUMEXCHANGE::Networking,
        VIEWSYNCEXCHANGE::Networking,
        COMMITTEEEXCHANGE::Networking,
    );

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
        let view_sync_exchange = VIEWSYNCEXCHANGE::create(
            keys.clone(),
            config.clone(),
            networks.1,
            pk.clone(),
            sk.clone(),
            ek.clone(),
        );
        let committee_exchange = COMMITTEEEXCHANGE::create(keys, config, networks.2, pk, sk, ek);

        Self {
            quorum_exchange,
            committee_exchange,
            view_sync_exchange,
            _phantom: PhantomData,
        }
    }

    fn quorum_exchange(&self) -> &Self::QuorumExchange {
        &self.quorum_exchange
    }

    fn view_sync_exchange(&self) -> &Self::ViewSyncExchange {
        &self.view_sync_exchange
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

pub type ViewSyncEx<TYPES, I> = <<I as NodeImplementation<TYPES>>::Exchanges as ExchangesType<
    <TYPES as NodeType>::ConsensusType,
    TYPES,
    <I as NodeImplementation<TYPES>>::Leaf,
    Message<TYPES, I>,
>>::ViewSyncExchange;

/// extra functions required on a node implementation to be usable by hotshot-testing
#[allow(clippy::type_complexity)]
#[async_trait]
pub trait TestableNodeImplementation<
    CONSENSUS: ConsensusType,
    TYPES: NodeType<ConsensusType = CONSENSUS>,
>: NodeImplementation<TYPES>
{
    /// Communication channel the DA committee. Only needed for the sequencing consensus.
    type CommitteeCommChannel;

    /// Connected network for the DA committee. Only needed for the sequencing consensus.
    type CommitteeNetwork;

    type ViewSyncCommChannel;
    type ViewSyncNetwork;

    /// Generate a quorum network given an expected node count.
    fn network_generator(
        expected_node_count: usize,
        num_bootstrap: usize,
        da_committee_size: usize,
    ) -> Box<dyn Fn(u64) -> QuorumNetwork<TYPES, Self> + 'static>
    where
        QuorumCommChannel<TYPES, Self>: CommunicationChannel<
            TYPES,
            Message<TYPES, Self>,
            <QuorumEx<TYPES, Self> as ConsensusExchange<TYPES, Message<TYPES, Self>>>::Proposal,
            <QuorumEx<TYPES, Self> as ConsensusExchange<TYPES, Message<TYPES, Self>>>::Vote,
            <QuorumEx<TYPES, Self> as ConsensusExchange<TYPES, Message<TYPES, Self>>>::Membership,
        >;

    /// Generate a quorum communication channel given the network.
    fn quorum_comm_channel_generator(
    ) -> Box<dyn Fn(Arc<QuorumNetwork<TYPES, Self>>) -> QuorumCommChannel<TYPES, Self> + 'static>
    where
        QuorumCommChannel<TYPES, Self>: CommunicationChannel<
            TYPES,
            Message<TYPES, Self>,
            <QuorumEx<TYPES, Self> as ConsensusExchange<TYPES, Message<TYPES, Self>>>::Proposal,
            <QuorumEx<TYPES, Self> as ConsensusExchange<TYPES, Message<TYPES, Self>>>::Vote,
            <QuorumEx<TYPES, Self> as ConsensusExchange<TYPES, Message<TYPES, Self>>>::Membership,
        >;

    /// Generate a committee communication channel given the network.
    fn committee_comm_channel_generator(
    ) -> Box<dyn Fn(Arc<QuorumNetwork<TYPES, Self>>) -> Self::CommitteeCommChannel + 'static>;

    fn view_sync_comm_channel_generator(
    ) -> Box<dyn Fn(Arc<QuorumNetwork<TYPES, Self>>) -> Self::ViewSyncCommChannel + 'static>;

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
    QuorumNetwork<TYPES, I>: TestableNetworkingImplementation<TYPES, Message<TYPES, I>>,
    QuorumCommChannel<TYPES, I>: TestableChannelImplementation<
        TYPES,
        Message<TYPES, I>,
        QuorumProposalType<TYPES, I>,
        QuorumVoteType<TYPES, I>,
        QuorumMembership<TYPES, I>,
        QuorumNetwork<TYPES, I>,
    >,
    ViewSyncCommChannel<TYPES, I>: TestableChannelImplementation<
        TYPES,
        Message<TYPES, I>,
        QuorumProposalType<TYPES, I>,
        ViewSyncVoteType<TYPES, I>,
        QuorumMembership<TYPES, I>,
        QuorumNetwork<TYPES, I>,
    >,
    TYPES::StateType: TestableState,
    TYPES::BlockType: TestableBlock,
    I::Storage: TestableStorage<TYPES, I::Leaf>,
    TYPES::SignatureKey: TestableSignatureKey,
    I::Leaf: TestableLeaf<NodeType = TYPES>,
{
    type CommitteeCommChannel = ();
    type CommitteeNetwork = ();

    type ViewSyncCommChannel = ViewSyncCommChannel<TYPES, I>;
    type ViewSyncNetwork = QuorumNetwork<TYPES, I>;

    fn network_generator(
        expected_node_count: usize,
        num_bootstrap: usize,
        da_committee_size: usize,
    ) -> Box<dyn Fn(u64) -> QuorumNetwork<TYPES, I> + 'static> {
        <QuorumNetwork<TYPES, I> as TestableNetworkingImplementation<_, _>>::generator(
            expected_node_count,
            num_bootstrap,
            1,
            da_committee_size,
        )
    }

    fn quorum_comm_channel_generator(
    ) -> Box<dyn Fn(Arc<QuorumNetwork<TYPES, I>>) -> QuorumCommChannel<TYPES, Self> + 'static> {
        < QuorumCommChannel::<TYPES, Self> as TestableChannelImplementation<_, _, _, _, _, _>>::generate_network()
    }

    fn committee_comm_channel_generator(
    ) -> Box<dyn Fn(Arc<QuorumNetwork<TYPES, Self>>) -> Self::CommitteeCommChannel + 'static> {
        Box::new(|_| ())
    }

    fn view_sync_comm_channel_generator(
    ) -> Box<dyn Fn(Arc<QuorumNetwork<TYPES, I>>) -> Self::ViewSyncCommChannel + 'static> {
        < ViewSyncCommChannel<TYPES, I> as TestableChannelImplementation<_, _, _, _, _, _>>::generate_network()
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
    CommitteeNetwork<TYPES, I>: TestableNetworkingImplementation<TYPES, Message<TYPES, I>>,
    QuorumNetwork<TYPES, I>: TestableNetworkingImplementation<TYPES, Message<TYPES, I>>,
    QuorumCommChannel<TYPES, I>: TestableChannelImplementation<
        TYPES,
        Message<TYPES, I>,
        QuorumProposalType<TYPES, I>,
        QuorumVoteType<TYPES, I>,
        QuorumMembership<TYPES, I>,
        QuorumNetwork<TYPES, I>,
    >,
    CommitteeCommChannel<TYPES, I>: TestableChannelImplementation<
        TYPES,
        Message<TYPES, I>,
        CommitteeProposalType<TYPES, I>,
        CommitteeVote<TYPES, I>,
        CommitteeMembership<TYPES, I>,
        QuorumNetwork<TYPES, I>,
    >,
    ViewSyncCommChannel<TYPES, I>: TestableChannelImplementation<
        TYPES,
        Message<TYPES, I>,
        QuorumProposalType<TYPES, I>,
        ViewSyncVoteType<TYPES, I>,
        QuorumMembership<TYPES, I>,
        QuorumNetwork<TYPES, I>,
    >,
    TYPES::StateType: TestableState,
    TYPES::BlockType: TestableBlock,
    I::Storage: TestableStorage<TYPES, I::Leaf>,
    TYPES::SignatureKey: TestableSignatureKey,
    I::Leaf: TestableLeaf<NodeType = TYPES>,
{
    type CommitteeCommChannel = CommitteeCommChannel<TYPES, I>;
    type CommitteeNetwork = CommitteeNetwork<TYPES, I>;

    type ViewSyncCommChannel = ViewSyncCommChannel<TYPES, I>;
    type ViewSyncNetwork = QuorumNetwork<TYPES, I>;

    fn network_generator(
        expected_node_count: usize,
        num_bootstrap: usize,
        da_committee_size: usize,
    ) -> Box<dyn Fn(u64) -> QuorumNetwork<TYPES, I> + 'static> {
        <QuorumNetwork<TYPES, I> as TestableNetworkingImplementation<_, _>>::generator(
            expected_node_count,
            num_bootstrap,
            1,
            da_committee_size,
        )
    }

    fn quorum_comm_channel_generator(
    ) -> Box<dyn Fn(Arc<QuorumNetwork<TYPES, I>>) -> QuorumCommChannel<TYPES, Self> + 'static> {
        < QuorumCommChannel::<TYPES, Self> as TestableChannelImplementation<_, _, _, _, _, _>>::generate_network()
    }

    fn committee_comm_channel_generator(
    ) -> Box<dyn Fn(Arc<QuorumNetwork<TYPES, I>>) -> Self::CommitteeCommChannel + 'static> {
        < CommitteeCommChannel<TYPES, I> as TestableChannelImplementation<_, _, _, _, _, _>>::generate_network()
    }

    fn view_sync_comm_channel_generator(
    ) -> Box<dyn Fn(Arc<QuorumNetwork<TYPES, I>>) -> Self::ViewSyncCommChannel + 'static> {
        < ViewSyncCommChannel<TYPES, I> as TestableChannelImplementation<_, _, _, _, _, _>>::generate_network()
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
pub type CommitteeProposalType<TYPES, I> =
    <CommitteeEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal;

/// A vote on a [`QuorumProposalType`].
pub type QuorumVoteType<TYPES, I> =
    <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote;

/// A vote on a [`ComitteeProposal`].
pub type CommitteeVote<TYPES, I> =
    <CommitteeEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote;

pub type ViewSyncVoteType<TYPES, I> =
    <ViewSyncEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote;

/// Communication channel for [`QuorumProposalType`] and [`QuorumVote`].
pub type QuorumCommChannel<TYPES, I> =
    <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Networking;

pub type ViewSyncCommChannel<TYPES, I> =
    <ViewSyncEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Networking;

/// Communication channel for [`CommitteeProposalType`] and [`DAVote`].
pub type CommitteeCommChannel<TYPES, I> =
    <CommitteeEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Networking;

/// Protocol for determining membership in a consensus committee.
pub type QuorumMembership<TYPES, I> =
    <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership;

/// Protocol for determining membership in a DA committee.
pub type CommitteeMembership<TYPES, I> =
    <CommitteeEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership;

/// Type for the underlying quorum `ConnectedNetwork` that will be shared (for now) b/t Communication Channels
pub type QuorumNetwork<TYPES, I> = <QuorumCommChannel<TYPES, I> as CommunicationChannel<
    TYPES,
    Message<TYPES, I>,
    QuorumProposalType<TYPES, I>,
    QuorumVoteType<TYPES, I>,
    QuorumMembership<TYPES, I>,
>>::NETWORK;

/// Type for the underlying committee `ConnectedNetwork` that will be shared (for now) b/t Communication Channels
pub type CommitteeNetwork<TYPES, I> = <CommitteeCommChannel<TYPES, I> as CommunicationChannel<
    TYPES,
    Message<TYPES, I>,
    CommitteeProposalType<TYPES, I>,
    CommitteeVote<TYPES, I>,
    CommitteeMembership<TYPES, I>,
>>::NETWORK;

pub type ViewSyncNetwork<TYPES, I> = <ViewSyncCommChannel<TYPES, I> as CommunicationChannel<
    TYPES,
    Message<TYPES, I>,
    QuorumProposalType<TYPES, I>,
    ViewSyncVoteType<TYPES, I>,
    QuorumMembership<TYPES, I>,
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
