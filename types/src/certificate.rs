//! Provides two types of cerrtificates and their accumulators.

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
use tracing::error;
use bincode::Options;
use commit::{Commitment, Committable};
use espresso_systems_common::hotshot::tag;
use hotshot_utils::bincode::bincode_opts;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display, Formatter};
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
    pub signatures: YesNoSignature<TYPES::BlockType, TYPES::VoteTokenType>, // no genesis bc not meaningful
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
    /// Threshold Signature
    pub signatures: YesNoSignature<LEAF, TYPES::VoteTokenType>,
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

#[derive(custom_debug::Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq, Hash)]
#[serde(bound(deserialize = ""))]
pub enum ViewSyncCertificate<TYPES: NodeType> {
    PreCommit(ViewSyncCertificateInternal<TYPES>),
    Commit(ViewSyncCertificateInternal<TYPES>),
    Finalize(ViewSyncCertificateInternal<TYPES>),
}

impl<TYPES: NodeType> ViewSyncCertificate<TYPES> {
    pub fn as_bytes(&self) -> Vec<u8> {
        bincode_opts().serialize(&self).unwrap()
    }
}

/// A view sync certificate representing a quorum of votes for a particular view sync phase
#[derive(custom_debug::Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq, Hash)]
#[serde(bound(deserialize = ""))]
pub struct ViewSyncCertificateInternal<TYPES: NodeType> {
    /// Relay the votes are intended for
    pub relay: u64,
    /// View number the network is attempting to synchronize on
    pub round: TYPES::Time,
    /// Threshold Signature
    pub signatures: YesNoSignature<ViewSyncData<TYPES>, TYPES::VoteTokenType>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash)]
#[serde(bound(deserialize = ""))]
/// Enum representing whether a QC's signatures are for a 'Yes' or 'No' QC
// TODO ED Rename these types to be clearer
pub enum YesNoSignature<LEAF: Committable + Serialize + Clone, TOKEN: VoteToken> {
    /// These signatures are for a 'Yes' QC
    /// Means these signatures will all be the same type - Yes signatures
    Yes(BTreeMap<EncodedPublicKey, (EncodedSignature, VoteData<LEAF>, TOKEN)>),
    /// These signatures are for a 'No' QC
    /// Means these signatures could be a combination of either Yes or No signatures
    No(BTreeMap<EncodedPublicKey, (EncodedSignature, VoteData<LEAF>, TOKEN)>),

    ViewSyncPreCommit(BTreeMap<EncodedPublicKey, (EncodedSignature, VoteData<LEAF>, TOKEN)>),

    ViewSyncCommit(BTreeMap<EncodedPublicKey, (EncodedSignature, VoteData<LEAF>, TOKEN)>),

    ViewSyncFinalize(BTreeMap<EncodedPublicKey, (EncodedSignature, VoteData<LEAF>, TOKEN)>),
}

