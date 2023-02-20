//! The election trait, used to decide which node is the leader and determine if a vote is valid.
#![allow(clippy::missing_docs_in_private_items)]
#![allow(missing_docs)]

use super::node_implementation::NodeType;
use super::signature_key::{EncodedPublicKey, EncodedSignature};
use crate::{data::LeafType, traits::signature_key::SignatureKey};
use bincode::Options;
use commit::{Commitment, Committable};
use either::Either;
use hotshot_utils::bincode::bincode_opts;
use serde::Deserialize;
use serde::{de::DeserializeOwned, Serialize};
use snafu::Snafu;
use std::collections::{BTreeMap, BTreeSet};
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

/// Data to vote on for different types of votes.
#[derive(Serialize)]
pub enum VoteData<TYPES: NodeType, LEAF: LeafType> {
    DA(Commitment<TYPES::BlockType>),
    Yes(Commitment<LEAF>),
    No(Commitment<LEAF>),
    Timeout(TYPES::Time),
}

impl<TYPES: NodeType, LEAF: LeafType> VoteData<TYPES, LEAF> {
    /// Convert vote data into bytes.
    ///
    /// # Panics
    /// Panics if the serialization fails.
    pub fn as_bytes(&self) -> Vec<u8> {
        bincode_opts().serialize(&self).unwrap()
    }
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
    // type StakeTable;
    // type KeyPair: SignatureKey;
    // type ConsensusTime: ConsensusTime;

    /// the count, which validation will confirm
    fn vote_count(&self) -> NonZeroU64;
}

/// election config
pub trait ElectionConfig:
    Default + Clone + Serialize + DeserializeOwned + Sync + Send + core::fmt::Debug
{
}

/// Describes any aggreation of signatures or votes.
pub trait Accumulator<T, U>: Sized {
    /// accumates the val to the current state.  If
    /// A threshold is reached we Return U (which could a certificate or similar)
    /// else we return self and can continue accumulation items.
    fn append(self, val: T) -> Either<Self, U>;
}

pub trait SignedCertificate<SIGNATURE: SignatureKey, TIME, TOKEN, LEAF>
where
    Self: Send + Sync + Clone + Serialize + for<'a> Deserialize<'a>,
    LEAF: Committable,
{
    /// Build a QC from the threshold signature and commitment
    fn from_signatures_and_commitment(
        view_number: TIME,
        signatures: BTreeMap<EncodedPublicKey, (EncodedSignature, TOKEN)>,
        commit: Commitment<LEAF>,
    ) -> Self;

    /// Get the view number.
    fn view_number(&self) -> TIME;

    /// Get signatures.
    fn signatures(&self) -> BTreeMap<EncodedPublicKey, (EncodedSignature, TOKEN)>;

    // TODO (da) the following functions should be refactored into a QC-specific trait.

    // Get the leaf commitment.
    fn leaf_commitment(&self) -> Commitment<LEAF>;

    // Set the leaf commitment.
    fn set_leaf_commitment(&mut self, commitment: Commitment<LEAF>);

    /// Get whether the certificate is for the genesis block.
    fn is_genesis(&self) -> bool;

    /// To be used only for generating the genesis quorum certificate; will fail if used anywhere else
    fn genesis() -> Self;
}

pub trait Membership<TYPES: NodeType>: Clone + Eq + PartialEq + Send + Sync + 'static {
    type StakeTable: Send + Sync;

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

    fn get_leader(&self, view_number: TYPES::Time) -> TYPES::SignatureKey;

    fn get_committee(&self, view_number: TYPES::Time) -> BTreeSet<TYPES::SignatureKey>;

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

    /// Returns the threshold for a specific `Membership` implementation
    fn threshold(&self) -> NonZeroU64;
}

/// Testable implementation of an [`Election`]. Will expose a method to generate a vote token used for testing.
pub trait TestableElection<TYPES: NodeType>: Membership<TYPES> {
    /// Generate a vote token used for testing.
    fn generate_test_vote_token() -> TYPES::VoteTokenType;
}
