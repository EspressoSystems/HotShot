//! Provides types useful for representing `HotShot`'s data structures
//!
//! This module provides types for representing consensus internal state, such as leaves,
//! `HotShot`'s version of a block, and proposals, messages upon which to reach the consensus.
#![allow(clippy::missing_docs_in_private_items)]
#![allow(missing_docs)]

use crate::{
    certificate::{DACertificate, QuorumCertificate},
    constants::genesis_proposer_id,
    traits::{
        election::{Election, Membership, SignedCertificate},
        node_implementation::NodeType,
        signature_key::EncodedPublicKey,
        state::{ConsensusTime, TestableBlock, TestableState, ValidatingConsensusType},
        storage::StoredView,
        Block, State,
    },
};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Read, SerializationError, Write};
use commit::{Commitment, Committable};
use derivative::Derivative;
use either::Either;
use espresso_systems_common::hotshot::tag;
#[allow(deprecated)]
use nll::nll_todo::nll_todo;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{fmt::Debug, hash::Hash, marker::PhantomData};

/// Type-safe wrapper around `u64` so we know the thing we're talking about is a view number.
#[derive(
    Copy,
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    CanonicalSerialize,
    CanonicalDeserialize,
)]
pub struct ViewNumber(u64);

impl ConsensusTime for ViewNumber {
    /// Create a genesis view number (0)
    fn genesis() -> Self {
        Self(0)
    }
    /// Create a new `ViewNumber` with the given value.
    fn new(n: u64) -> Self {
        Self(n)
    }
}

impl std::ops::Add<u64> for ViewNumber {
    type Output = ViewNumber;

    fn add(self, rhs: u64) -> Self::Output {
        Self(self.0 + rhs)
    }
}

impl std::ops::AddAssign<u64> for ViewNumber {
    fn add_assign(&mut self, rhs: u64) {
        self.0 += rhs;
    }
}

impl std::ops::Deref for ViewNumber {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// The `Transaction` type associated with a `State`, as a syntactic shortcut
pub type Transaction<STATE> = <<STATE as State>::BlockType as Block>::Transaction;
/// `Commitment` to the `Transaction` type associated with a `State`, as a syntactic shortcut
pub type TxnCommitment<STATE> = Commitment<Transaction<STATE>>;

/// subset of state that we stick into a leaf.
/// original hotstuff proposal
#[derive(custom_debug::Debug, Serialize, Deserialize, Clone, Derivative, Eq)]
#[serde(bound(deserialize = ""))]
#[derivative(PartialEq, Hash)]
pub struct ValidatingProposal<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>>
where
    LEAF: Committable,
{
    ///  current view's block commitment
    pub block_commitment: Commitment<TYPES::BlockType>,

    /// CurView from leader when proposing leaf
    pub view_number: TYPES::Time,

    /// Height from leader when proposing leaf
    pub height: u64,

    /// Per spec, justification
    pub justify_qc: QuorumCertificate<TYPES, LEAF>,

    /// The hash of the parent `Leaf`
    /// So we can ask if it extends
    #[debug(skip)]
    pub parent_commitment: Commitment<LEAF>,

    /// Block leaf wants to apply
    pub deltas: TYPES::BlockType,

    /// What the state should be after applying `self.deltas`
    pub state_commitment: Commitment<TYPES::StateType>,

    /// Transactions that were marked for rejection while collecting deltas
    pub rejected: Vec<<TYPES::BlockType as Block>::Transaction>,

    /// the propser id
    pub proposer_id: EncodedPublicKey,
}

#[derive(custom_debug::Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub struct DAProposal<TYPES: NodeType> {
    /// Block leaf wants to apply
    pub deltas: TYPES::BlockType,
    /// View this proposal applies to
    pub view_number: TYPES::Time,
}

#[derive(custom_debug::Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
#[serde(bound(deserialize = ""))]
pub struct CommitmentProposal<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> {
    #[allow(clippy::missing_docs_in_private_items)]
    pub block_commitment: Commitment<TYPES::BlockType>,

