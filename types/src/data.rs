//! Provides types useful for representing `HotShot`'s data structures
//!
//! This module provides types for representing consensus internal state, such as the [`Leaf`],
//! `HotShot`'s version of a block, and the [`QuorumCertificate`], representing the threshold
//! signatures fundamental to consensus.
use crate::{
    constants::genesis_proposer_id,
    traits::{
        election::{Accumulator, Either, Election, SignedCertificate},
        node_implementation::NodeTypes,
        signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey},
        state::ConsensusTime,
        storage::StoredView,
        Block, State,
    },
};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Read, SerializationError, Write};
use commit::{Commitment, Committable};
use derivative::Derivative;
use hotshot_utils::hack::nll_todo;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt::Debug, marker::PhantomData, num::NonZeroU64, ops::Deref};

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

impl<TYPES: NodeTypes, LEAF: LeafType<NodeType = TYPES>> QuorumCertificate<TYPES, LEAF> {
    /// To be used only for generating the genesis quorum certificate; will fail if used anywhere else
    pub fn genesis() -> Self {
        Self {
            // block_commitment: fake_commitment(),
            leaf_commitment: fake_commitment::<LEAF>(),
            view_number: <TYPES::Time as ConsensusTime>::genesis(),
            signatures: BTreeMap::default(),
            genesis: true,
        }
    }
}

// TODO (da) move this, and QC to separate files
#[derive(Clone, Serialize, Deserialize)]
pub struct DACertificate<TYPES: NodeTypes> {
    /// The view number this quorum certificate was generated during
    ///
    /// This value is covered by the threshold signature.
    pub view_number: TYPES::Time,

    /// The list of signatures establishing the validity of this Quorum Certifcate
    ///
    /// This is a mapping of the byte encoded public keys provided by the [`NodeImplementation`], to
    /// the byte encoded signatures provided by those keys.
    ///
    /// These formats are deliberatly done as a `Vec` instead of an array to prevent creating the
    /// assumption that singatures are constant in length
    /// TODO (da) make a separate vote token type for DA and QC
    pub signatures: BTreeMap<EncodedPublicKey, (EncodedSignature, TYPES::VoteTokenType)>,
    // no genesis bc not meaningful
}

/// The type used for Quorum Certificates
///
/// A Quorum Certificate is a threshold signature of the [`Leaf`] being proposed, as well as some
/// metadata, such as the [`Stage`] of consensus the quorum certificate was generated during.
#[derive(custom_debug::Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq, Hash)]
#[serde(bound(deserialize = ""))]
pub struct QuorumCertificate<TYPES: NodeTypes, LEAF: LeafType<NodeType = TYPES>> {
    // block commitment is contained within the leaf. Still need to check this
    /// TODO (da) we need to check
    ///   - parent QC PROPOSAL
    ///   - somehow make this semantically equivalent to what is currently `Leaf`
    ///
    #[debug(skip)]
    pub leaf_commitment: Commitment<LEAF>,

    pub view_number: TYPES::Time,

    pub signatures: BTreeMap<EncodedPublicKey, (EncodedSignature, TYPES::VoteTokenType)>,

    pub genesis: bool,
}

pub struct CertificateAccumulator<SIGNATURE, CERT>
where
    SIGNATURE: SignatureKey,
    CERT: SignedCertificate<SIGNATURE>,
{
    _pd_0: PhantomData<SIGNATURE>,
    _pd_1: PhantomData<CERT>,
    _valid_signatures: Vec<(EncodedSignature, SIGNATURE)>,
    _threshold: NonZeroU64,
    // TODO
}

impl<SIGNATURE, CERT> Accumulator<(EncodedSignature, SIGNATURE), CERT>
    for CertificateAccumulator<SIGNATURE, CERT>
where
    SIGNATURE: SignatureKey,
    CERT: SignedCertificate<SIGNATURE>,
{
    fn append(
        val: Vec<(EncodedSignature, SIGNATURE)>,
    ) -> crate::traits::election::Either<Self, CERT> {
        nll_todo()
    }
}

