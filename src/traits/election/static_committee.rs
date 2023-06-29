use super::vrf::JfPubKey;
// use ark_bls12_381::Parameters as Param381;
use commit::{Commitment, Committable, RawCommitmentBuilder};
use espresso_systems_common::hotshot::tag;
use hotshot_types::{
    data::LeafType,
    traits::{
        election::{Checked, ElectionConfig, ElectionError, Membership, VoteToken},
        node_implementation::NodeType,
        signature_key::{EncodedSignature, SignatureKey},
    },
};
use jf_primitives::signatures::BLSSignatureScheme;
// Sishan NOTE: for QC aggregation
use jf_primitives::signatures::bls_over_bn254::{BLSOverBN254CurveSignatureScheme, KeyPair as QCKeyPair};
#[allow(deprecated)]
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::num::NonZeroU64;

/// Dummy implementation of [`Membership`]

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneralStaticCommittee<T, LEAF: LeafType<NodeType = T>, PUBKEY: SignatureKey> {
    /// All the nodes participating
    nodes: Vec<PUBKEY>,
    /// The nodes on the static committee
    committee_nodes: Vec<PUBKEY>,
    /// Node type phantom
    _type_phantom: PhantomData<T>,
    /// Leaf phantom
    _leaf_phantom: PhantomData<LEAF>,
}

/// static committee using a vrf kp
pub type StaticCommittee<T, LEAF> = GeneralStaticCommittee<T, LEAF, JfPubKey<BLSOverBN254CurveSignatureScheme>>;

impl<T, LEAF: LeafType<NodeType = T>, PUBKEY: SignatureKey>
    GeneralStaticCommittee<T, LEAF, PUBKEY>
{
    /// Creates a new dummy elector
    #[must_use]
    pub fn new(nodes: Vec<PUBKEY>) -> Self {
        Self {
            nodes: nodes.clone(),
            committee_nodes: nodes,
            _type_phantom: PhantomData,
            _leaf_phantom: PhantomData,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq)]
#[serde(bound(deserialize = ""))]
/// Vote token for a static committee
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
pub struct StaticElectionConfig {
    /// Number of nodes on the committee
    num_nodes: u64,
}

impl ElectionConfig for StaticElectionConfig {}

impl<TYPES, LEAF: LeafType<NodeType = TYPES>, PUBKEY: SignatureKey + 'static> Membership<TYPES>
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
        key_pair_test: QCKeyPair,
    ) -> std::result::Result<Option<StaticVoteToken<PUBKEY>>, ElectionError> {
        // TODO ED Below
        let pub_key = PUBKEY::from_private(private_key);
        if !self.committee_nodes.contains(&pub_key) {
            return Ok(None);
        }
        let mut message: Vec<u8> = vec![];
        message.extend(view_number.to_le_bytes());
        let signature = PUBKEY::sign(private_key, key_pair_test.clone(), &message);
        Ok(Some(StaticVoteToken { signature, pub_key }))
    }

    fn validate_vote_token(
        &self,
        _view_number: TYPES::Time,
        pub_key: PUBKEY,
        token: Checked<TYPES::VoteTokenType>,
    ) -> Result<Checked<TYPES::VoteTokenType>, ElectionError> {
        match token {
            Checked::Valid(t) | Checked::Unchecked(t) => {
                if !self.committee_nodes.contains(&pub_key) {
                    Ok(Checked::Inval(t))
                } else {
                    Ok(Checked::Valid(t))
                }
            }
            Checked::Inval(t) => Ok(Checked::Inval(t)),
        }
    }

    fn default_election_config(num_nodes: u64) -> TYPES::ElectionConfigType {
        StaticElectionConfig { num_nodes }
    }

    fn create_election(keys: Vec<PUBKEY>, config: TYPES::ElectionConfigType) -> Self {
        let mut committee_nodes = keys.clone();
        committee_nodes.truncate(config.num_nodes.try_into().unwrap());
        Self {
            nodes: keys,
            committee_nodes,
            _type_phantom: PhantomData,
            _leaf_phantom: PhantomData,
        }
    }

    fn success_threshold(&self) -> NonZeroU64 {
        NonZeroU64::new(((self.committee_nodes.len() as u64 * 2) / 3) + 1).unwrap()
    }

    fn failure_threshold(&self) -> NonZeroU64 {
        NonZeroU64::new(((self.committee_nodes.len() as u64) / 3) + 1).unwrap()
    }

    fn get_committee(
        &self,
        _view_number: <TYPES as NodeType>::Time,
    ) -> std::collections::BTreeSet<<TYPES as NodeType>::SignatureKey> {
        self.committee_nodes.clone().into_iter().collect()
    }
}