    /// CurView from leader when proposing leaf
    pub view_number: TYPES::Time,

    /// Height from leader when proposing leaf
    pub height: u64,

    /// Per spec, justification
    pub justify_qc: QuorumCertificate<TYPES, LEAF>,

    /// Data availibity certificate
    pub dac: DACertificate<TYPES>,

    /// the propser id
    pub proposer_id: EncodedPublicKey,

    /// application specific metadata
    pub application_metadata: TYPES::ApplicationMetadataType,
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> ProposalType
    for ValidatingProposal<TYPES, LEAF>
{
    type NodeType = TYPES;
    fn get_view_number(&self) -> <Self::NodeType as NodeType>::Time {
        self.view_number
    }
}

impl<TYPES: NodeType> ProposalType for DAProposal<TYPES> {
    type NodeType = TYPES;
    fn get_view_number(&self) -> <Self::NodeType as NodeType>::Time {
        self.view_number
    }
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> ProposalType
    for CommitmentProposal<TYPES, LEAF>
where
    TYPES::ApplicationMetadataType: Send + Sync,
{
    type NodeType = TYPES;
    fn get_view_number(&self) -> <Self::NodeType as NodeType>::Time {
        self.view_number
    }
}
pub trait ProposalType:
    Debug + Clone + 'static + Serialize + for<'a> Deserialize<'a> + Send + Sync + PartialEq + Eq
{
    type NodeType: NodeType;
    fn get_view_number(&self) -> <Self::NodeType as NodeType>::Time;
}

pub trait LeafType:
    Debug
    + Clone
    + 'static
    + Committable
    + Serialize
    + for<'a> Deserialize<'a>
    + Send
    + Sync
    + Eq
    + std::hash::Hash
{
    type NodeType: NodeType;
    type DeltasType: Clone + Debug + for<'a> Deserialize<'a> + PartialEq + Send + Serialize + Sync;
    type StateCommitmentType: Clone
        + Debug
        + for<'a> Deserialize<'a>
        + PartialEq
        + Send
        + Serialize
        + Sync;
    type QuorumCertificate: SignedCertificate<
            <Self::NodeType as NodeType>::SignatureKey,
            <Self::NodeType as NodeType>::Time,
            <Self::NodeType as NodeType>::VoteTokenType,
            Self,
        > + Committable
        + Debug
        + Eq
        + Hash
        + PartialEq
        + Send;
    type DACertificate: SignedCertificate<
            <Self::NodeType as NodeType>::SignatureKey,
            <Self::NodeType as NodeType>::Time,
            <Self::NodeType as NodeType>::VoteTokenType,
            <Self::NodeType as NodeType>::BlockType,
        > + Debug
        + Eq
        + PartialEq
        + Send;

    fn new(
        view_number: <Self::NodeType as NodeType>::Time,
        justify_qc: Self::QuorumCertificate,
        deltas: <Self::NodeType as NodeType>::BlockType,
        state: <Self::NodeType as NodeType>::StateType,
    ) -> Self;

    fn get_view_number(&self) -> <Self::NodeType as NodeType>::Time;

    fn get_height(&self) -> u64;

    fn set_height(&mut self, height: u64);

    fn get_justify_qc(&self) -> Self::QuorumCertificate;

    fn get_parent_commitment(&self) -> Commitment<Self>;

    fn get_deltas(&self) -> Self::DeltasType;

    fn get_state(&self) -> Self::StateCommitmentType;

    fn get_rejected(&self) -> Vec<<<Self::NodeType as NodeType>::BlockType as Block>::Transaction>;

    fn get_timestamp(&self) -> i128;

    fn get_proposer_id(&self) -> EncodedPublicKey;

    fn from_stored_view(stored_view: StoredView<Self::NodeType, Self>) -> Self;
}

pub trait TestableLeaf {
    type NodeType: NodeType;

