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
    ///
    #[debug(skip)]
    pub leaf_commitment: Commitment<LEAF>,

    /// Which view this QC relates to
    pub view_number: TYPES::Time,
    /// Threshold Signature
    pub signatures: BTreeMap<EncodedPublicKey, (EncodedSignature, TYPES::VoteTokenType)>,
    /// If this QC is for the genesis block
    pub genesis: bool,
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> QuorumCertificate<TYPES, LEAF> {
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

/// `CertificateAccumulator` is describes the process of collecting signatures
/// to form a QC or a DA certificate.
#[allow(clippy::missing_docs_in_private_items)]
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
    fn append(_val: Vec<(EncodedSignature, SIGNATURE)>) -> Either<Self, CERT> {
        #[allow(deprecated)]
        nll_todo()
    }
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> SignedCertificate<TYPES::SignatureKey>
    for QuorumCertificate<TYPES, LEAF>
{
    type Accumulator = CertificateAccumulator<TYPES::SignatureKey, QuorumCertificate<TYPES, LEAF>>;
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

        builder.u64_field("Genesis", self.genesis.into()).finalize()
    }

    fn tag() -> String {
        tag::QC.to_string()
    }
}

impl<TYPES: NodeType> SignedCertificate<TYPES::SignatureKey> for DACertificate<TYPES> {
    type Accumulator = CertificateAccumulator<TYPES::SignatureKey, DACertificate<TYPES>>;
}

impl<TYPES: NodeType> Eq for DACertificate<TYPES> {}
