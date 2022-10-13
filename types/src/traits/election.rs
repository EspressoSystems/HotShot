//! The election trait, used to decide which node is the leader and determine if a vote is valid.

use std::{collections::BTreeMap, num::NonZeroUsize};

use super::{
    signature_key::{EncodedPublicKey, EncodedSignature},
    state::ConsensusTime,
    State,
};
use crate::{
    data::{Leaf, ViewNumber},
    traits::signature_key::SignatureKey,
};
use commit::Commitment;
use hotshot_utils::hack::nll_todo;
use serde::{de::DeserializeOwned, Serialize};
use snafu::Snafu;

/// Error for election problems
#[derive(Snafu, Debug)]
pub enum ElectionError {
    /// stub error to be filled in
    StubError,
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
pub trait VoteToken {
    /// the count, which validation will confirm
    fn vote_count(&self) -> u64;
}

/// Describes how `HotShot` chooses committees and leaders
pub trait Election<P: SignatureKey, T: ConsensusTime>: Send + Sync {
    /// Data structure describing the currently valid states
    type StakeTable: Send + Sync;
    /// The state type this election implementation is bound to
    type StateType: State;
    /// A membership proof
    type VoteTokenType: VoteToken + Serialize + DeserializeOwned + Send + Sync;

    /// Returns the table from the current committed state
    fn get_stake_table(&self, view_number: ViewNumber, state: &Self::StateType)
        -> Self::StakeTable;

    /// Returns leader for the current view number, given the current stake table
    fn get_leader(&self, view_number: ViewNumber) -> P;

    /// Returns true if signatures have enough votes to pass the committee threshold
    fn check_threshold(
        &self,
        _signatures: &BTreeMap<EncodedPublicKey, EncodedSignature>,
        _threshold: NonZeroUsize,
    ) -> bool {
        nll_todo()
    }

    /// Attempts to generate a vote token for self
    ///
    /// Returns `None` if the number of seats would be zero
    /// # Errors
    /// TODO tbd
    fn make_vote_token(
        &self,
        view_number: ViewNumber,
        private_key: &<P as SignatureKey>::PrivateKey,
        // TODO (ct) this should be replaced with something else...
        next_state: Commitment<Leaf<Self::StateType>>,
    ) -> Result<Option<Self::VoteTokenType>, ElectionError>;

    /// Checks the claims of a received vote token
    ///
    /// # Errors
    /// TODO tbd
    fn validate_vote_token(
        &self,
        view_number: ViewNumber,
        pub_key: P,
        token: Checked<Self::VoteTokenType>,
        next_state: commit::Commitment<Leaf<Self::StateType>>,
    ) -> Result<Checked<Self::VoteTokenType>, ElectionError>;
}