    fn create_random_transaction(
        &self,
        rng: &mut dyn rand::RngCore,
        padding: u64,
    ) -> <<Self::NodeType as NodeType>::BlockType as Block>::Transaction;
}

/// This is the consensus-internal analogous concept to a block, and it contains the block proper,
/// as well as the hash of its parent `Leaf`.
/// NOTE: `State` is constrained to implementing `BlockContents`, is `TypeMap::Block`
#[derive(Serialize, Deserialize, Clone, Debug, Derivative)]
#[serde(bound(deserialize = ""))]
#[derivative(Hash, PartialEq, Eq)]
pub struct ValidatingLeaf<TYPES: NodeType> {
    /// CurView from leader when proposing leaf
    pub view_number: TYPES::Time,

    /// Number of leaves before this one in the chain
    pub height: u64,

    /// Per spec, justification
    pub justify_qc: QuorumCertificate<TYPES, Self>,

    /// The hash of the parent `Leaf`
    /// So we can ask if it extends
    pub parent_commitment: Commitment<ValidatingLeaf<TYPES>>,

    /// Block leaf wants to apply
    pub deltas: TYPES::BlockType,

    /// What the state should be AFTER applying `self.deltas`
    pub state: TYPES::StateType,

    /// Transactions that were marked for rejection while collecting deltas
    pub rejected: Vec<<TYPES::BlockType as Block>::Transaction>,

    /// the timestamp the leaf was constructed at, in nanoseconds. Only exposed for dashboard stats
    #[derivative(PartialEq = "ignore")]
    #[derivative(Hash = "ignore")]
    pub timestamp: i128,

    /// the proposer id of the leaf
    #[derivative(PartialEq = "ignore")]
    #[derivative(Hash = "ignore")]
    pub proposer_id: EncodedPublicKey,
}

/// This is the consensus-internal analogous concept to a block, and it contains the block proper,
/// as well as the hash of its parent `Leaf`.
/// NOTE: `State` is constrained to implementing `BlockContents`, is `TypeMap::Block`
#[derive(Serialize, Deserialize, Clone, Debug, Derivative, Eq)]
#[derivative(PartialEq, Hash)]
#[serde(bound(deserialize = ""))]
pub struct SequencingLeaf<TYPES: NodeType> {
    /// CurView from leader when proposing leaf
    pub view_number: TYPES::Time,

    /// Number of leaves before this one in the chain
    pub height: u64,

    /// Per spec, justification
    pub justify_qc: QuorumCertificate<TYPES, Self>,

    /// The hash of the parent `SequencingLeaf`
    /// So we can ask if it extends
    pub parent_commitment: Commitment<SequencingLeaf<TYPES>>,

    /// The block or block commitment to be applied
    pub deltas: Either<TYPES::BlockType, Commitment<TYPES::BlockType>>,

    /// Transactions that were marked for rejection while collecting deltas
    pub rejected: Vec<<TYPES::BlockType as Block>::Transaction>,

    /// the timestamp the leaf was constructed at, in nanoseconds. Only exposed for dashboard stats
    #[derivative(PartialEq = "ignore")]
    pub timestamp: i128,

    /// the proposer id of the leaf
    #[derivative(PartialEq = "ignore")]
    pub proposer_id: EncodedPublicKey,
}

impl<TYPES: NodeType> LeafType for ValidatingLeaf<TYPES> {
    type NodeType = TYPES;
    type DeltasType = TYPES::BlockType;
    type StateCommitmentType = TYPES::StateType;
    type QuorumCertificate = QuorumCertificate<Self::NodeType, Self>;
    type DACertificate = DACertificate<Self::NodeType>;

    fn new(
        view_number: <Self::NodeType as NodeType>::Time,
        justify_qc: QuorumCertificate<Self::NodeType, Self>,
        deltas: <Self::NodeType as NodeType>::BlockType,
        state: <Self::NodeType as NodeType>::StateType,
    ) -> Self {
        Self {
            view_number,
            height: 0,
            justify_qc,
            parent_commitment: fake_commitment(),
            deltas,
            state,
            rejected: Vec::new(),
            timestamp: time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            proposer_id: genesis_proposer_id(),
        }
    }

