// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

//! Implementations of the simple vote types.

use std::{fmt::Debug, hash::Hash, marker::PhantomData};

use anyhow::Result;
use committable::{Commitment, Committable};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use vbs::version::{StaticVersionType, Version};

use crate::{
    data::Leaf,
    message::UpgradeLock,
    traits::{
        node_implementation::{NodeType, Versions},
        signature_key::SignatureKey,
    },
    vid::VidCommitment,
    vote::{HasViewNumber, Vote},
};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a yes vote.
#[serde(bound(deserialize = ""))]
pub struct QuorumData<TYPES: NodeType> {
    /// Commitment to the leaf
    pub leaf_commit: Commitment<Leaf<TYPES>>,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a DA vote.
pub struct DaData {
    /// Commitment to a block payload
    pub payload_commit: VidCommitment,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a timeout vote.
pub struct TimeoutData<Time: ConsensusTime + Display> {
    /// View the timeout is for
    pub view: Time,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a VID vote.
pub struct VidData {
    /// Commitment to the block payload the VID vote is on.
    pub payload_commit: VidCommitment,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a Pre Commit vote.
pub struct ViewSyncPreCommitData<Time: ConsensusTime + Display> {
    /// The relay this vote is intended for
    pub relay: u64,
    /// The view number we are trying to sync on
    pub round: Time,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a Commit vote.
pub struct ViewSyncCommitData<Time: ConsensusTime + Display> {
    /// The relay this vote is intended for
    pub relay: u64,
    /// The view number we are trying to sync on
    pub round: Time,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a Finalize vote.
pub struct ViewSyncFinalizeData<Time: ConsensusTime + Display> {
    /// The relay this vote is intended for
    pub relay: u64,
    /// The view number we are trying to sync on
    pub round: Time,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a Upgrade vote.
pub struct UpgradeProposalData<Time: ConsensusTime + Display> {
    /// The old version that we are upgrading from.
    pub old_version: Version,
    /// The new version that we are upgrading to.
    pub new_version: Version,
    /// The last view in which we are allowed to reach a decide on this upgrade.
    /// If it is not decided by that view, we discard it.
    pub decide_by: Time,
    /// A unique identifier for the specific protocol being voted on.
    pub new_version_hash: Vec<u8>,
    /// The last block for which the old version will be in effect.
    pub old_version_last_view: Time,
    /// The first block for which the new version will be in effect.
    pub new_version_first_view: Time,
}

/// Marker trait for data or commitments that can be voted on.
/// Only structs in this file can implement voteable.  This is enforced with the `Sealed` trait
/// Sealing this trait prevents creating new vote types outside this file.
pub trait Voteable:
    sealed::Sealed + Committable + Clone + Serialize + Debug + PartialEq + Hash + Eq
{
}

/// Sealed is used to make sure no other files can implement the Voteable trait.
/// All simple voteable types should be implemented here.  This prevents us from
/// creating/using improper types when using the vote types.
mod sealed {
    use committable::Committable;

    /// Only structs in this file can impl `Sealed`
    pub trait Sealed {}

