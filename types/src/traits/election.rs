//! The election trait, used to decide which node is the leader and determine if a vote is valid.
#![allow(clippy::missing_docs_in_private_items)]
#![allow(missing_docs)]

use super::node_implementation::NodeType;
use super::signature_key::{EncodedPublicKey, EncodedSignature};
use crate::{
    data::LeafType,
    traits::{
        election::Checked::{Inval, Unchecked, Valid},
        signature_key::SignatureKey,
    },
};
use commit::{Commitment, Committable};
use either::Either;
use serde::Deserialize;
use serde::{de::DeserializeOwned, Serialize};
use snafu::Snafu;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::num::NonZeroU64;
use tracing::error;

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
pub trait SignedCertificate<SIGNATURE: SignatureKey, TIME, TOKEN, LEAF>
where
    Self: Send + Sync + Clone + Serialize + for<'a> Deserialize<'a>,
    LEAF: Committable,
{
    type Accumulator: Accumulator<(EncodedSignature, SIGNATURE), Self>;

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

/// Describes how `HotShot` chooses committees and leaders
/// TODO (da) make a separate vote token type for DA and QC
/// @ny thinks we should make the vote token types be bound to `ConsensusType`
pub trait Election<TYPES: NodeType>: Clone + Eq + PartialEq + Send + Sync + 'static {
    /// Data structure describing the currently valid states
    /// TODO make this a trait so we can pass in places
    type StakeTable: Send + Sync;

    /// certificate for quorum on consenus
    type QuorumCertificate: SignedCertificate<TYPES::SignatureKey, TYPES::Time, TYPES::VoteTokenType, Self::LeafType>
        + Clone
        + Debug
        + Eq
        + PartialEq;

    /// certificate for data availability
    type DACertificate: SignedCertificate<TYPES::SignatureKey, TYPES::Time, TYPES::VoteTokenType, Self::LeafType>
        + Clone
        + Debug
        + Eq
        + PartialEq;

    type LeafType: LeafType<NodeType = TYPES, QuorumCertificate = Self::QuorumCertificate>;

    /// check that the quorum certificate is valid
    fn is_valid_qc(&self, qc: &<Self::LeafType as LeafType>::QuorumCertificate) -> bool {
        if qc.is_genesis() && qc.view_number() == TYPES::Time::genesis() {
            return true;
        }
        let hash = qc.leaf_commitment();

        let stake = qc
            .signatures()
            .iter()
            .filter(|signature| {
                self.is_valid_signature(
                    signature.0,
                    &signature.1 .0,
                    hash,
                    qc.view_number(),
                    Unchecked(signature.1 .1.clone()),
                )
            })
            .fold(0, |acc, x| (acc + u64::from(x.1 .1.vote_count())));

        if stake >= u64::from(self.threshold()) {
            return true;
        }
        false
    }

    /// check that the data availability certificate is valid
    fn is_valid_dac(&self, qc: <Self::LeafType as LeafType>::DACertificate) -> bool;

    /// Confirm that a signature, for QC or DAC, is valid.
    fn is_valid_signature(
        &self,
        encoded_key: &EncodedPublicKey,
        encoded_signature: &EncodedSignature,
        hash: Commitment<Self::LeafType>,
        view_number: TYPES::Time,
        vote_token: Checked<TYPES::VoteTokenType>,
    ) -> bool {
        let mut is_valid_vote_token = false;
        let mut is_valid_signature = false;
        if let Some(key) = <TYPES::SignatureKey as SignatureKey>::from_bytes(encoded_key) {
            is_valid_signature = key.validate(encoded_signature, hash.as_ref());
            let valid_vote_token = self.validate_vote_token(view_number, key, vote_token);
            is_valid_vote_token = match valid_vote_token {
                Err(_) => {
                    error!("Vote token was invalid");
                    false
                }
                Ok(Valid(_)) => true,
                Ok(Inval(_) | Unchecked(_)) => false,
            };
        }
        is_valid_signature && is_valid_vote_token
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
    fn threshold(&self) -> NonZeroU64;
}

/// Testable implementation of an [`Election`]. Will expose a method to generate a vote token used for testing.
pub trait TestableElection<TYPES: NodeType>: Election<TYPES> {
    /// Generate a vote token used for testing.
    fn generate_test_vote_token() -> TYPES::VoteTokenType;
}
