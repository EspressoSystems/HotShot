//! The election trait, used to decide which node is the leader and determine if a vote is valid.

use super::{state::ConsensusTime, State};
use crate::{
    data::{Leaf, ViewNumber},
    traits::signature_key::SignatureKey,
};
use commit::Commitment;
use serde::{de::DeserializeOwned, Serialize};
use snafu::Snafu;

/// Error for election problems
#[derive(Snafu, Debug)]
pub enum ElectionError {
    /// stub error to be filled in
    StubError
}

/// Describes how `HotShot` chooses committees and leaders
pub trait Election<P: SignatureKey, T: ConsensusTime>: Send + Sync {
    /// Data structure describing the currently valid states
    type StakeTable: Send + Sync;
    /// The state type this election implementation is bound to
    type StateType: State;
    /// A membership proof
    type VoteToken: Serialize + DeserializeOwned;
    /// A type stated, validated membership proof
    type ValidatedVoteToken;

    /// Returns the table from the current committed state
    fn get_stake_table(&self, state: &Self::StateType) -> Self::StakeTable;

    /// Returns leader for the current view number, given the current stake table
    fn get_leader(&self, view_number: ViewNumber) -> P;

    /// Validates a vote token and returns the number of seats that it has
    ///
    /// Salt: Hash of the leaf that is being proposed
    fn get_votes(
        &self,
        view_number: ViewNumber,
        pub_key: P,
        token: Self::VoteToken,
        next_state: Commitment<Leaf<Self::StateType>>,
    ) -> Result<Self::ValidatedVoteToken, ElectionError>;

    /// Returns the number of votes the validated vote token has
    fn get_vote_count(&self, token: &Self::ValidatedVoteToken) -> u64;

    /// Attempts to generate a vote token for self
    ///
    /// Returns `None` if the number of seats would be zero
    fn make_vote_token(
        &self,
        view_number: ViewNumber,
        private_key: &<P as SignatureKey>::PrivateKey,
        next_state: Commitment<Leaf<Self::StateType>>,
    ) -> Result<Self::VoteToken, ElectionError>;

    // checks fee table validity, adds it to the block, this gets called by the leader when proposing the block
    // fn attach_proposed_fee_table(&self, b: &mut Block, fees: Vec<(ReceiverKey,u64)>) -> Result<()>

    // checks fee table validity, adds it to the block, this gets called by the leader when proposing the block
    // fn attach_proposed_fee_table(&self, b: &mut Block, fees: Vec<(ReceiverKey,u64)>) -> Result<()>
}
