use crate::data::StateHash;
use crate::{PrivKey, PubKey};
use phaselock_types::traits::election::Election;
use std::marker::PhantomData;

/// Dummy implementation of [`Election`]
pub struct StaticCommittee<S, const N: usize> {
    /// The nodes participating
    nodes: Vec<PubKey>,
    /// State phantom
    _state_phantom: PhantomData<S>,
}

impl<S, const N: usize> StaticCommittee<S, N> {
    /// Creates a new dummy elector
    pub fn new(nodes: Vec<PubKey>) -> Self {
        Self {
            nodes,
            _state_phantom: PhantomData,
        }
    }
}

impl<S, const N: usize> Election<N> for StaticCommittee<S, N>
where
    S: Send + Sync,
{
    /// Just use the vector of public keys for the stake table
    type StakeTable = Vec<PubKey>;
    /// Arbitrary state type, we don't use it
    type State = ();
    /// Arbitrary state type, we don't use it
    type SelectionThreshold = ();
    /// The vote token is just a signature
    type VoteToken = threshold_crypto::SignatureShare;
    /// Same for the validated vote token
    type ValidatedVoteToken = (threshold_crypto::SignatureShare, PubKey);
    /// Clone the static table
    fn get_stake_table(&self, _state: &Self::State) -> Self::StakeTable {
        self.nodes.clone()
    }
    /// Index the vector of public keys with the current view number
    fn get_leader(&self, table: &Self::StakeTable, view_number: u64) -> PubKey {
        let index = (view_number % table.len() as u64) as usize;
        table[index].clone()
    }
    /// Simply verify the signature and check the membership list
    fn get_votes(
        &self,
        table: &Self::StakeTable,
        _selection_threshold: Self::SelectionThreshold,
        view_number: u64,
        pub_key: PubKey,
        token: Self::VoteToken,
        next_state: StateHash<N>,
    ) -> Option<Self::ValidatedVoteToken> {
        let mut message: Vec<u8> = vec![];
        message.extend(&view_number.to_le_bytes());
        message.extend(next_state.as_ref());
        if pub_key.node.verify(&token, message) && table.contains(&pub_key) {
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
        view_number: u64,
        private_key: &PrivKey,
        next_state: StateHash<N>,
    ) -> Option<Self::VoteToken> {
        let mut message: Vec<u8> = vec![];
        message.extend(&view_number.to_le_bytes());
        message.extend(next_state.as_ref());
        let token = private_key.node.sign(message);
        Some(token)
    }
    /// If its a validated token, it always has one vote
    fn get_vote_count(&self, _token: &Self::ValidatedVoteToken) -> u64 {
        1
    }
}
