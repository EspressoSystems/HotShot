use ark_std::{rand::SeedableRng, string::ToString, vec::Vec};
use digest::crypto_common::rand_core::CryptoRngCore;
use displaydoc::Display;
use jf_primitives::errors::PrimitivesError;

/// Snapshots of the stake table
/// - the latest "Head" where all new changes are applied to
/// - `EpochStart` marks the snapshot at the beginning of the current epoch
/// - `LastEpochStart` marks the beginning of the last epoch
/// - `BlockNum(u64)` at arbitrary block height
pub enum SnapshotVersion {
    Head,
    EpochStart,
    LastEpochStart,
    BlockNum(u64),
}

/// Common interfaces required for a stake table used in HotShot System.
/// APIs that doesn't take `version: SnapshotVersion` as an input by default works on the head/latest version.
pub trait StakeTableScheme {
    /// type for stake key
    type Key: Clone;
    /// type for the staked amount
    type Amount: Clone + Copy;
    /// type for the commitment to the current stake table
    type Commitment;
    /// type for the proof associated with the lookup result (if any)
    type LookupProof;
    /// type for the iterator over (key, value) entries
    type IntoIter: Iterator<Item = (Self::Key, Self::Amount)>;

    /// Register a new key into the stake table.
    fn register(&mut self, new_key: Self::Key, amount: Self::Amount)
        -> Result<(), StakeTableError>;

    /// Batch register a list of new keys. A default implementation is provided
    /// w/o batch optimization.
    fn batch_register<I, J>(&mut self, new_keys: I, amounts: J) -> Result<(), StakeTableError>
    where
        I: IntoIterator<Item = Self::Key>,
        J: IntoIterator<Item = Self::Amount>,
    {
        let _ = new_keys
            .into_iter()
            .zip(amounts.into_iter())
            .try_for_each(|(key, amount)| Self::register(self, key, amount));
        Ok(())
    }

    /// Deregister an existing key from the stake table.
    /// Returns error if some keys are not found.
    fn deregister(&mut self, existing_key: &Self::Key) -> Result<(), StakeTableError>;

    /// Batch deregister a list of keys. A default implementation is provided
    /// w/o batch optimization.
    fn batch_deregister<'a, I>(&mut self, existing_keys: I) -> Result<(), StakeTableError>
    where
        I: IntoIterator<Item = &'a <Self as StakeTableScheme>::Key>,
        <Self as StakeTableScheme>::Key: 'a,
    {
        let _ = existing_keys
            .into_iter()
            .try_for_each(|key| Self::deregister(self, key));
        Ok(())
    }

    /// Returns the commitment to the `version` of stake table.
    fn commitment(&self, version: SnapshotVersion) -> Result<Self::Commitment, StakeTableError>;

    /// Returns the accumulated stakes of all registered keys of the `version`
    /// of stake table.
    fn total_stake(&self, version: SnapshotVersion) -> Result<Self::Amount, StakeTableError>;

    /// Returns the number of keys in the `version` of the table.
    fn len(&self, version: SnapshotVersion) -> Result<usize, StakeTableError>;

    /// Returns true if `key` is currently registered, else returns false.
    fn contains_key(&self, key: &Self::Key) -> bool;

    /// Lookup the stake under a key against a specific historical `version`,
    /// returns error if keys unregistered.
    fn lookup(
        &self,
        version: SnapshotVersion,
        key: &Self::Key,
    ) -> Result<(Self::Amount, Self::LookupProof), StakeTableError>;

    /// Returns the stakes withhelded by a public key, None if the key is not registered.
    /// If you need a lookup proof, use [`Self::lookup()`] instead (which is usually more expensive).
    fn simple_lookup(
        &self,
        version: SnapshotVersion,
        key: &Self::Key,
    ) -> Result<Self::Amount, StakeTableError>;

    /// Update the stake of the `key` with `(negative ? -1 : 1) * delta`.
    /// Return the updated stake or error.
    fn update(
        &mut self,
        key: &Self::Key,
        delta: Self::Amount,
        negative: bool,
    ) -> Result<Self::Amount, StakeTableError>;

    /// Batch update the stake balance of `keys`. Read documentation about
    /// [`Self::update()`]. By default, we call `Self::update()` on each
    /// (key, amount, negative) tuple.
    fn batch_update(
        &mut self,
        keys: &[Self::Key],
        amounts: &[Self::Amount],
        negative_flags: Vec<bool>,
    ) -> Result<Vec<Self::Amount>, StakeTableError> {
        let updated_amounts = keys
            .iter()
            .zip(amounts.iter())
            .zip(negative_flags.iter())
            .map(|((key, &amount), negative)| Self::update(self, key, amount, *negative))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(updated_amounts)
    }

    /// Randomly sample a (key, stake_amount) pair proportional to the stake distributions,
    /// given a fixed seed for `rng`, this sampling should be deterministic.
    fn sample(
        &self,
        rng: &mut (impl SeedableRng + CryptoRngCore),
    ) -> Option<(&Self::Key, &Self::Amount)>;

    /// Returns an iterator over all (key, value) entries of the `version` of the table
    fn iter(&self, version: SnapshotVersion) -> Result<Self::IntoIter, StakeTableError>;
}

#[derive(Debug, Display)]
pub enum StakeTableError {
    /// Internal error caused by Rescue
    RescueError,
    /// Key mismatched
    MismatchedKey,
    /// Key not found
    KeyNotFound,
    /// Key already exists
    ExistingKey,
    /// Malformed Merkle proof
    MalformedProof,
    /// Verification Error
    VerificationError,
    /// Insufficient fund: the number of stake cannot be negative
    InsufficientFund,
    /// The number of stake exceed U256
    StakeOverflow,
    /// The historical snapshot requested is not supported.
    SnapshotUnsupported,
}

impl ark_std::error::Error for StakeTableError {}

impl From<StakeTableError> for PrimitivesError {
    fn from(value: StakeTableError) -> Self {
        // FIXME: (alex) should we define a PrimitivesError::General()?
        Self::ParameterError(value.to_string())
    }
}
