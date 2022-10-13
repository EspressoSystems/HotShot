use commit::Commitment;
use hotshot_types::{
    data::{Leaf, ViewNumber},
    traits::{
        election::{Election, VoteToken, ElectionError, Checked},
        signature_key::{
            ed25519::{Ed25519Priv, Ed25519Pub},
            EncodedSignature, SignatureKey,
        },
        state::ConsensusTime,
        State,
    },
};
use hotshot_utils::hack::nll_todo;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

/// Dummy implementation of [`Election`]
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
pub struct StaticVoteToken {
    signature: EncodedSignature,
    pub_key: Ed25519Pub
}

impl VoteToken for StaticVoteToken {
    fn vote_count(&self) -> u64 {
        nll_todo()
    }
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
    /// Clone the static table
    fn get_stake_table(&self, view_number: ViewNumber, _state: &Self::StateType) -> Self::StakeTable {
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
        nll_todo()
        // let mut message: Vec<u8> = vec![];
        // message.extend(&view_number.to_le_bytes());
        // message.extend(next_state.as_ref());
        // let token = Ed25519Pub::sign(private_key, &message);
        // Ok(Some(token))
    }

    fn validate_vote_token(
        &self,
        view_number: ViewNumber,
        pub_key: Ed25519Pub,
        token: Checked<Self::VoteTokenType>,
        next_state: commit::Commitment<hotshot_types::data::Leaf<Self::StateType>>,
    ) -> Result<hotshot_types::traits::election::Checked<Self::VoteTokenType>, hotshot_types::traits::election::ElectionError> {
        nll_todo()
    }

    // If its a validated token, it always has one vote
    // fn get_vote_count(&self, _token: &Self::ValidatedVoteToken) -> u64 {
    //     1
    // }
}