    fn get_view_number(&self) -> TYPES::Time {
        self.view_number
    }

    fn get_height(&self) -> u64 {
        self.height
    }

    fn set_height(&mut self, height: u64) {
        self.height = height;
    }

    fn get_justify_qc(&self) -> QuorumCertificate<TYPES, Self> {
        self.justify_qc.clone()
    }

    fn get_parent_commitment(&self) -> Commitment<Self> {
        self.parent_commitment
    }

    fn get_deltas(&self) -> Self::DeltasType {
        self.deltas.clone()
    }

    fn get_state(&self) -> Self::StateCommitmentType {
        self.state.clone()
    }

    fn get_rejected(&self) -> Vec<<TYPES::BlockType as Block>::Transaction> {
        self.rejected.clone()
    }

    fn get_timestamp(&self) -> i128 {
        self.timestamp
    }

    fn get_proposer_id(&self) -> EncodedPublicKey {
        self.proposer_id.clone()
    }

    fn from_stored_view(stored_view: StoredView<Self::NodeType, Self>) -> Self {
        Self {
            view_number: stored_view.view_number,
            height: 0,
            justify_qc: stored_view.justify_qc,
            parent_commitment: stored_view.parent,
            deltas: stored_view.deltas,
            state: stored_view.state,
            rejected: stored_view.rejected,
            timestamp: stored_view.timestamp,
            proposer_id: stored_view.proposer_id,
        }
    }
}

impl<TYPES: NodeType> TestableLeaf for ValidatingLeaf<TYPES>
where
    TYPES::StateType: TestableState,
    TYPES::BlockType: TestableBlock,
{
    type NodeType = TYPES;

    fn create_random_transaction(
        &self,
        rng: &mut dyn rand::RngCore,
        padding: u64,
    ) -> <<Self::NodeType as NodeType>::BlockType as Block>::Transaction {
        <TYPES::StateType as TestableState>::create_random_transaction(&self.state, rng, padding)
    }
}

impl<TYPES: NodeType> LeafType for SequencingLeaf<TYPES> {
    type NodeType = TYPES;
    type DeltasType = Either<TYPES::BlockType, Commitment<TYPES::BlockType>>;
    type StateCommitmentType = ();
    type QuorumCertificate = QuorumCertificate<Self::NodeType, Self>;
    type DACertificate = DACertificate<Self::NodeType>;

    fn new(
        view_number: <Self::NodeType as NodeType>::Time,
        justify_qc: QuorumCertificate<Self::NodeType, Self>,
        deltas: <Self::NodeType as NodeType>::BlockType,
        _state: <Self::NodeType as NodeType>::StateType,
    ) -> Self {
        Self {
            view_number,
            height: 0,
            justify_qc,
            parent_commitment: fake_commitment(),
            deltas: Either::Left(deltas),
            rejected: Vec::new(),
            timestamp: time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            proposer_id: genesis_proposer_id(),
        }
    }

    fn get_view_number(&self) -> TYPES::Time {
        self.view_number
    }

    fn get_height(&self) -> u64 {
        self.height
    }

    fn set_height(&mut self, height: u64) {
        self.height = height;
    }

    fn get_justify_qc(&self) -> QuorumCertificate<TYPES, Self> {
        self.justify_qc.clone()
    }

    fn get_parent_commitment(&self) -> Commitment<Self> {
        self.parent_commitment
    }

    fn get_deltas(&self) -> Self::DeltasType {
        self.deltas.clone()
    }

    // The Sequencing Leaf doesn't have a state.
    fn get_state(&self) -> Self::StateCommitmentType {}

    fn get_rejected(&self) -> Vec<<TYPES::BlockType as Block>::Transaction> {
        self.rejected.clone()
    }

    fn get_timestamp(&self) -> i128 {
        self.timestamp
    }

