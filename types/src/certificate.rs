//! Provides two types of cerrtificates and their accumulators.

use crate::data::serialize_signature;
use crate::vote::ViewSyncData;
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
use std::fmt::{self, Display, Formatter};
use std::{fmt::Debug, ops::Deref};

// NOTE Sishan: For signature aggregation
use hotshot_primitives::quorum_certificate::{
    BitvectorQuorumCertificate, QuorumCertificateValidation, StakeTableEntry
};
use jf_primitives::signatures::bls_over_bn254::{
    BLSOverBN254CurveSignatureScheme, VerKey,
};
use jf_primitives::signatures::{SignatureScheme};

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
    // pub signatures: YesNoSignature<TYPES::BlockType, TYPES::VoteTokenType>, // no genesis bc not meaningful
    /// Sishan NOTE: for QC aggregation
    pub signatures: QCAssembledSignature,
}

/// The type used for Quorum Certificates
///
/// A Quorum Certificate is a threshold signature of the `Leaf` being proposed, as well as some
/// metadata, such as the `Stage` of consensus the quorum certificate was generated during.
#[derive(custom_debug::Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq, Hash)]
#[serde(bound(deserialize = ""))]
pub struct QuorumCertificate<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> {
    /// commitment to previous leaf
    #[debug(skip)]
    pub leaf_commitment: Commitment<LEAF>,

    /// Which view this QC relates to
    pub view_number: TYPES::Time,
    /// Sishan NOTE: QC for QC aggregation
    pub signatures: QCAssembledSignature,
    /// If this QC is for the genesis block
    pub is_genesis: bool,
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> Display for QuorumCertificate<TYPES, LEAF> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "view: {:?}, is_genesis: {:?}",
            self.view_number, self.is_genesis
        )
    }
}

/// A view sync certificate representing a quorum of votes for a particular view sync phase
#[derive(custom_debug::Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq, Hash)]
#[serde(bound(deserialize = ""))]
pub struct ViewSyncCertificate<TYPES: NodeType> {
    /// Relay the votes are intended for
    pub relay: EncodedPublicKey,
    /// View number the network is attempting to synchronize on
    pub round: TYPES::Time,
    /// Aggregated QC 
    pub signatures: QCAssembledSignature,
}


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash)]
#[serde(bound(deserialize = ""))]
/// Enum representing whether a QC's signatures are for a 'Yes' or 'No' or 'DA' or 'Genesis' QC
pub enum QCAssembledSignature {
    /// These signatures are for a 'Yes' QC
    Yes((<BLSOverBN254CurveSignatureScheme as SignatureScheme>::Signature, 
        <BitvectorQuorumCertificate<BLSOverBN254CurveSignatureScheme> as QuorumCertificateValidation<BLSOverBN254CurveSignatureScheme>>::Proof)),
    /// These signatures are for a 'No' QC
    No((<BLSOverBN254CurveSignatureScheme as SignatureScheme>::Signature, 
        <BitvectorQuorumCertificate<BLSOverBN254CurveSignatureScheme> as QuorumCertificateValidation<BLSOverBN254CurveSignatureScheme>>::Proof)),
    /// These signatures are for a 'DA' QC
    DA((<BLSOverBN254CurveSignatureScheme as SignatureScheme>::Signature, 
        <BitvectorQuorumCertificate<BLSOverBN254CurveSignatureScheme> as QuorumCertificateValidation<BLSOverBN254CurveSignatureScheme>>::Proof)),
    /// These signatures are for genesis QC
    Genesis(),
}

/// Data from a vote needed to accumulate into a `SignedCertificate`
pub struct VoteMetaData<COMMITTABLE: Committable + Serialize + Clone, T: VoteToken, TIME> {
    /// Voter's public key
    /// Sishan NOTE TODO: In qc aggregation, this encoded_key is substitued by the ver_key, should be discarded later
    pub encoded_key: EncodedPublicKey,
    /// Votes signature
    pub encoded_signature: EncodedSignature,
    /// Sishan NOTE: entry with public key for QC aggregation
    pub entry: StakeTableEntry<VerKey>,
    /// Voter's public key under QC KeyPair Signature Scheme
    pub ver_key: VerKey,
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
        signatures: QCAssembledSignature,
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

    fn signatures(&self) -> QCAssembledSignature {
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
            signatures: QCAssembledSignature::Genesis(),
            is_genesis: true,
        }
    }
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> Eq for QuorumCertificate<TYPES, LEAF> {}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> Committable
    for QuorumCertificate<TYPES, LEAF>
{
    fn commit(&self) -> Commitment<Self> {
        let signatures_bytes = serialize_signature(&self.signatures);

        commit::RawCommitmentBuilder::new("Quorum Certificate Commitment")
            .field("leaf commitment", self.leaf_commitment)
            .u64_field("view number", *self.view_number.deref())
            .constant_str("justify_qc signatures")
            .var_size_bytes(&signatures_bytes)
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
        signatures: QCAssembledSignature,
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

    fn signatures(&self) -> QCAssembledSignature {
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

impl<TYPES: NodeType>
    SignedCertificate<TYPES::SignatureKey, TYPES::Time, TYPES::VoteTokenType, ViewSyncData<TYPES>>
    for ViewSyncCertificate<TYPES>
{
    /// Build a QC from the threshold signature and commitment
    fn from_signatures_and_commitment(
        _view_number: TYPES::Time,
        _signatures: QCAssembledSignature,
        _commit: Commitment<ViewSyncData<TYPES>>,
    ) -> Self {
        todo!()
    }

    /// Get the view number.
    fn view_number(&self) -> TYPES::Time {
        todo!()
    }

    /// Get signatures.
    fn signatures(&self) -> QCAssembledSignature {
        todo!()
    }

    // TODO (da) the following functions should be refactored into a QC-specific trait.

    /// Get the leaf commitment.
    fn leaf_commitment(&self) -> Commitment<ViewSyncData<TYPES>> {
        todo!()
    }

    /// Set the leaf commitment.
    fn set_leaf_commitment(&mut self, _commitment: Commitment<ViewSyncData<TYPES>>) {
        todo!()
    }

    /// Get whether the certificate is for the genesis block.
    fn is_genesis(&self) -> bool {
        todo!()
    }

    /// To be used only for generating the genesis quorum certificate; will fail if used anywhere else
    fn genesis() -> Self {
        todo!()
    }
}
impl<TYPES: NodeType> Eq for ViewSyncCertificate<TYPES> {}
