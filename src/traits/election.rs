use crate::data::StateHash;
use phaselock_types::{
    data::{Stage, ViewNumber},
    traits::{
        election::Election,
        signature_key::{
            ed25519::{Ed25519Priv, Ed25519Pub},
            EncodedSignature, SignatureKey,
        },
    },
};
use std::marker::PhantomData;

/// Dummy implementation of [`Election`]
pub struct StaticCommittee<S, const N: usize> {
    /// The nodes participating
    nodes: Vec<Ed25519Pub>,
    /// State phantom
    _state_phantom: PhantomData<S>,
}

impl<S, const N: usize> StaticCommittee<S, N> {
    /// Creates a new dummy elector
    pub fn new(nodes: Vec<Ed25519Pub>) -> Self {
        Self {
            nodes,
            _state_phantom: PhantomData,
        }
    }
}

impl<S, const N: usize> Election<Ed25519Pub, N> for StaticCommittee<S, N>
where
    S: Send + Sync,
{
    /// Just use the vector of public keys for the stake table
    type StakeTable = Vec<Ed25519Pub>;
    /// Arbitrary state type, we don't use it
    type State = ();
    /// Arbitrary state type, we don't use it
    type SelectionThreshold = ();
    /// The vote token is just a signature
    type VoteToken = EncodedSignature;
    /// Same for the validated vote token
    type ValidatedVoteToken = (EncodedSignature, Ed25519Pub);
    /// Clone the static table
    fn get_stake_table(&self, _state: &Self::State) -> Self::StakeTable {
        self.nodes.clone()
    }
    /// Index the vector of public keys with the current view number
    fn get_leader(
        &self,
        table: &Self::StakeTable,
        view_number: ViewNumber,
        _: Stage,
    ) -> Ed25519Pub {
        let index = (*view_number % table.len() as u64) as usize;
        table[index].clone()
    }
    /// Simply verify the signature and check the membership list
    fn get_votes(
        &self,
        table: &Self::StakeTable,
        _selection_threshold: Self::SelectionThreshold,
        view_number: ViewNumber,
        pub_key: Ed25519Pub,
        token: Self::VoteToken,
        next_state: StateHash<N>,
    ) -> Option<Self::ValidatedVoteToken> {
        let mut message: Vec<u8> = vec![];
        message.extend(&view_number.to_le_bytes());
        message.extend(next_state.as_ref());
        if pub_key.validate(&token, &message) && table.contains(&pub_key) {
            Some((token, pub_key))
        } else {
            None
        }
    }
    /// Simply make the partial signature
    fn make_vote_token(
        &self,
        _table: &Self::StakeTable,
        _selection_threshold: Self::SelectionThreshold,
        view_number: ViewNumber,
        private_key: &Ed25519Priv,
        next_state: StateHash<N>,
    ) -> Option<Self::VoteToken> {
        let mut message: Vec<u8> = vec![];
        message.extend(&view_number.to_le_bytes());
        message.extend(next_state.as_ref());
        let token = Ed25519Pub::sign(private_key, &message);
        Some(token)
    }
    /// If its a validated token, it always has one vote
    fn get_vote_count(&self, _token: &Self::ValidatedVoteToken) -> u64 {
        1
    }
}