impl<TYPES: NodeTypes, LEAF: LeafType<NodeType = TYPES>> SignedCertificate<TYPES::SignatureKey>
    for QuorumCertificate<TYPES, LEAF>
{
    type Accumulator = CertificateAccumulator<TYPES::SignatureKey, QuorumCertificate<TYPES, LEAF>>;
}

impl<TYPES: NodeTypes, LEAF: LeafType<NodeType = TYPES>> Eq for QuorumCertificate<TYPES, LEAF> {}

impl<TYPES: NodeTypes, LEAF: LeafType<NodeType = TYPES>> Committable
    for QuorumCertificate<TYPES, LEAF>
{
    fn commit(&self) -> Commitment<Self> {
        let mut builder = commit::RawCommitmentBuilder::new("Quorum Certificate Commitment");

        builder = builder
            .field("Leaf commitment", self.leaf_commitment)
            .u64_field("View number", *self.view_number.deref());

        for (idx, (k, v)) in self.signatures.iter().enumerate() {
            builder = builder
                .var_size_field(&format!("Signature {idx} public key"), &k.0)
                .var_size_field(&format!("Signature {idx} signature"), &v.0 .0)
                .field(&format!("Signature {idx} signature"), v.1.commit());
        }

        builder.u64_field("Genesis", self.genesis.into()).finalize()
    }
}

/// The `Transaction` type associated with a `State`, as a syntactic shortcut
pub type Transaction<STATE> = <<STATE as State>::BlockType as Block>::Transaction;
/// `Commitment` to the `Transaction` type associated with a `State`, as a syntactic shortcut
pub type TxnCommitment<STATE> = Commitment<Transaction<STATE>>;

// TODO leafproposal trait for both types of consensus
// TODO rename to match spec

/// subset of state that we stick into a leaf.
/// original hotstuff proposal
#[derive(custom_debug::Debug, Serialize, Deserialize, Clone, Derivative, Eq)]
#[serde(bound(deserialize = ""))]
#[derivative(PartialEq, Hash)]
pub struct ValidatingProposal<TYPES: NodeTypes, ELECTION: Election<TYPES>>
where
    ELECTION::LeafType: Committable,
{
    ///  current view's block commitment
    pub block_commitment: Commitment<TYPES::BlockType>,

    /// CurView from leader when proposing leaf
    pub view_number: TYPES::Time,

    /// Per spec, justification
    pub justify_qc: QuorumCertificate<TYPES, ELECTION::LeafType>,

    /// The hash of the parent `Leaf`
    /// So we can ask if it extends
    #[debug(skip)]
    pub parent_commitment: Commitment<ELECTION::LeafType>,

    /// Block leaf wants to apply
    pub deltas: TYPES::BlockType,

    /// What the state should be after applying `self.deltas`
    #[debug(skip)]
    pub state_commitment: Commitment<TYPES::StateType>,

    /// Transactions that were marked for rejection while collecting deltas
    pub rejected: Vec<<TYPES::BlockType as Block>::Transaction>,

    /// the propser id
    /// TODO (da) why is this not included? in partieq or hash? it really should be
    /// since we are using this in our validity checks when accepting a proposal
    pub proposer_id: EncodedPublicKey,

    pub _pd: PhantomData<ELECTION>,
}

#[derive(custom_debug::Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub struct DAProposal<TYPES: NodeTypes, ELECTION: Election<TYPES>> {
    /// Block leaf wants to apply
    pub deltas: TYPES::BlockType,

    pub view_number: TYPES::Time,

    pub _pd: PhantomData<ELECTION>,
}

/// make generic over election
/// OR move certs out of election and into nodetypes
#[derive(custom_debug::Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
#[serde(bound(deserialize = ""))]
pub struct CommitmentProposal<TYPES: NodeTypes, ELECTION: Election<TYPES>> {
    pub block_commitment: Commitment<TYPES::BlockType>,