    fn get_proposer_id(&self) -> EncodedPublicKey {
        self.proposer_id.clone()
    }

    fn from_stored_view(stored_view: StoredView<Self::NodeType, Self>) -> Self {
        Self {
            view_number: stored_view.view_number,
            height: 0,
            justify_qc: stored_view.justify_qc,
            parent_commitment: stored_view.parent,
            deltas: stored_view.deltas,
            rejected: stored_view.rejected,
            timestamp: stored_view.timestamp,
            proposer_id: stored_view.proposer_id,
        }
    }
}

impl<TYPES: NodeType> TestableLeaf for SequencingLeaf<TYPES>
where
    TYPES::StateType: TestableState,
    TYPES::BlockType: TestableBlock,
{
    type NodeType = TYPES;

    fn create_random_transaction(
        &self,
        _rng: &mut dyn rand::RngCore,
        _padding: u64,
    ) -> <<Self::NodeType as NodeType>::BlockType as Block>::Transaction {
        #[allow(deprecated)]
        nll_todo()
    }
}
/// Fake the thing a genesis block points to. Needed to avoid infinite recursion
pub fn fake_commitment<S: Committable>() -> Commitment<S> {
    commit::RawCommitmentBuilder::new("Dummy commitment for arbitrary genesis").finalize()
}

/// create a random commitment
pub fn random_commitment<S: Committable>(rng: &mut dyn rand::RngCore) -> Commitment<S> {
    let random_array: Vec<u8> = (0u8..100u8).map(|_| rng.gen_range(0..255)).collect();
    commit::RawCommitmentBuilder::new("Random Commitment")
        .constant_str("Random Field")
        .var_size_bytes(&random_array)
        .finalize()
}

impl<TYPES: NodeType> Committable for ValidatingLeaf<TYPES> {
    fn commit(&self) -> commit::Commitment<Self> {
        let mut signatures_bytes = vec![];
        for (k, v) in &self.justify_qc.signatures {
            signatures_bytes.extend(&k.0);
            signatures_bytes.extend(&v.0 .0);
            signatures_bytes.extend::<&[u8]>(v.1.commit().as_ref());
        }
        commit::RawCommitmentBuilder::new("Leaf Comm")
            .u64_field("view_number", *self.view_number)
            .u64_field("height", self.height)
            .field("parent Leaf commitment", self.parent_commitment)
            .field("block commitment", self.deltas.commit())
            .field("state commitment", self.state.commit())
            .constant_str("justify_qc view number")
            .u64(*self.justify_qc.view_number)
            .field(
                "justify_qc leaf commitment",
                self.justify_qc.leaf_commitment(),
            )
            .constant_str("justify_qc signatures")
            .var_size_bytes(&signatures_bytes)
            .finalize()
    }

