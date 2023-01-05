//! Provides two types of cerrtificates and their accumulators.

use crate::{
    data::{fake_commitment, LeafType},
    traits::{
        election::{Accumulator, SignedCertificate},
        node_implementation::NodeType,
        signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey},
        state::ConsensusTime,
    },
};
use commit::{Commitment, Committable};
use either::Either;
use espresso_systems_common::hotshot::tag;
#[allow(deprecated)]
use nll::nll_todo::nll_todo;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt::Debug, marker::PhantomData, num::NonZeroU64, ops::Deref};

/// A `DACertificate` is a threshold signature that some data is available.  
/// It is signed by the members of the DA comittee, not the entire network. It is used
/// to prove that the data will be made available to those outside of the DA committee.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DACertificate<TYPES: NodeType> {
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
pub struct QuorumCertificate<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> {
    // block commitment is contained within the leaf. Still need to check this
    /// TODO (da) we need to check
    ///   - parent QC PROPOSAL
    ///   - somehow make this semantically equivalent to what is currently `Leaf`
    /// Is this needed, given the proposal also contain this?
    #[debug(skip)]
    pub leaf_commitment: Commitment<LEAF>,

    /// Which view this QC relates to
    pub view_number: TYPES::Time,
    /// Threshold Signature
    pub signatures: BTreeMap<EncodedPublicKey, (EncodedSignature, TYPES::VoteTokenType)>,
    /// If this QC is for the genesis block
    pub is_genesis: bool,
}

/// `CertificateAccumulator` is describes the process of collecting signatures
/// to form a QC or a DA certificate.
#[allow(clippy::missing_docs_in_private_items)]
pub struct CertificateAccumulator<SIGNATURE, TIME, TOKEN, LEAF, CERT>
where
    SIGNATURE: SignatureKey,
    LEAF: Committable,
    CERT: SignedCertificate<SIGNATURE, TIME, TOKEN, LEAF>,
{
    _pd_0: PhantomData<SIGNATURE>,
    _pd_1: PhantomData<CERT>,
    _pd_2: PhantomData<TIME>,
    _pd_3: PhantomData<TOKEN>,
    _pd_4: PhantomData<LEAF>,
    _valid_signatures: Vec<(EncodedSignature, SIGNATURE)>,
    _threshold: NonZeroU64,
    // TODO
}

impl<SIGNATURE, TIME, CERT, TOKEN, LEAF> Accumulator<(EncodedSignature, SIGNATURE), CERT>
    for CertificateAccumulator<SIGNATURE, TIME, TOKEN, LEAF, CERT>
where
    SIGNATURE: SignatureKey,
    LEAF: Committable,
    CERT: SignedCertificate<SIGNATURE, TIME, TOKEN, LEAF>,
{
    fn append(_val: Vec<(EncodedSignature, SIGNATURE)>) -> Either<Self, CERT> {
        #[allow(deprecated)]
        nll_todo()
    }
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>>
    SignedCertificate<TYPES::SignatureKey, TYPES::Time, TYPES::VoteTokenType, LEAF>
    for QuorumCertificate<TYPES, LEAF>
{
    type Accumulator = CertificateAccumulator<
        TYPES::SignatureKey,
        TYPES::Time,
        TYPES::VoteTokenType,
        LEAF,
        QuorumCertificate<TYPES, LEAF>,
    >;

    fn view_number(&self) -> TYPES::Time {
        self.view_number
    }

    fn signatures(&self) -> BTreeMap<EncodedPublicKey, (EncodedSignature, TYPES::VoteTokenType)> {
        self.signatures.clone()
    }

    fn leaf_commitment(&self) -> Commitment<LEAF> {
        self.leaf_commitment
    }

    fn set_leaf_commitment(&mut self, commitment: Commitment<LEAF>) {
        self.leaf_commitment = commitment;
    }

    fn is_genesis(&self) -> bool {
        self.is_genesis
    }

    fn genesis() -> Self {
        Self {
            leaf_commitment: fake_commitment::<LEAF>(),
            view_number: <TYPES::Time as ConsensusTime>::genesis(),
            signatures: BTreeMap::default(),
            is_genesis: true,
        }
    }
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> Eq for QuorumCertificate<TYPES, LEAF> {}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> Committable
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

        builder
            .u64_field("Is genesis", self.is_genesis.into())
            .finalize()
    }

    fn tag() -> String {
        tag::QC.to_string()
    }
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>>
    SignedCertificate<TYPES::SignatureKey, TYPES::Time, TYPES::VoteTokenType, LEAF>
    for DACertificate<TYPES>
{
    type Accumulator = CertificateAccumulator<
        TYPES::SignatureKey,
        TYPES::Time,
        TYPES::VoteTokenType,
        LEAF,
        DACertificate<TYPES>,
    >;

    fn view_number(&self) -> TYPES::Time {
        self.view_number
    }

    fn signatures(&self) -> BTreeMap<EncodedPublicKey, (EncodedSignature, TYPES::VoteTokenType)> {
        self.signatures.clone()
    }

    fn leaf_commitment(&self) -> Commitment<LEAF> {
        // This function is only useful for QC. Will be removed after we have separated cert traits.
        #[allow(deprecated)]
        nll_todo()
    }

    fn set_leaf_commitment(&mut self, _commitment: Commitment<LEAF>) {
        // This function is only useful for QC. Will be removed after we have separated cert traits.
        #[allow(deprecated)]
        nll_todo()
    }

    fn is_genesis(&self) -> bool {
        // This function is only useful for QC. Will be removed after we have separated cert traits.
        false
    }

    fn genesis() -> Self {
        // This function is only useful for QC. Will be removed after we have separated cert traits.
        #[allow(deprecated)]
        nll_todo()
    }
}

impl<TYPES: NodeType> Eq for DACertificate<TYPES> {}
