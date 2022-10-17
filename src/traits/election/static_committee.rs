use commit::Commitment;
use hotshot_types::{
    data::{Leaf, ViewNumber},
    traits::{
        election::{Checked, Election, ElectionError, VoteToken, ElectionConfig},
        signature_key::{
            ed25519::{Ed25519Priv, Ed25519Pub},
            EncodedSignature, SignatureKey,
        },
        state::ConsensusTime,
        State,
    },
};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

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

#[derive(Serialize, Deserialize, Clone, Debug)]
/// TODO ed - docs
pub struct StaticVoteToken {
    /// signature
    signature: EncodedSignature,
    /// public key
    pub_key: Ed25519Pub,
}

impl VoteToken for StaticVoteToken {
    fn vote_count(&self) -> u64 {
        1
    }
}

/// configuration for static committee. stub for now
#[derive(Default, Clone, Serialize, Deserialize, core::fmt::Debug)]
pub struct StaticElectionConfig {
}

impl ElectionConfig for StaticElectionConfig {
}

impl<S, T> Election<Ed25519Pub, T> for StaticCommittee<S>
where
    S: Send + Sync + State,
    T: ConsensusTime,
{
    /// Just use the vector of public keys for the stake table
    type StakeTable = Vec<Ed25519Pub>;
    type StateType = S;
    type VoteTokenType = StaticVoteToken;

    type ElectionConfigType = StaticElectionConfig;

    /// Clone the static table
    fn get_stake_table(
        &self,
        _view_number: ViewNumber,
        _state: &Self::StateType,
    ) -> Self::StakeTable {
        self.nodes.clone()
    }
    /// Index the vector of public keys with the current view number
    fn get_leader(&self, view_number: ViewNumber) -> Ed25519Pub {
        let index = (*view_number % self.nodes.len() as u64) as usize;
        self.nodes[index]
    }

    /// Simply verify the signature and check the membership list
    // fn get_votes(
    //     &self,
    //     view_number: ViewNumber,
    //     pub_key: Ed25519Pub,
    //     token: Self::VoteToken,
    //     next_state: Commitment<Leaf<Self::StateType>>,
    // ) -> Option<Self::ValidatedVoteToken> {
    //     let mut message: Vec<u8> = vec![];
    //     message.extend(&view_number.to_le_bytes());
    //     message.extend(next_state.as_ref());
    //     if pub_key.validate(&token, &message) && self.nodes.contains(&pub_key) {
    //         Some((token, pub_key))
    //     } else {
    //         None
    //     }
    // }

    /// Simply make the partial signature
    fn make_vote_token(
        &self,
        view_number: ViewNumber,
        private_key: &Ed25519Priv,
        next_state: Commitment<Leaf<Self::StateType>>,
    ) -> std::result::Result<std::option::Option<StaticVoteToken>, ElectionError> {
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
        _view_number: ViewNumber,
        _pub_key: Ed25519Pub,
        token: Checked<Self::VoteTokenType>,
        _next_state: commit::Commitment<hotshot_types::data::Leaf<Self::StateType>>,
    ) -> Result<
        hotshot_types::traits::election::Checked<Self::VoteTokenType>,
        hotshot_types::traits::election::ElectionError,
    > {
        match token {
            Checked::Valid(t)| Checked::Unchecked(t) => Ok(Checked::Valid(t)),
            Checked::Inval(t) => Ok(Checked::Inval(t))
        }
    }

    fn default_election_config(_num_nodes: u64) -> Self::ElectionConfigType {
        StaticElectionConfig {}
    }

    fn create_election(keys: Vec<Ed25519Pub>, _config: Self::ElectionConfigType) -> Self {
        Self {
            nodes: keys,
            _state_phantom: PhantomData
        }
    }

}
