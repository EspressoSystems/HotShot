//! Provides two types of cerrtificates and their accumulators.

use crate::{
    data::{fake_commitment, LeafType},
    traits::{
        election::{SignedCertificate, VoteData, VoteToken},
        node_implementation::NodeType,
        signature_key::{EncodedPublicKey, EncodedSignature},
        state::ConsensusTime,
    },
};
use commit::{Commitment, Committable};
use espresso_systems_common::hotshot::tag;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt::Debug, ops::Deref};

/// A `DACertificate` is a threshold signature that some data is available.
/// It is signed by the members of the DA committee, not the entire network. It is used
/// to prove that the data will be made available to those outside of the DA committee.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Hash)]
pub struct DACertificate<TYPES: NodeType> {
    /// The view number this quorum certificate was generated during
    ///
    /// This value is covered by the threshold signature.
    pub view_number: TYPES::Time,

    /// committment to the block
    pub block_commitment: Commitment<TYPES::BlockType>,

    /// The list of signatures establishing the validity of this Quorum Certifcate
    ///
    /// This is a mapping of the byte encoded public keys provided by the [`crate::traits::node_implementation::NodeImplementation`], to
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
/// A Quorum Certificate is a threshold signature of the `Leaf` being proposed, as well as some
/// metadata, such as the `Stage` of consensus the quorum certificate was generated during.
#[derive(custom_debug::Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq, Hash)]
#[serde(bound(deserialize = ""))]
pub struct QuorumCertificate<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> {
    // block commitment is contained within the leaf. Still need to check this
    /// TODO (da) we need to check
    ///   - parent QC PROPOSAL
    ///   - somehow make this semantically equivalent to what is currently `Leaf`
    #[debug(skip)]
    pub leaf_commitment: Commitment<LEAF>,

    /// Which view this QC relates to
    pub view_number: TYPES::Time,
    /// Threshold Signature
    pub signatures: BTreeMap<EncodedPublicKey, (EncodedSignature, TYPES::VoteTokenType)>,
    /// If this QC is for the genesis block
    pub is_genesis: bool,
}

/// Data from a vote needed to accumulate into a `SignedCertificate`
pub struct VoteMetaData<COMMITTABLE: Committable + Serialize, T: VoteToken, TIME> {
    /// Voter's public key
    pub encoded_key: EncodedPublicKey,
    /// Votes signature
    pub encoded_signature: EncodedSignature,
    /// Commitment to what's voted on.  E.g. the leaf for a `QuorumCertificate`
    pub commitment: Commitment<COMMITTABLE>,
    /// Data of the vote, yes, no, timeout, or DA
    pub data: VoteData<COMMITTABLE>,
    /// The votes's token
    pub vote_token: T,
    /// View number for the vote
    pub view_number: TIME,
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>>
    SignedCertificate<TYPES::SignatureKey, TYPES::Time, TYPES::VoteTokenType, LEAF>
    for QuorumCertificate<TYPES, LEAF>
{
    fn from_signatures_and_commitment(
        view_number: TYPES::Time,
        signatures: BTreeMap<EncodedPublicKey, (EncodedSignature, TYPES::VoteTokenType)>,
        commit: Commitment<LEAF>,
    ) -> Self {
        QuorumCertificate {
            leaf_commitment: commit,
            view_number,
            signatures,
            is_genesis: false,
        }
    }

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

impl<TYPES: NodeType>
    SignedCertificate<TYPES::SignatureKey, TYPES::Time, TYPES::VoteTokenType, TYPES::BlockType>
    for DACertificate<TYPES>
{
    fn from_signatures_and_commitment(
        view_number: TYPES::Time,
        signatures: BTreeMap<EncodedPublicKey, (EncodedSignature, TYPES::VoteTokenType)>,
        commit: Commitment<TYPES::BlockType>,
    ) -> Self {
        DACertificate {
            view_number,
            signatures,
            block_commitment: commit,
        }
    }

    fn view_number(&self) -> TYPES::Time {
        self.view_number
    }

    fn signatures(&self) -> BTreeMap<EncodedPublicKey, (EncodedSignature, TYPES::VoteTokenType)> {
        self.signatures.clone()
    }

    fn leaf_commitment(&self) -> Commitment<TYPES::BlockType> {
        self.block_commitment
    }

    fn set_leaf_commitment(&mut self, _commitment: Commitment<TYPES::BlockType>) {
        // This function is only useful for QC. Will be removed after we have separated cert traits.
    }

    fn is_genesis(&self) -> bool {
        // This function is only useful for QC. Will be removed after we have separated cert traits.
        false
    }

    fn genesis() -> Self {
        // This function is only useful for QC. Will be removed after we have separated cert traits.
        unimplemented!()
    }
}

impl<TYPES: NodeType> Eq for DACertificate<TYPES> {}
