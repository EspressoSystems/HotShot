//! The election trait, used to decide which node is the leader and determine if a vote is valid.

use crate::{
    data::{Stage, StateHash, ViewNumber},
    traits::signature_key::SignatureKey,
    PubKey,
};

/// Describes how `PhaseLock` chooses committees and leaders
pub trait Election<S: SignatureKey, const N: usize>: Send + Sync {
    /// Data structure describing the currently valid states
    type StakeTable: Send + Sync;
    /// The threshold for membership selection.
    type SelectionThreshold;
    /// The state type this election implementation is bound to
    type State: Send + Sync + Default;
    /// A membership proof
    type VoteToken;
    /// A type stated, validated membership proof
    type ValidatedVoteToken;

    /// Returns the table from the current committed state
    fn get_stake_table(&self, state: &Self::State) -> Self::StakeTable;
    /// Returns leader for the current view number, given the current stake table
    fn get_leader(
        &self,
        table: &Self::StakeTable,
        view_number: ViewNumber,
        stage: Stage,
    ) -> PubKey<S>;
    /// Validates a vote token and returns the number of seats that it has
    ///
    /// Salt: Hash of the leaf that is being proposed
    fn get_votes(
        &self,
        table: &Self::StakeTable,
        selection_threshold: Self::SelectionThreshold,
        view_number: ViewNumber,
        pub_key: PubKey<S>,
        token: Self::VoteToken,
        next_state: StateHash<N>,
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
        view_number: ViewNumber,
        private_key: &S::PrivateKey,
        next_state: StateHash<N>,
    ) -> Option<Self::VoteToken>;

    // checks fee table validity, adds it to the block, this gets called by the leader when proposing the block
    // fn attach_proposed_fee_table(&self, b: &mut Block, fees: Vec<(ReceiverKey,u64)>) -> Result<()>

    // checks fee table validity, adds it to the block, this gets called by the leader when proposing the block
    // fn attach_proposed_fee_table(&self, b: &mut Block, fees: Vec<(ReceiverKey,u64)>) -> Result<()>
}
