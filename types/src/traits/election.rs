//! The election trait, used to decide which node is the leader and determine if a vote is valid.

///
pub mod jf;
pub mod stub;

use super::{state::ConsensusTime, StateContents};
use crate::{data::ViewNumber, traits::signature_key::SignatureKey};
use commit::Commitment;
use std::num::NonZeroUsize;

/// Describes how `HotShot` chooses committees and leaders
pub trait Election<P: SignatureKey, T: ConsensusTime>: Send + Sync {
    /// Data structure describing the currently valid states
    type StakeTable: Send + Sync;
    /// The threshold for membership selection.
    type SelectionThreshold;
    /// The state type this election implementation is bound to
    type State: StateContents;
    /// A membership proof
    type VoteToken;
    /// A type stated, validated membership proof
    type ValidatedVoteToken;

    /// Returns the table from the current committed state
    fn get_stake_table(&self, state: &Self::State) -> Self::StakeTable;

    /// Returns leader for the current view number, given the current stake table
    fn get_leader(&self, table: &Self::StakeTable, view_number: ViewNumber) -> P;

    /// Validates a vote token and returns the number of seats that it has
    ///
    /// Salt: Hash of the leaf that is being proposed
    fn get_votes(
        &self,
        table: &Self::StakeTable,
        selection_threshold: Self::SelectionThreshold,
        view_number: ViewNumber,
        pub_key: P,
        token: Self::VoteToken,
        next_state: Commitment<Self::State>,
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
        private_key: &<P as SignatureKey>::PrivateKey,
        next_state: Commitment<Self::State>,
    ) -> Option<Self::VoteToken>;

    /// Calcuates the required `SelectionThreshold` for the given parameters
    fn calculate_selection_threshold(
        &self,
        expected_size: NonZeroUsize,
        total_participants: NonZeroUsize,
    ) -> Self::SelectionThreshold;

    // checks fee table validity, adds it to the block, this gets called by the leader when proposing the block
    // fn attach_proposed_fee_table(&self, b: &mut Block, fees: Vec<(ReceiverKey,u64)>) -> Result<()>

    // checks fee table validity, adds it to the block, this gets called by the leader when proposing the block
    // fn attach_proposed_fee_table(&self, b: &mut Block, fees: Vec<(ReceiverKey,u64)>) -> Result<()>
}
