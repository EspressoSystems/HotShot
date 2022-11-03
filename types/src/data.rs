//! Provides types useful for representing `HotShot`'s data structures
//!
//! This module provides types for representing consensus internal state, such as the [`Leaf`],
//! `HotShot`'s version of a block, and the [`QuorumCertificate`], representing the threshold
//! signatures fundamental to consensus.
use crate::{
    constants::genesis_proposer_id,
    traits::{
        signature_key::{EncodedPublicKey, EncodedSignature},
        state::ConsensusTime,
        storage::StoredView,
        Block, State,
    },
};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Read, SerializationError, Write};
use commit::{Commitment, Committable};
use derivative::Derivative;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt::Debug};

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

impl ConsensusTime for ViewNumber {}

impl ViewNumber {
    /// Create a genesis view number (0)
    pub const fn genesis() -> Self {
        Self(0)
    }

    /// Create a new `ViewNumber` with the given value.
    pub const fn new(n: u64) -> Self {
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

impl<STATE: State> QuorumCertificate<STATE> {
    /// To be used only for generating the genesis quorum certificate; will fail if used anywhere else
    pub fn genesis() -> Self {
        Self {
            block_commitment: fake_commitment(),
            leaf_commitment: fake_commitment::<Leaf<STATE>>(),
            view_number: ViewNumber::genesis(),
            signatures: BTreeMap::default(),
            genesis: true,
        }
    }
}

/// The type used for Quorum Certificates
///
/// A Quorum Certificate is a threshold signature of the [`Leaf`] being proposed, as well as some
/// metadata, such as the [`Stage`] of consensus the quorum certificate was generated during.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, custom_debug::Debug, std::hash::Hash)]
pub struct QuorumCertificate<STATE: State> {
    /// Hash of the block refereed to by this Quorum Certificate.
    ///
    /// This is included for convenience, and is not fundamental to consensus or covered by the
    /// signature. This _must_ be identical to the [`BlockContents`] provided hash of the `item` in
    /// the referenced leaf.
    #[debug(skip)]
    #[serde(deserialize_with = "<Commitment<STATE::BlockType> as Deserialize>::deserialize")]
    pub block_commitment: Commitment<STATE::BlockType>,

    /// Hash of the [`Leaf`] referred to by this Quorum Certificate
    ///
    /// This value is covered by the threshold signature.
    #[debug(skip)]
    #[serde(deserialize_with = "<Commitment<Leaf<STATE>> as Deserialize>::deserialize")]
    pub leaf_commitment: Commitment<Leaf<STATE>>,

    /// The view number this quorum certificate was generated during
    ///
    /// This value is covered by the threshold signature.
    pub view_number: ViewNumber,

    /// The list of signatures establishing the validity of this Quorum Certifcate
    ///
    /// This is a mapping of the byte encoded public keys provided by the [`NodeImplementation`], to
    /// the byte encoded signatures provided by those keys.
    ///
    /// These formats are deliberatly done as a `Vec` instead of an array to prevent creating the
    /// assumption that singatures are constant in length
    pub signatures: BTreeMap<EncodedPublicKey, (EncodedSignature, Vec<u8>)>,

    /// Temporary bypass for boostrapping
    ///
    /// This value indicates that this is a dummy certificate for the genesis block, and thus does
    /// not have a signature. This value is not covered by the signature, and it is invalid for this
    /// to be set outside of bootstrap
    pub genesis: bool,
}

/// The `Transaction` type associated with a `State`, as a syntactic shortcut
pub type Transaction<STATE> = <<STATE as State>::BlockType as Block>::Transaction;
/// `Commitment` to the `Transaction` type associated with a `State`, as a syntactic shortcut
pub type TxnCommitment<STATE> = Commitment<Transaction<STATE>>;

/// subset of state that we stick into a leaf.
#[derive(Clone, Derivative, Serialize, Deserialize, custom_debug::Debug)]
#[derivative(PartialEq, Eq, Hash)]
pub struct ProposalLeaf<STATE: State> {
    /// CurView from leader when proposing leaf
    pub view_number: ViewNumber,

    /// Per spec, justification
    #[serde(deserialize_with = "<QuorumCertificate<STATE> as Deserialize>::deserialize")]
    pub justify_qc: QuorumCertificate<STATE>,

    /// The hash of the parent `Leaf`
    /// So we can ask if it extends
    #[debug(skip)]
    #[serde(deserialize_with = "<Commitment<Leaf<STATE>> as Deserialize>::deserialize")]
    pub parent_commitment: Commitment<Leaf<STATE>>,

    /// Block leaf wants to apply
    #[serde(deserialize_with = "STATE::BlockType::deserialize")]
    pub deltas: STATE::BlockType,

    /// What the state should be after applying `self.deltas`
    #[serde(deserialize_with = "<Commitment<STATE> as Deserialize>::deserialize")]
    #[debug(skip)]
    pub state_commitment: Commitment<STATE>,

    /// Transactions that were marked for rejection while collecting deltas
    #[serde(deserialize_with = "<Vec<TxnCommitment<STATE>> as Deserialize>::deserialize")]
    pub rejected: Vec<TxnCommitment<STATE>>,

    /// the propser id
    #[derivative(PartialEq = "ignore", Hash = "ignore")]
    pub proposer_id: EncodedPublicKey,
}

/// This is the consensus-internal analogous concept to a block, and it contains the block proper,
/// as well as the hash of its parent `Leaf`.
/// NOTE: `State` is constrained to implementing `BlockContents`, is `TypeMap::Block`
#[derive(Serialize, Deserialize, custom_debug::Debug, Clone, Derivative)]
#[derivative(PartialEq, Eq)]
pub struct Leaf<STATE: State> {
    /// CurView from leader when proposing leaf
    pub view_number: ViewNumber,

    /// Per spec, justification
    #[serde(deserialize_with = "<QuorumCertificate<STATE> as Deserialize>::deserialize")]
    pub justify_qc: QuorumCertificate<STATE>,

    /// The hash of the parent `Leaf`
    /// So we can ask if it extends
    #[debug(skip)]
    #[serde(deserialize_with = "<Commitment<Leaf<STATE>> as Deserialize>::deserialize")]
    pub parent_commitment: Commitment<Leaf<STATE>>,

    /// Block leaf wants to apply
    #[serde(deserialize_with = "STATE::BlockType::deserialize")]
    pub deltas: STATE::BlockType,

    /// What the state should be AFTER applying `self.deltas`
    #[serde(deserialize_with = "STATE::deserialize")]
    pub state: STATE,

    /// Transactions that were marked for rejection while collecting deltas
    #[serde(deserialize_with = "<Vec<TxnCommitment<STATE>> as Deserialize>::deserialize")]
    pub rejected: Vec<TxnCommitment<STATE>>,

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
pub fn random_commitment<S: Committable>() -> Commitment<S> {
    let mut rng = rand::thread_rng();
    let random_array: Vec<u8> = (0u8..100u8).map(|_| rng.gen_range(0..255)).collect();
    commit::RawCommitmentBuilder::new("Random Commitment")
        .constant_str("Random Field")
        .var_size_bytes(&random_array)
        .finalize()
}

impl<STATE: State> Committable for Leaf<STATE> {
    fn commit(&self) -> commit::Commitment<Self> {
        let mut signatures_bytes = vec![];
        for (k, v) in &self.justify_qc.signatures {
            // TODO there is probably a way to avoid cloning.
            signatures_bytes.append(&mut k.0.clone());
            signatures_bytes.append(&mut v.0 .0.clone());
            signatures_bytes.append(&mut v.1.clone());
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

impl<STATE: State> From<Leaf<STATE>> for ProposalLeaf<STATE> {
    fn from(leaf: Leaf<STATE>) -> Self {
        Self {
            view_number: leaf.view_number,
            justify_qc: leaf.justify_qc,
            parent_commitment: leaf.parent_commitment,
            deltas: leaf.deltas,
            state_commitment: leaf.state.commit(),
            rejected: leaf.rejected,
            proposer_id: leaf.proposer_id,
        }
    }
}

impl<STATE: State<Time = ViewNumber>> Leaf<STATE> {
    /// Creates a new leaf with the specified block and parent
    ///
    /// # Arguments
    ///   * `item` - The block to include
    ///   * `parent` - The hash of the `Leaf` that is to be the parent of this `Leaf`
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        state: STATE,
        deltas: STATE::BlockType,
        parent_commitment: Commitment<Leaf<STATE>>,
        justify_qc: QuorumCertificate<STATE>,
        view_number: ViewNumber,
        rejected: Vec<TxnCommitment<STATE>>,
        timestamp: i128,
        proposer_id: EncodedPublicKey,
    ) -> Self {
        Leaf {
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
    pub fn genesis(deltas: STATE::BlockType) -> Self {
        // if this fails, we're not able to initialize consensus.
        let state = STATE::default()
            .append(&deltas, &ViewNumber::genesis())
            .unwrap();
        Self {
            view_number: ViewNumber::genesis(),
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

impl<STATE: State<Time = ViewNumber>> From<StoredView<STATE>> for Leaf<STATE> {
    fn from(append: StoredView<STATE>) -> Self {
        Leaf::new(
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

impl<STATE: State> From<Leaf<STATE>> for StoredView<STATE> {
    fn from(val: Leaf<STATE>) -> Self {
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
