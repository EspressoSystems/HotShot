use commit::{Commitment, Committable, RawCommitmentBuilder};
use hotshot_types::traits::{
    election::{Checked, Election, ElectionConfig, ElectionError, VoteToken},
    node_implementation::NodeTypes,
    signature_key::{
        ed25519::{Ed25519Priv, Ed25519Pub},
        EncodedSignature, SignatureKey,
    },
};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::num::NonZeroU64;

/// Dummy implementation of [`Election`]

#[derive(Clone, Debug)]
pub struct GeneralStaticCommittee<S, PUBKEY, PRIVKEY> {
    /// The nodes participating
    nodes: Vec<PUBKEY>,
    /// State phantom
    _state_phantom: PhantomData<S>,
    /// private key phantom data
    _priv_key_phantom: PhantomData<PRIVKEY>,
}

pub type StaticCommittee<S> = GeneralStaticCommittee<S, Ed25519Pub, Ed25519Priv>;

impl<S, PUBKEY, PRIVKEY> GeneralStaticCommittee<S, PUBKEY, PRIVKEY> {
    /// Creates a new dummy elector
    pub fn new(nodes: Vec<PUBKEY>) -> Self {
        Self {
            nodes,
            _state_phantom: PhantomData,
            _priv_key_phantom: PhantomData,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq)]
/// TODO ed - docs
pub struct StaticVoteToken<K> {
    /// signature
    signature: EncodedSignature,
    /// public key
    pub_key: K,
}

impl<PUBKEY> VoteToken for StaticVoteToken<PUBKEY> {
    fn vote_count(&self) -> NonZeroU64 {
        NonZeroU64::new(1).unwrap()
    }
}

impl<PUBKEY> Committable for StaticVoteToken<PUBKEY> {
    fn commit(&self) -> Commitment<Self> {
        RawCommitmentBuilder::new("StaticVoteToken")
            .var_size_field("signature", &self.signature.0)
            .var_size_field("pub_key", &self.pub_key.to_bytes().0)
            .finalize()
    }
}

/// configuration for static committee. stub for now
#[derive(Default, Clone, Serialize, Deserialize, core::fmt::Debug)]
pub struct StaticElectionConfig {}

impl ElectionConfig for StaticElectionConfig {}

impl<TYPES, PUBKEY, PRIVKEY> Election<TYPES> for GeneralStaticCommittee<TYPES, PUBKEY, PRIVKEY>
where
    TYPES: NodeTypes<
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
        self.nodes[index]
    }

    /// Simply make the partial signature
    fn make_vote_token(
        &self,
        view_number: TYPES::Time,
        private_key: &PRIVKEY,
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
            _state_phantom: PhantomData,
            _priv_key_phantom: PhantomData,
        }
    }

    fn get_threshold(&self) -> NonZeroU64 {
        NonZeroU64::new(((self.nodes.len() as u64 * 2) / 3) + 1).unwrap()
    }
}
