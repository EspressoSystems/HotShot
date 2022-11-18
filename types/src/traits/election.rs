//! The election trait, used to decide which node is the leader and determine if a vote is valid.

use super::node_implementation::NodeTypes;
use crate::traits::signature_key::SignatureKey;
use commit::Committable;
use serde::{de::DeserializeOwned, Serialize};
use snafu::Snafu;
use std::fmt::Debug;
use std::hash::Hash;
use std::num::NonZeroU64;

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
    fn vote_count(&self) -> NonZeroU64;
}

/// election config
pub trait ElectionConfig:
    Default + Clone + Serialize + DeserializeOwned + Sync + Send + core::fmt::Debug
{
}

pub enum Either<T, U> {
    Left(T),
    Right(U)
}

pub trait Accumulator<T> {
    fn add_signatures(signature: Vec<_> ) -> Either<Self, T> {
    }
}

pub trait SignedCertificate
    where Self : Send + Sync + Clone {
        type Accumulator: Accumulator<Self>;

}

/// Describes how `HotShot` chooses committees and leaders
pub trait Election<TYPES: NodeTypes>: Send + Sync + 'static {
    /// Data structure describing the currently valid states
    type StakeTable: Send + Sync;

    /// certificate for quorum on consenus
    type QuorumCertificate: Send + Sync + SignedCertificate;

    /// certificate for data availability
    type DACertificate: Send + Sync + SignedCertificate;

    fn validate_qc(&self, qc: Self::QuorumCertificate) -> bool {
    }

    fn is_valid_signature(&self,
                          ) {

    }

    /// generate a default election configuration
    fn default_election_config(num_nodes: u64) -> TYPES::ElectionConfigType;

    /// create an election
    /// TODO may want to move this to a testableelection trait
    fn create_election(keys: Vec<TYPES::SignatureKey>, config: TYPES::ElectionConfigType) -> Self;

    /// Returns the table from the current committed state
    fn get_stake_table(
        &self,
        view_number: TYPES::Time,
        state: &TYPES::StateType,
    ) -> Self::StakeTable;

    /// Returns leader for the current view number, given the current stake table
    fn get_leader(&self, view_number: TYPES::Time) -> TYPES::SignatureKey;

    /// Attempts to generate a vote token for self
    ///
    /// Returns `None` if the number of seats would be zero
    /// # Errors
    /// TODO tbd
    fn make_vote_token(
        &self,
        view_number: TYPES::Time,
        priv_key: &<TYPES::SignatureKey as SignatureKey>::PrivateKey,
    ) -> Result<Option<TYPES::VoteTokenType>, ElectionError>;

    /// Checks the claims of a received vote token
    ///
    /// # Errors
    /// TODO tbd
    fn validate_vote_token(
        &self,
        view_number: TYPES::Time,
        pub_key: TYPES::SignatureKey,
        token: Checked<TYPES::VoteTokenType>,
    ) -> Result<Checked<TYPES::VoteTokenType>, ElectionError>;

    /// Returns the threshold for a specific `Election` implementation
    fn get_threshold(&self) -> NonZeroU64;
}

/// Testable implementation of an [`Election`]. Will expose a method to generate a vote token used for testing.
pub trait TestableElection<TYPES: NodeTypes>: Election<TYPES> {
    /// Generate a vote token used for testing.
    fn generate_test_vote_token() -> TYPES::VoteTokenType;
}