    /// CurView from leader when proposing leaf
    pub view_number: TYPES::Time,

    /// Per spec, justification
    pub justify_qc: ELECTION::QuorumCertificate,
    // pub justify_qc: Box<dyn Send + Sync + SignedCertificate<TYPES::SignatureKey>>,
    /// TODO implmeent data availibity certificate and add trait here
    pub availability_certificate: ELECTION::DACertificate,

    /// parent commitment alrady in justify_qqc

    /// What the state should be after applying `self.deltas`
    #[debug(skip)]
    pub state_commitment: Commitment<TYPES::StateType>,

    /// the propser id
    pub proposer_id: EncodedPublicKey,

    /// application specific metadata
    pub application_metadata: TYPES::ApplicationMetadataType,
}

impl<TYPES: NodeTypes, ELECTION: Election<TYPES>> ProposalType
    for ValidatingProposal<TYPES, ELECTION>
where
    ELECTION::LeafType: Committable,
{
    type NodeTypes = TYPES;
    type Election = ELECTION;
}

impl<TYPES: NodeTypes, ELECTION: Election<TYPES>> ProposalType for DAProposal<TYPES, ELECTION> {
    type NodeTypes = TYPES;
    type Election = ELECTION;
}

impl<TYPES: NodeTypes, ELECTION: Election<TYPES>> ProposalType
    for CommitmentProposal<TYPES, ELECTION>
where
    TYPES::ApplicationMetadataType: Send + Sync,
{
    type NodeTypes = TYPES;
    type Election = ELECTION;
}

// TODO (da) rename NodeTypes to NodeType for consistency

