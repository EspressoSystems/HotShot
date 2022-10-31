use commit::{Commitment, Committable, RawCommitmentBuilder};
use hotshot_types::{
    data::Leaf,
    traits::{
        election::{Checked, Election, ElectionConfig, ElectionError, VoteToken},
        node_implementation::NodeTypes,
        signature_key::{
            ed25519::{Ed25519Priv, Ed25519Pub},
            EncodedSignature, SignatureKey,
        },
    },
};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::num::NonZeroU64;

/// Dummy implementation of [`Election`]

#[derive(Clone, Debug)]
pub struct StaticCommittee<S> {
    /// The nodes participating
    nodes: Vec<Ed25519Pub>,
    /// State phantom
    _state_phantom: PhantomData<S>,
}

impl<S> StaticCommittee<S> {
    /// Creates a new dummy elector
    pub fn new(nodes: Vec<Ed25519Pub>) -> Self {
        Self {
            nodes,
            _state_phantom: PhantomData,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq)]
/// TODO ed - docs
pub struct StaticVoteToken {
    /// signature
    signature: EncodedSignature,
    /// public key
    pub_key: Ed25519Pub,
}

impl VoteToken for StaticVoteToken {
    fn vote_count(&self) -> NonZeroU64 {
        NonZeroU64::new(1).unwrap()
    }
}

impl Committable for StaticVoteToken {
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

impl<TYPES> Election<TYPES> for StaticCommittee<TYPES>
where
    TYPES: NodeTypes<
        SignatureKey = Ed25519Pub,
        VoteTokenType = StaticVoteToken,
        ElectionConfigType = StaticElectionConfig,
    >,
{
    /// Just use the vector of public keys for the stake table
    type StakeTable = Vec<Ed25519Pub>;

    /// Clone the static table
    fn get_stake_table(
        &self,
        _view_number: TYPES::Time,
        _state: &TYPES::StateType,
    ) -> Self::StakeTable {
        self.nodes.clone()
    }
    /// Index the vector of public keys with the current view number
    fn get_leader(&self, time: TYPES::Time) -> Ed25519Pub {
        let index = (*time % self.nodes.len() as u64) as usize;
        self.nodes[index]
    }

    /// Simply make the partial signature
    fn make_vote_token(
        &self,
        view_number: TYPES::Time,
        private_key: &Ed25519Priv,
        next_state: Commitment<Leaf<TYPES>>,
    ) -> Result<Option<StaticVoteToken>, ElectionError> {
        let mut message: Vec<u8> = vec![];
        message.extend(&view_number.to_le_bytes());
        message.extend(next_state.as_ref());
        let signature = Ed25519Pub::sign(private_key, &message);
        Ok(Some(StaticVoteToken {
            signature,
            pub_key: Ed25519Pub::from_private(private_key),
        }))
    }

    fn validate_vote_token(
        &self,
        _view_number: TYPES::Time,
        _pub_key: Ed25519Pub,
        token: Checked<TYPES::VoteTokenType>,
        _next_state: commit::Commitment<Leaf<TYPES>>,
    ) -> Result<Checked<TYPES::VoteTokenType>, ElectionError> {
        match token {
            Checked::Valid(t) | Checked::Unchecked(t) => Ok(Checked::Valid(t)),
            Checked::Inval(t) => Ok(Checked::Inval(t)),
        }
    }

    fn default_election_config(_num_nodes: u64) -> TYPES::ElectionConfigType {
        StaticElectionConfig {}
    }

    fn create_election(keys: Vec<Ed25519Pub>, _config: TYPES::ElectionConfigType) -> Self {
        Self {
            nodes: keys,
            _state_phantom: PhantomData,
        }
    }

    fn get_threshold(&self) -> NonZeroU64 {
        NonZeroU64::new(((self.nodes.len() as u64 * 2) / 3) + 1).unwrap()
    }
}
