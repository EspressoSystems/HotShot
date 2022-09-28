//! The election trait, used to decide which node is the leader and determine if a vote is valid.

use commit::Commitment;

use crate::{traits::signature_key::SignatureKey, data::TimeType};

use super::{StateContents, state::ConsensusTime};

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
    fn get_leader(&self, table: &Self::StakeTable, view_number: TimeType) -> P;

    /// Validates a vote token and returns the number of seats that it has
    ///
    /// Salt: Hash of the leaf that is being proposed
    fn get_votes(
        &self,
        table: &Self::StakeTable,
        selection_threshold: Self::SelectionThreshold,
        view_number: TimeType,
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
        view_number: TimeType,
        private_key: &<P as SignatureKey>::PrivateKey,
        next_state: Commitment<Self::State>,
    ) -> Option<Self::VoteToken>;

    // checks fee table validity, adds it to the block, this gets called by the leader when proposing the block
    // fn attach_proposed_fee_table(&self, b: &mut Block, fees: Vec<(ReceiverKey,u64)>) -> Result<()>

    // checks fee table validity, adds it to the block, this gets called by the leader when proposing the block
    // fn attach_proposed_fee_table(&self, b: &mut Block, fees: Vec<(ReceiverKey,u64)>) -> Result<()>
}