pub trait ProposalType:
    Debug + Clone + 'static + Serialize + for<'a> Deserialize<'a> + Send + Sync + PartialEq + Eq
{
    type NodeTypes: NodeTypes;
    type Election: Election<Self::NodeTypes>;
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
    + PartialEq
    + Eq
{
    type NodeType: NodeTypes;
}

/// This is the consensus-internal analogous concept to a block, and it contains the block proper,
/// as well as the hash of its parent `Leaf`.
/// NOTE: `State` is constrained to implementing `BlockContents`, is `TypeMap::Block`
#[derive(Serialize, Deserialize, Clone, Debug, Derivative, Eq)]
#[derivative(PartialEq)]
#[serde(bound(deserialize = ""))]
pub struct ValidatingLeaf<TYPES: NodeTypes> {
    /// CurView from leader when proposing leaf
    pub view_number: TYPES::Time,

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
    pub timestamp: i128,

    /// the proposer id of the leaf
    #[derivative(PartialEq = "ignore")]
    pub proposer_id: EncodedPublicKey,
}

impl<TYPES: NodeTypes> LeafType for ValidatingLeaf<TYPES> {
    type NodeType = TYPES;
}

impl<TYPES: NodeTypes> LeafType for DALeaf<TYPES> {
    type NodeType = TYPES;
}

/// This is the consensus-internal analogous concept to a block, and it contains the block proper,
/// as well as the hash of its parent `Leaf`.
/// NOTE: `State` is constrained to implementing `BlockContents`, is `TypeMap::Block`
#[derive(Serialize, Deserialize, Clone, Debug, Derivative, Eq)]
#[derivative(PartialEq)]
#[serde(bound(deserialize = ""))]
pub struct DALeaf<TYPES: NodeTypes> {
    /// CurView from leader when proposing leaf
    pub view_number: TYPES::Time,

    /// Per spec, justification
    pub justify_qc: QuorumCertificate<TYPES, Self>,

    /// The hash of the parent `DALeaf`
    /// So we can ask if it extends
    pub parent_commitment: Commitment<DALeaf<TYPES>>,

    /// Block leaf wants to apply
    pub deltas: TYPES::BlockType,

    /// What the state should be AFTER applying `self.deltas`
    /// dependent on whether we have the state yet
    pub state: Either<TYPES::StateType, Commitment<TYPES::StateType>>,

    /// Transactions that were marked for rejection while collecting deltas
    pub rejected: Vec<<TYPES::BlockType as Block>::Transaction>,

    /// the timestamp the leaf was constructed at, in nanoseconds. Only exposed for dashboard stats
    #[derivative(PartialEq = "ignore")]
    pub timestamp: i128,

    /// the proposer id of the leaf
    #[derivative(PartialEq = "ignore")]
    pub proposer_id: EncodedPublicKey,
}

/// Kake the thing a genesis block points to. Needed to avoid infinite recursion
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

impl<TYPES: NodeTypes> Committable for ValidatingLeaf<TYPES> {
    fn commit(&self) -> commit::Commitment<Self> {
        let mut signatures_bytes = vec![];
        for (k, v) in &self.justify_qc.signatures {
            signatures_bytes.extend(&k.0);
            signatures_bytes.extend(&v.0 .0);
            signatures_bytes.extend(v.1.commit().as_ref());
        }
        commit::RawCommitmentBuilder::new("Leaf Comm")
            .constant_str("view_number")
            .u64(*self.view_number)
            .field("parent Leaf commitment", self.parent_commitment)
            .field("deltas commitment", self.deltas.commit())
            .field("state commitment", self.state.commit())
            .constant_str("justify_qc view number")
            .u64(*self.justify_qc.view_number)
            .field(
                "justify_qc block commitment",
                self.justify_qc.block_commitment,
            )
            .field(
                "justify_qc leaf commitment",
                self.justify_qc.leaf_commitment,
            )
            .constant_str("justify_qc signatures")
            .var_size_bytes(&signatures_bytes)
            .finalize()
    }
}

impl<TYPES: NodeTypes> Committable for DALeaf<TYPES> {
    fn commit(&self) -> commit::Commitment<Self> {
        nll_todo()
    }
}

impl<TYPES: NodeTypes, ELECTION: Election<TYPES>> From<ValidatingLeaf<TYPES>>
    for ValidatingProposal<TYPES, ELECTION>
{
    fn from(leaf: ValidatingLeaf<TYPES>) -> Self {
        Self {
            view_number: leaf.view_number,
            justify_qc: leaf.justify_qc,
            parent_commitment: leaf.parent_commitment,
            deltas: leaf.deltas,
            state_commitment: leaf.state.commit(),
            rejected: leaf.rejected,
            proposer_id: leaf.proposer_id,
            block_commitment: nll_todo(),
            _pd: PhantomData,
        }
    }
}

impl<TYPES: NodeTypes> ValidatingLeaf<TYPES> {
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
        rejected: Vec<<TYPES::BlockType as Block>::Transaction>,
        timestamp: i128,
        proposer_id: EncodedPublicKey,
    ) -> Self {
        ValidatingLeaf {
            view_number,
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
        let state = TYPES::StateType::default()
            .append(&deltas, &TYPES::Time::genesis())
            .unwrap();
        Self {
            view_number: TYPES::Time::genesis(),
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

impl<TYPES: NodeTypes> From<StoredView<TYPES, ValidatingLeaf<TYPES>>> for ValidatingLeaf<TYPES> {
    fn from(append: StoredView<TYPES, ValidatingLeaf<TYPES>>) -> Self {
        ValidatingLeaf::new(
            append.state,
            append.append.into_deltas(),
            append.parent,
            append.justify_qc,
            append.view_number,
            Vec::new(),
            append.timestamp,
            append.proposer_id,
        )
    }
}

impl<TYPES: NodeTypes> From<ValidatingLeaf<TYPES>> for StoredView<TYPES, ValidatingLeaf<TYPES>> {
    fn from(val: ValidatingLeaf<TYPES>) -> Self {
        StoredView {
            view_number: val.view_number,
            parent: val.parent_commitment,
            justify_qc: val.justify_qc,
            state: val.state,
            append: val.deltas.into(),
            rejected: val.rejected,
            timestamp: val.timestamp,
            proposer_id: val.proposer_id,
        }
    }
}