/// Data from a vote needed to accumulate into a `SignedCertificate`
pub struct VoteMetaData<COMMITTABLE: Committable + Serialize + Clone, T: VoteToken, TIME> {
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
    /// The relay index for view sync
    // TODO ED Make VoteMetaData more generic to avoid this variable that only ViewSync uses
    pub relay: Option<u64>,
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>>
    SignedCertificate<TYPES::SignatureKey, TYPES::Time, TYPES::VoteTokenType, LEAF>
    for QuorumCertificate<TYPES, LEAF>
{
    fn from_signatures_and_commitment(
        view_number: TYPES::Time,
        signatures: YesNoSignature<LEAF, TYPES::VoteTokenType>,
        commit: Commitment<LEAF>,
        relay: Option<u64>,
    ) -> Self {
        let qc = QuorumCertificate {
            leaf_commitment: commit,
            view_number,
            signatures,
            is_genesis: false,
        }; 
        error!("QC commitment when formed is {:?}", qc.leaf_commitment);
        qc
    }

    fn view_number(&self) -> TYPES::Time {
        self.view_number
    }

    fn signatures(&self) -> YesNoSignature<LEAF, TYPES::VoteTokenType> {
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
            signatures: YesNoSignature::Yes(BTreeMap::default()),
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

        let signatures = match self.signatures.clone() {
            YesNoSignature::Yes(signatures) => {
                builder = builder.var_size_field("QC Type", "Yes".as_bytes());
                signatures
            }
            YesNoSignature::No(signatures) => {
                builder = builder.var_size_field("QC Type", "No".as_bytes());
                signatures
            }
            YesNoSignature::ViewSyncPreCommit(_)
            | YesNoSignature::ViewSyncCommit(_)
            | YesNoSignature::ViewSyncFinalize(_) => unimplemented!(),
        };
        for (idx, (k, v)) in signatures.iter().enumerate() {
            builder = builder
                .var_size_field(&format!("Signature {idx} public key"), &k.0)
                .var_size_field(&format!("Signature {idx} signature"), &v.0 .0)
                .var_size_field(&format!("Signature {idx} vote data"), &v.1.as_bytes())
                .field(&format!("Signature {idx} vote token"), v.2.commit());
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
        signatures: YesNoSignature<TYPES::BlockType, TYPES::VoteTokenType>,
        commit: Commitment<TYPES::BlockType>,
        relay: Option<u64>,
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

    fn signatures(&self) -> YesNoSignature<TYPES::BlockType, TYPES::VoteTokenType> {
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

impl<TYPES: NodeType> Committable for ViewSyncCertificate<TYPES> {
    fn commit(&self) -> Commitment<Self> {
        let mut builder = commit::RawCommitmentBuilder::new("View Sync Certificate Commitment");

        // builder = builder
        //     .field("Leaf commitment", self.leaf_commitment)
        //     .u64_field("View number", *self.view_number.deref());

        let certificate_internal = match &self {
            // TODO ED Not the best way to do this
            ViewSyncCertificate::PreCommit(certificate_internal) => {
                builder = builder.var_size_field("View Sync Phase", "PreCommit".as_bytes());
                certificate_internal
            }
            ViewSyncCertificate::Commit(certificate_internal) => {
                builder = builder.var_size_field("View Sync Phase", "Commit".as_bytes());
                certificate_internal
            }
            ViewSyncCertificate::Finalize(certificate_internal) => {
                builder = builder.var_size_field("View Sync Phase", "Finalize".as_bytes());
                certificate_internal
            }
        };
        let signatures = match self.signatures().clone() {
            YesNoSignature::ViewSyncPreCommit(signatures) => {
                builder =
                    builder.var_size_field("Signature View Sync Phase", "PreCommit".as_bytes());
                signatures
            }
            YesNoSignature::ViewSyncCommit(signatures) => {
                builder = builder.var_size_field("Signature View Sync Phase", "Commit".as_bytes());
                signatures
            }
            YesNoSignature::ViewSyncFinalize(signatures) => {
                builder =
                    builder.var_size_field("Signature View Sync Phase", "Finalize".as_bytes());
                signatures
            }
            _ => unimplemented!(),
        };
        for (idx, (k, v)) in signatures.iter().enumerate() {
            builder = builder
                .var_size_field(&format!("Signature {idx} public key"), &k.0)
                .var_size_field(&format!("Signature {idx} signature"), &v.0 .0)
                .var_size_field(&format!("Signature {idx} vote data"), &v.1.as_bytes())
                .field(&format!("Signature {idx} vote token"), v.2.commit());
        }

        builder = builder
            .u64_field("Relay", certificate_internal.relay)
            .u64_field("Round", *certificate_internal.round);
        builder.finalize()
    }

    fn tag() -> String {
        // TODO ED Update this repo with a view sync tag
        tag::QC.to_string()
    }
}

impl<TYPES: NodeType>
    SignedCertificate<TYPES::SignatureKey, TYPES::Time, TYPES::VoteTokenType, ViewSyncData<TYPES>>
    for ViewSyncCertificate<TYPES>
{
    /// Build a QC from the threshold signature and commitment
    fn from_signatures_and_commitment(
        view_number: TYPES::Time,
        signatures: YesNoSignature<ViewSyncData<TYPES>, TYPES::VoteTokenType>,
        commit: Commitment<ViewSyncData<TYPES>>,
        relay: Option<u64>,
    ) -> Self {
        let certificate_internal = ViewSyncCertificateInternal {
            round: view_number,
            relay: relay.unwrap(),
            signatures: signatures.clone(),
        };
        match signatures {
            YesNoSignature::ViewSyncPreCommit(_) => {
                ViewSyncCertificate::PreCommit(certificate_internal)
            }
            YesNoSignature::ViewSyncCommit(_) => ViewSyncCertificate::Commit(certificate_internal),
            YesNoSignature::ViewSyncFinalize(_) => {
                ViewSyncCertificate::Finalize(certificate_internal)
            }
            _ => unimplemented!(),
        }
    }

    /// Get the view number.
    fn view_number(&self) -> TYPES::Time {
        match self.clone() {
            ViewSyncCertificate::PreCommit(certificate_internal)
            | ViewSyncCertificate::Commit(certificate_internal)
            | ViewSyncCertificate::Finalize(certificate_internal) => certificate_internal.round,
        }
    }

    /// Get signatures.
    fn signatures(&self) -> YesNoSignature<ViewSyncData<TYPES>, TYPES::VoteTokenType> {
        match self.clone() {
            ViewSyncCertificate::PreCommit(certificate_internal)
            | ViewSyncCertificate::Commit(certificate_internal)
            | ViewSyncCertificate::Finalize(certificate_internal) => {
                certificate_internal.signatures
            }
        }
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
