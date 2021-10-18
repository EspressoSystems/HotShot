use std::marker::PhantomData;

use crate::{BlockHash, PrivKey, PubKey};

/// Describes how `PhaseLock` chooses committees and leaders
pub trait Election<const N: usize> {
    /// Data structure describing the currently valid states
    type StakeTable;
    /// The threshold for membership selection.
    type SelectionThreshold;
    /// The state type this election implementation is bound to
    type State;
    /// A membership proof
    type VoteToken;
    /// A type stated, validated membership proof
    type ValidatedVoteToken;

    /// Returns the table from the current committed state
    fn get_stake_table(&self, state: &Self::State) -> Self::StakeTable;
    /// Returns leader for the current view number, given the current stake table
    fn get_leader(&self, table: &Self::StakeTable, view_number: u64) -> PubKey;
    /// Validates a vote token and returns the number of seats that it has
    ///
    /// Salt: Hash of the leaf that is being proposed
    fn get_votes(
        &self,
        table: &Self::StakeTable,
        selection_threshold: Self::SelectionThreshold,
        view_number: u64,
        pub_key: PubKey,
        token: Self::VoteToken,
        next_state: BlockHash<N>,
    ) -> Option<Self::ValidatedVoteToken>;
    /// Returns the number of votes the validated vote token has
    fn get_vote_count(&self, token: &Self::ValidatedVoteToken) -> u64;
    /// Attempts to generate a vote token for self
    ///
    /// Returns `None` if the number of seats would be zero
    fn make_vote_token(
        &self,
        table: &Self::StakeTable,
        selection_threshold: Self::SelectionThreshold,
        view_number: u64,
        private_key: &PrivKey,
        next_state: BlockHash<N>,
    ) -> Option<Self::VoteToken>;
}

/// Dummy implementation of [`Election`]
pub struct StaticCommittee<S, const N: usize> {
    /// The nodes participating
    nodes: Vec<PubKey>,
    /// State phantom
    _state_phantom: PhantomData<S>,
}

impl<S, const N: usize> StaticCommittee<S, N> {
    /// Creates a new dummy elector
    pub fn new(nodes: impl IntoIterator<Item = impl AsRef<PubKey>>) -> Self {
        let nodes = nodes.into_iter().map(|x| x.as_ref().clone()).collect();
        Self {
            nodes,
            _state_phantom: PhantomData,
        }
    }
}

impl<S, const N: usize> Election<N> for StaticCommittee<S, N> {
    /// Just use the vector of public keys for the stake table
    type StakeTable = Vec<PubKey>;
    /// Arbitrary state type, we don't use it
    type State = S;
    /// Arbitrary state type, we don't use it
    type SelectionThreshold = S;
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
        next_state: BlockHash<N>,
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
        next_state: BlockHash<N>,
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
