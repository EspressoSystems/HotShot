use ark_bls12_381::Parameters as Param381;
use commit::{Commitment, Committable, RawCommitmentBuilder};
use espresso_systems_common::hotshot::tag;
use hotshot_types::{
    certificate::{DACertificate, QuorumCertificate},
    data::LeafType,
    traits::{
        election::{Checked, Election, ElectionConfig, ElectionError, VoteToken},
        node_implementation::NodeType,
        signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey},
    },
};
use jf_primitives::signatures::BLSSignatureScheme;
#[allow(deprecated)]
use nll::nll_todo::nll_todo;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::num::NonZeroU64;

use super::vrf::JfPubKey;

/// Dummy implementation of [`Election`]

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneralStaticCommittee<T, LEAF: LeafType<NodeType = T>, PUBKEY: SignatureKey> {
    /// The nodes participating
    nodes: Vec<PUBKEY>,
    /// Node type phantom
    _type_phantom: PhantomData<T>,
    /// Leaf phantom
    _leaf_phantom: PhantomData<LEAF>,
}

/// static committee using a vrf kp
pub type StaticCommittee<T, LEAF> =
    GeneralStaticCommittee<T, LEAF, JfPubKey<BLSSignatureScheme<Param381>>>;

impl<T, LEAF: LeafType<NodeType = T>, PUBKEY: SignatureKey>
    GeneralStaticCommittee<T, LEAF, PUBKEY>
{
    /// Creates a new dummy elector
    pub fn new(nodes: Vec<PUBKEY>) -> Self {
        Self {
            nodes,
            _type_phantom: PhantomData,
            _leaf_phantom: PhantomData,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq)]
#[serde(bound(deserialize = ""))]
/// TODO ed - docs
pub struct StaticVoteToken<K: SignatureKey> {
    /// signature
    signature: EncodedSignature,
    /// public key
    pub_key: K,
}

impl<PUBKEY: SignatureKey> VoteToken for StaticVoteToken<PUBKEY> {
    fn vote_count(&self) -> NonZeroU64 {
        NonZeroU64::new(1).unwrap()
    }
}

impl<PUBKEY: SignatureKey> Committable for StaticVoteToken<PUBKEY> {
    fn commit(&self) -> Commitment<Self> {
        RawCommitmentBuilder::new("StaticVoteToken")
            .var_size_field("signature", &self.signature.0)
            .var_size_field("pub_key", &self.pub_key.to_bytes().0)
            .finalize()
    }

    fn tag() -> String {
        tag::STATIC_VOTE_TOKEN.to_string()
    }
}

/// configuration for static committee. stub for now
#[derive(Default, Clone, Serialize, Deserialize, core::fmt::Debug)]
pub struct StaticElectionConfig {}

impl ElectionConfig for StaticElectionConfig {}

impl<TYPES, LEAF: LeafType<NodeType = TYPES>, PUBKEY: SignatureKey + 'static> Election<TYPES>
    for GeneralStaticCommittee<TYPES, LEAF, PUBKEY>
where
    TYPES: NodeType<
        SignatureKey = PUBKEY,
        VoteTokenType = StaticVoteToken<PUBKEY>,
        ElectionConfigType = StaticElectionConfig,
    >,
{
    /// Just use the vector of public keys for the stake table
    type StakeTable = Vec<PUBKEY>;

    type QuorumCertificate = QuorumCertificate<TYPES, Self::LeafType>;

    type DACertificate = DACertificate<TYPES>;

    type LeafType = LEAF;

    fn is_valid_qc(&self, _qc: Self::QuorumCertificate) -> bool {
        #[allow(deprecated)]
        nll_todo()
    }

    fn is_valid_dac(&self, _qc: Self::DACertificate) -> bool {
        #[allow(deprecated)]
        nll_todo()
    }

    fn is_valid_qc_signature(
        &self,
        _encoded_key: &EncodedPublicKey,
        _encoded_signature: &EncodedSignature,
        _hash: Commitment<Self::LeafType>,
        _view_number: TYPES::Time,
        _vote_token: Checked<TYPES::VoteTokenType>,
    ) -> bool {
        #[allow(deprecated)]
        nll_todo()
    }

    fn is_valid_dac_signature(
        &self,
        _encoded_key: &EncodedPublicKey,
        _encoded_signature: &EncodedSignature,
        _view_number: TYPES::Time,
        _vote_token: Checked<TYPES::VoteTokenType>,
    ) -> bool {
        #[allow(deprecated)]
        nll_todo()
    }

    /// Clone the static table
    fn get_stake_table(
        &self,
        _view_number: TYPES::Time,
        _state: &TYPES::StateType,
    ) -> Self::StakeTable {
        self.nodes.clone()
    }
    /// Index the vector of public keys with the current view number
    fn get_leader(&self, view_number: TYPES::Time) -> PUBKEY {
        let index = (*view_number % self.nodes.len() as u64) as usize;
        self.nodes[index].clone()
    }

    /// Simply make the partial signature
    fn make_vote_token(
        &self,
        view_number: TYPES::Time,
        private_key: &<PUBKEY as SignatureKey>::PrivateKey,
    ) -> std::result::Result<Option<StaticVoteToken<PUBKEY>>, ElectionError> {
        let mut message: Vec<u8> = vec![];
        message.extend(view_number.to_le_bytes());
        let signature = PUBKEY::sign(private_key, &message);
        Ok(Some(StaticVoteToken {
            signature,
            pub_key: PUBKEY::from_private(private_key),
        }))
    }

    fn validate_vote_token(
        &self,
        _view_number: TYPES::Time,
        _pub_key: PUBKEY,
        token: Checked<TYPES::VoteTokenType>,
    ) -> Result<Checked<TYPES::VoteTokenType>, ElectionError> {
        match token {
            Checked::Valid(t) | Checked::Unchecked(t) => Ok(Checked::Valid(t)),
            Checked::Inval(t) => Ok(Checked::Inval(t)),
        }
    }

    fn default_election_config(_num_nodes: u64) -> TYPES::ElectionConfigType {
        StaticElectionConfig {}
    }

    fn create_election(keys: Vec<PUBKEY>, _config: TYPES::ElectionConfigType) -> Self {
        Self {
            nodes: keys,
            _type_phantom: PhantomData,
            _leaf_phantom: PhantomData,
        }
    }

    fn get_threshold(&self) -> NonZeroU64 {
        NonZeroU64::new(((self.nodes.len() as u64 * 2) / 3) + 1).unwrap()
    }
}
