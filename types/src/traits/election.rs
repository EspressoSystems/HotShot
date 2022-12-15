//! The election trait, used to decide which node is the leader and determine if a vote is valid.
#![allow(clippy::missing_docs_in_private_items)]
#![allow(missing_docs)]

use super::node_implementation::NodeType;
use super::signature_key::{EncodedPublicKey, EncodedSignature};
use crate::data::LeafType;
use crate::traits::signature_key::SignatureKey;
use commit::{Commitment, Committable};
use either::Either;
use serde::Deserialize;
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
    fn append(val: Vec<T>) -> Either<Self, U>;
}

/// todo associated types for:
/// - signature key
/// - encoded things
/// -
pub trait SignedCertificate<SIGNATURE: SignatureKey>
where
    Self: Send + Sync + Clone + Serialize + for<'a> Deserialize<'a>,
{
    type Accumulator: Accumulator<(EncodedSignature, SIGNATURE), Self>;
}

/// Describes how `HotShot` chooses committees and leaders
/// TODO (da) make a separate vote token type for DA and QC
/// @ny thinks we should make the vote token types be bound to `ConsensusType`
pub trait Election<TYPES: NodeType>: Clone + Eq + PartialEq + Send + Sync + 'static {
    /// Data structure describing the currently valid states
    /// TODO make this a trait so we can pass in places
    type StakeTable: Send + Sync;

    /// certificate for quorum on consenus
    type QuorumCertificate: SignedCertificate<TYPES::SignatureKey> + Clone + Debug + Eq + PartialEq;

    /// certificate for data availability
    type DACertificate: SignedCertificate<TYPES::SignatureKey> + Clone + Debug + Eq + PartialEq;

    type LeafType: LeafType<NodeType = TYPES>;

    /// check that the quorum certificate is valid
    fn is_valid_qc(&self, qc: Self::QuorumCertificate) -> bool;

    /// check that the data availability certificate is valid
    fn is_valid_dac(&self, qc: Self::DACertificate) -> bool;

    /// confirm that a quorum certificate signature is valid
    fn is_valid_qc_signature(
        &self,
        encoded_key: &EncodedPublicKey,
        encoded_signature: &EncodedSignature,
        hash: Commitment<Self::LeafType>,
        view_number: TYPES::Time,
        vote_token: Checked<TYPES::VoteTokenType>,
    ) -> bool;

    /// confirm that a data availability signature is valid
    fn is_valid_dac_signature(
        &self,
        encoded_key: &EncodedPublicKey,
        encoded_signature: &EncodedSignature,
        view_number: TYPES::Time,
        vote_token: Checked<TYPES::VoteTokenType>,
    ) -> bool;

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
pub trait TestableElection<TYPES: NodeType>: Election<TYPES> {
    /// Generate a vote token used for testing.
    fn generate_test_vote_token() -> TYPES::VoteTokenType;
}