    // TODO: Does the implement for things outside this file that are committable?
    impl<C: Committable> Sealed for C {}
}

/// A simple yes vote over some votable type.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
pub struct SimpleVote<Key: SignatureKey, Time: ConsensusTime + Display, DATA: Voteable> {
    /// The signature share associated with this vote
    pub signature: (Key, <Key as SignatureKey>::PureAssembledSignatureType),
    /// The leaf commitment being voted on.
    pub data: DATA,
    /// The view this vote was cast for
    pub view_number: Time,
}

impl<Key: SignatureKey, Time: ConsensusTime + Display, DATA: Voteable + 'static> HasViewNumber<Time>
    for SimpleVote<Key, Time, DATA>
{
    fn view_number(&self) -> Time {
        self.view_number
    }
}

impl<Key: SignatureKey, Time: ConsensusTime + Display, DATA: Voteable + 'static> Vote<Key, Time>
    for SimpleVote<Key, Time, DATA>
{
    type Commitment = DATA;

    fn signing_key(&self) -> Key {
        self.signature.0.clone()
    }

    fn signature(&self) -> <Key as SignatureKey>::PureAssembledSignatureType {
        self.signature.1.clone()
    }

    fn date(&self) -> &DATA {
        &self.data
    }

    fn date_commitment(&self) -> Commitment<DATA> {
        self.data.commit()
    }
}

impl<Key: SignatureKey, Time: ConsensusTime + Display, DATA: Voteable + 'static>
    SimpleVote<Key, Time, DATA>
{
    /// Creates and signs a simple vote
    /// # Errors
    /// If we are unable to sign the data
    pub async fn create_signed_vote<V: Versions>(
        data: DATA,
        view: Time,
        pub_key: &Key,
        private_key: &<Key as SignatureKey>::PrivateKey,
        upgrade_lock: &UpgradeLock<TYPES, V>,
    ) -> Result<Self> {
        let commit = VersionedVoteData::new(data.clone(), view, upgrade_lock)
            .await?
            .commit();

        let signature = (
            pub_key.clone(),
            TYPES::SignatureKey::sign(private_key, commit.as_ref())?,
        );

        Ok(Self {
            signature,
            data,
            view_number: view,
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// A wrapper for vote data that carries a view number and an `upgrade_lock`, allowing switching the commitment calculation dynamically depending on the version
pub struct VersionedVoteData<Time: ConsensusTime + Display, DATA: Voteable, V: Versions> {
    /// underlying vote data
    data: DATA,

    /// view number
    view: Time,

    /// version applied to the view number
    version: Version,

    /// phantom data
    _pd: PhantomData<V>,
}

impl<Time: ConsensusTime + Display, DATA: Voteable, V: Versions> VersionedVoteData<Time, DATA, V> {
    /// Create a new `VersionedVoteData` struct
    ///
    /// # Errors
    ///
    /// Returns an error if `upgrade_lock.version(view)` is unable to return a version we support
    pub async fn new(data: DATA, view: Time, upgrade_lock: &UpgradeLock<Time, V>) -> Result<Self> {
        let version = upgrade_lock.version(view).await?;

        Ok(Self {
            data,
            view,
            version,
            _pd: PhantomData,
        })
    }

    /// Create a new `VersionedVoteData` struct
    ///
    /// This function cannot error, but may use an invalid version.
    pub async fn new_infallible(
        data: DATA,
        view: Time,
        upgrade_lock: &UpgradeLock<Time, V>,
    ) -> Self {
        let version = upgrade_lock.version_infallible(view).await;

        Self {
            data,
            view,
            version,
            _pd: PhantomData,
        }
    }
}

impl<Time: ConsensusTime + Display, DATA: Voteable, V: Versions> Committable
    for VersionedVoteData<Time, DATA, V>
{
    fn commit(&self) -> Commitment<Self> {
        if self.version < V::Marketplace::VERSION {
            let bytes: [u8; 32] = self.data.commit().into();

            Commitment::<Self>::from_raw(bytes)
        } else {
            committable::RawCommitmentBuilder::new("Vote")
                .var_size_bytes(self.data.commit().as_ref())
                .u64(*self.view)
                .finalize()
        }
    }
}

impl<Time: ConsensusTime + Display> Committable for QuorumData<Time> {
    fn commit(&self) -> Commitment<Self> {
        committable::RawCommitmentBuilder::new("Quorum data")
            .var_size_bytes(self.leaf_commit.as_ref())
            .finalize()
    }
}

impl<Time: ConsensusTime + Display> Committable for TimeoutData<Time> {
    fn commit(&self) -> Commitment<Self> {
        committable::RawCommitmentBuilder::new("Timeout data")
            .u64(*self.view)
            .finalize()
    }
}

impl Committable for DaData {
    fn commit(&self) -> Commitment<Self> {
        committable::RawCommitmentBuilder::new("DA data")
            .var_size_bytes(self.payload_commit.as_ref())
            .finalize()
    }
}

impl Committable for VidData {
    fn commit(&self) -> Commitment<Self> {
        committable::RawCommitmentBuilder::new("VID data")
            .var_size_bytes(self.payload_commit.as_ref())
            .finalize()
    }
}

impl<Time: ConsensusTime + Display> Committable for UpgradeProposalData<Time> {
    fn commit(&self) -> Commitment<Self> {
        let builder = committable::RawCommitmentBuilder::new("Upgrade data");
        builder
            .u64(*self.decide_by)
            .u64(*self.new_version_first_view)
            .u64(*self.old_version_last_view)
            .var_size_bytes(self.new_version_hash.as_slice())
            .u16(self.new_version.minor)
            .u16(self.new_version.major)
            .u16(self.old_version.minor)
            .u16(self.old_version.major)
            .finalize()
    }
}

/// This implements commit for all the types which contain a view and relay public key.
fn view_and_relay_commit<Time: ConsensusTime + Display, T: Committable>(
    view: Time,
    relay: u64,
    tag: &str,
) -> Commitment<T> {
    let builder = committable::RawCommitmentBuilder::new(tag);
    builder.u64(*view).u64(relay).finalize()
}

impl<Time: ConsensusTime + Display> Committable for ViewSyncPreCommitData<Time> {
    fn commit(&self) -> Commitment<Self> {
        view_and_relay_commit::<Time, Self>(self.round, self.relay, "View Sync Precommit")
    }
}

impl<Time: ConsensusTime + Display> Committable for ViewSyncFinalizeData<Time> {
    fn commit(&self) -> Commitment<Self> {
        view_and_relay_commit::<Time, Self>(self.round, self.relay, "View Sync Finalize")
    }
}
impl<Time: ConsensusTime + Display> Committable for ViewSyncCommitData<Time> {
    fn commit(&self) -> Commitment<Self> {
        view_and_relay_commit::<Time, Self>(self.round, self.relay, "View Sync Commit")
    }
}

// impl votable for all the data types in this file sealed marker should ensure nothing is accidently
// implemented for structs that aren't "voteable"
impl<V: sealed::Sealed + Committable + Clone + Serialize + Debug + PartialEq + Hash + Eq> Voteable
    for V
{
}

// Type aliases for simple use of all the main votes.  We should never see `SimpleVote` outside this file
/// Quorum vote Alias
pub type QuorumVote<Key: SignatureKey, Time: ConsensusTime + Display> =
    SimpleVote<Key, Time, QuorumData<Time>>;
/// DA vote type alias
pub type DaVote<Key: SignatureKey, Time: ConsensusTime + Display> = SimpleVote<Key, Time, DaData>;
/// Timeout Vote type alias
pub type TimeoutVote<Key: SignatureKey, Time: ConsensusTime + Display> =
    SimpleVote<Key, Time, TimeoutData<Time>>;
/// View Sync Commit Vote type alias
pub type ViewSyncCommitVote<Key: SignatureKey, Time: ConsensusTime + Display> =
    SimpleVote<Key, Time, ViewSyncCommitData<Time>>;
/// View Sync Pre Commit Vote type alias
pub type ViewSyncPreCommitVote<Key: SignatureKey, Time: ConsensusTime + Display> =
    SimpleVote<Key, Time, ViewSyncPreCommitData<Time>>;
/// View Sync Finalize Vote type alias
pub type ViewSyncFinalizeVote<Key: SignatureKey, Time: ConsensusTime + Display> =
    SimpleVote<Key, Time, ViewSyncFinalizeData<Time>>;
/// Upgrade proposal vote
pub type UpgradeVote<Key: SignatureKey, Time: ConsensusTime + Display> =
    SimpleVote<Key, Time, UpgradeProposalData<Time>>;