    fn tag() -> String {
        tag::LEAF.to_string()
    }
}

impl<TYPES: NodeType> Committable for SequencingLeaf<TYPES> {
    fn commit(&self) -> commit::Commitment<Self> {
        // Commit the block commitment, rather than the block, so that the replicas can reconstruct
        // the leaf.
        let block_commitment = match &self.deltas {
            Either::Left(block) => block.commit(),
            Either::Right(commitment) => *commitment,
        };
        let mut signatures_bytes = vec![];
        for (k, v) in &self.justify_qc.signatures {
            signatures_bytes.extend(&k.0);
            signatures_bytes.extend(&v.0 .0);
            signatures_bytes.extend::<&[u8]>(v.1.commit().as_ref());
        }
        commit::RawCommitmentBuilder::new("Leaf Comm")
            .u64_field("view_number", *self.view_number)
            .u64_field("height", self.height)
            .field("parent Leaf commitment", self.parent_commitment)
            .field("block commitment", block_commitment)
            .constant_str("justify_qc view number")
            .u64(*self.justify_qc.view_number)
            .field(
                "justify_qc leaf commitment",
                self.justify_qc.leaf_commitment(),
            )
            .constant_str("justify_qc signatures")
            .var_size_bytes(&signatures_bytes)
            .finalize()
    }
}

impl<TYPES: NodeType> From<ValidatingLeaf<TYPES>>
    for ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>
{
    fn from(leaf: ValidatingLeaf<TYPES>) -> Self {
        Self {
            view_number: leaf.view_number,
            height: leaf.height,
            justify_qc: leaf.justify_qc,
            parent_commitment: leaf.parent_commitment,
            deltas: leaf.deltas.clone(),
            state_commitment: leaf.state.commit(),
            rejected: leaf.rejected,
            proposer_id: leaf.proposer_id,
            block_commitment: leaf.deltas.commit(),
        }
    }
}

impl<TYPES: NodeType> ValidatingLeaf<TYPES>
where
    TYPES::ConsensusType: ValidatingConsensusType,
{
    /// Creates a new leaf with the specified block and parent
    ///
    /// # Arguments
    ///   * `item` - The block to include
    ///   * `parent` - The hash of the `Leaf` that is to be the parent of this `Leaf`
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        state: TYPES::StateType,
        deltas: TYPES::BlockType,
        parent_commitment: Commitment<ValidatingLeaf<TYPES>>,
        justify_qc: QuorumCertificate<TYPES, Self>,
        view_number: TYPES::Time,
        height: u64,
        rejected: Vec<<TYPES::BlockType as Block>::Transaction>,
        timestamp: i128,
        proposer_id: EncodedPublicKey,
    ) -> Self {
        ValidatingLeaf {
            view_number,
            height,
            justify_qc,
            parent_commitment,
            deltas,
            state,
            rejected,
            timestamp,
            proposer_id,
        }
    }

    /// Creates the genesis Leaf for the genesis View (special case),
    /// from the genesis block (deltas, application supplied)
    /// and genesis state (result of deltas applied to the default state)
    /// justified by the genesis qc (special case)
    ///
    /// # Panics
    ///
    /// Panics if deltas is not a valid genesis block,
    /// or if state cannot extend deltas from default()
    pub fn genesis(deltas: TYPES::BlockType) -> Self {
        // if this fails, we're not able to initialize consensus.
        let state = <TYPES as NodeType>::StateType::append(
            &TYPES::StateType::default(),
            &deltas,
            &TYPES::Time::genesis(),
        )
        .unwrap();
        Self {
            view_number: TYPES::Time::genesis(),
            height: 0,
            justify_qc: QuorumCertificate::genesis(),
            parent_commitment: fake_commitment(),
            deltas,
            state,
            rejected: Vec::new(),
            timestamp: time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            proposer_id: genesis_proposer_id(),
        }
    }
}

impl<TYPES: NodeType> From<StoredView<TYPES, ValidatingLeaf<TYPES>>> for ValidatingLeaf<TYPES>
where
    TYPES::ConsensusType: ValidatingConsensusType,
{
    fn from(append: StoredView<TYPES, ValidatingLeaf<TYPES>>) -> Self {
        ValidatingLeaf::new(
            append.state,
            append.deltas,
            append.parent,
            append.justify_qc,
            append.view_number,
            append.height,
            Vec::new(),
            append.timestamp,
            append.proposer_id,
        )
    }
}

impl<TYPES, LEAF> From<LEAF> for StoredView<TYPES, LEAF>
where
    TYPES: NodeType,
    LEAF: LeafType<NodeType = TYPES>,
{
    fn from(leaf: LEAF) -> Self {
        StoredView {
            view_number: leaf.get_view_number(),
            height: leaf.get_height(),
            parent: leaf.get_parent_commitment(),
            justify_qc: leaf.get_justify_qc(),
            state: leaf.get_state(),
            deltas: leaf.get_deltas(),
            rejected: leaf.get_rejected(),
            timestamp: leaf.get_timestamp(),
            proposer_id: leaf.get_proposer_id(),
        }
    }
}
