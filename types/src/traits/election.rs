//! The election trait, used to decide which node is the leader and determine if a vote is valid.

use super::node_implementation::NodeTypes;
use crate::{data::Leaf, traits::signature_key::SignatureKey};
use commit::{Commitment, Committable};
use serde::{de::DeserializeOwned, Serialize};
use snafu::Snafu;
use std::fmt::Debug;
use std::hash::Hash;

/// Error for election problems
#[derive(Snafu, Debug)]
pub enum ElectionError {
    /// stub error to be filled in
    StubError,
    /// Math error doing something
    /// NOTE: it would be better to make Election polymorphic over
    /// the election error and then have specific math errors
    MathError,
}

/// For items that will always have the same validity outcome on a successful check,
/// allows for the case of "not yet possible to check" where the check might be
/// attempted again at a later point in time, but saves on repeated checking when
/// the outcome is already knowable.
///
/// This would be a useful general utility.
pub enum Checked<T> {
    /// This item has been checked, and is valid
    Valid(T),
    /// This item has been checked, and is not valid
    Inval(T),
    /// This item has not been checked
    Unchecked(T),
}

/// Proof of this entity's right to vote, and of the weight of those votes
pub trait VoteToken:
    Clone
    + Debug
    + Send
    + Sync
    + serde::Serialize
    + for<'de> serde::Deserialize<'de>
    + PartialEq
    + Hash
    + Committable
{
    /// the count, which validation will confirm
    fn vote_count(&self) -> u64;
}

/// election config
pub trait ElectionConfig:
    Default + Clone + Serialize + DeserializeOwned + Sync + Send + core::fmt::Debug
{
}

/// Describes how `HotShot` chooses committees and leaders
pub trait Election<TYPES: NodeTypes>: Send + Sync + 'static {
    /// Data structure describing the currently valid states
    type StakeTable: Send + Sync;

    /// configuration for election
    type ElectionConfigType: ElectionConfig;

    /// generate a default election configuration
    fn default_election_config(num_nodes: u64) -> Self::ElectionConfigType;

    /// create an election
    /// TODO may want to move this to a testableelection trait
    fn create_election(keys: Vec<TYPES::SignatureKey>, config: Self::ElectionConfigType) -> Self;

    /// Returns the table from the current committed state
    fn get_stake_table(&self, time: TYPES::Time, state: &TYPES::StateType) -> Self::StakeTable;

    /// Returns leader for the current view number, given the current stake table
    fn get_leader(&self, time: TYPES::Time) -> TYPES::SignatureKey;

    /// Attempts to generate a vote token for self
    ///
    /// Returns `None` if the number of seats would be zero
    /// # Errors
    /// TODO tbd
    fn make_vote_token(
        &self,
        time: TYPES::Time,
        pub_key: &<TYPES::SignatureKey as SignatureKey>::PrivateKey,
        // TODO (ct) this should be replaced with something else...
        next_state: Commitment<Leaf<TYPES>>,
    ) -> Result<Option<TYPES::VoteTokenType>, ElectionError>;

    /// Checks the claims of a received vote token
    ///
    /// # Errors
    /// TODO tbd
    fn validate_vote_token(
        &self,
        time: TYPES::Time,
        pub_key: TYPES::SignatureKey,
        token: Checked<TYPES::VoteTokenType>,
        next_state: commit::Commitment<Leaf<TYPES>>,
    ) -> Result<Checked<TYPES::VoteTokenType>, ElectionError>;
}
