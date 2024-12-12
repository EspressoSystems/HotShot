// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

//! Implementations of the simple vote types.

use std::{fmt::Debug, hash::Hash, marker::PhantomData};

use committable::{Commitment, Committable};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use utils::anytrace::*;
use vbs::version::Version;

use crate::{
    data::{Leaf, Leaf2},
    message::UpgradeLock,
    traits::{
        node_implementation::{ConsensusTime, NodeType, Versions},
        signature_key::SignatureKey,
    },
    vid::VidCommitment,
    vote::{HasViewNumber, Vote},
};

/// Marker that data should use the quorum cert type
pub(crate) trait QuorumMarker {}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a yes vote.
#[serde(bound(deserialize = ""))]
pub struct QuorumData<TYPES: NodeType> {
    /// Commitment to the leaf
    pub leaf_commit: Commitment<Leaf<TYPES>>,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a yes vote.
#[serde(bound(deserialize = ""))]
pub struct QuorumData2<TYPES: NodeType> {
    /// Commitment to the leaf
    pub leaf_commit: Commitment<Leaf2<TYPES>>,
    /// Epoch number
    pub epoch: TYPES::Epoch,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a DA vote.
pub struct DaData {
    /// Commitment to a block payload
    pub payload_commit: VidCommitment,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a DA vote.
pub struct DaData2<TYPES: NodeType> {
    /// Commitment to a block payload
    pub payload_commit: VidCommitment,
    /// Epoch number
    pub epoch: TYPES::Epoch,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a timeout vote.
pub struct TimeoutData<TYPES: NodeType> {
    /// View the timeout is for
    pub view: TYPES::View,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a timeout vote.
pub struct TimeoutData2<TYPES: NodeType> {
    /// View the timeout is for
    pub view: TYPES::View,
    /// Epoch number
    pub epoch: TYPES::Epoch,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a Pre Commit vote.
pub struct ViewSyncPreCommitData<TYPES: NodeType> {
    /// The relay this vote is intended for
    pub relay: u64,
    /// The view number we are trying to sync on
    pub round: TYPES::View,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a Pre Commit vote.
pub struct ViewSyncPreCommitData2<TYPES: NodeType> {
    /// The relay this vote is intended for
    pub relay: u64,
    /// The view number we are trying to sync on
    pub round: TYPES::View,
    /// Epoch number
    pub epoch: TYPES::Epoch,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a Commit vote.
pub struct ViewSyncCommitData<TYPES: NodeType> {
    /// The relay this vote is intended for
    pub relay: u64,
    /// The view number we are trying to sync on
    pub round: TYPES::View,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a Commit vote.
pub struct ViewSyncCommitData2<TYPES: NodeType> {
    /// The relay this vote is intended for
    pub relay: u64,
    /// The view number we are trying to sync on
    pub round: TYPES::View,
    /// Epoch number
    pub epoch: TYPES::Epoch,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a Finalize vote.
pub struct ViewSyncFinalizeData<TYPES: NodeType> {
    /// The relay this vote is intended for
    pub relay: u64,
    /// The view number we are trying to sync on
    pub round: TYPES::View,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a Finalize vote.
pub struct ViewSyncFinalizeData2<TYPES: NodeType> {
    /// The relay this vote is intended for
    pub relay: u64,
    /// The view number we are trying to sync on
    pub round: TYPES::View,
    /// Epoch number
    pub epoch: TYPES::Epoch,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
/// Data used for a Upgrade vote.
pub struct UpgradeProposalData<TYPES: NodeType + DeserializeOwned> {
    /// The old version that we are upgrading from.
    pub old_version: Version,
    /// The new version that we are upgrading to.
    pub new_version: Version,
    /// The last view in which we are allowed to reach a decide on this upgrade.
    /// If it is not decided by that view, we discard it.
    pub decide_by: TYPES::View,
    /// A unique identifier for the specific protocol being voted on.
    pub new_version_hash: Vec<u8>,
    /// The last block for which the old version will be in effect.
    pub old_version_last_view: TYPES::View,
    /// The first block for which the new version will be in effect.
    pub new_version_first_view: TYPES::View,
}

/// Data used for an upgrade once epochs are implemented
pub struct UpgradeData2<TYPES: NodeType> {
    /// The old version that we are upgrading from
    pub old_version: Version,
    /// The new version that we are upgrading to
    pub new_version: Version,
    /// A unique identifier for the specific protocol being voted on
    pub hash: Vec<u8>,
    /// The first epoch in which the upgrade will be in effect
    pub epoch: TYPES::Epoch,
}

/// Marker trait for data or commitments that can be voted on.
/// Only structs in this file can implement voteable.  This is enforced with the `Sealed` trait
/// Sealing this trait prevents creating new vote types outside this file.
pub trait Voteable<TYPES: NodeType>:
    sealed::Sealed + Committable + Clone + Serialize + Debug + PartialEq + Hash + Eq
{
}

/// Marker trait for data or commitments that can be voted on.
/// Only structs in this file can implement voteable.  This is enforced with the `Sealed` trait
/// Sealing this trait prevents creating new vote types outside this file.
pub trait Voteable2<TYPES: NodeType>:
    sealed::Sealed + HasEpoch<TYPES> + Committable + Clone + Serialize + Debug + PartialEq + Hash + Eq
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

impl<T: NodeType> QuorumMarker for QuorumData<T> {}
impl<T: NodeType> QuorumMarker for QuorumData2<T> {}
impl<T: NodeType> QuorumMarker for TimeoutData<T> {}
impl<T: NodeType> QuorumMarker for TimeoutData2<T> {}
impl<T: NodeType> QuorumMarker for ViewSyncPreCommitData<T> {}
impl<T: NodeType> QuorumMarker for ViewSyncCommitData<T> {}
impl<T: NodeType> QuorumMarker for ViewSyncFinalizeData<T> {}
impl<T: NodeType> QuorumMarker for ViewSyncPreCommitData2<T> {}
impl<T: NodeType> QuorumMarker for ViewSyncCommitData2<T> {}
impl<T: NodeType> QuorumMarker for ViewSyncFinalizeData2<T> {}
impl<T: NodeType + DeserializeOwned> QuorumMarker for UpgradeProposalData<T> {}

/// A simple yes vote over some votable type.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
pub struct SimpleVote<TYPES: NodeType, DATA: Voteable<TYPES>> {
    /// The signature share associated with this vote
    pub signature: (
        TYPES::SignatureKey,
        <TYPES::SignatureKey as SignatureKey>::PureAssembledSignatureType,
    ),
    /// The leaf commitment being voted on.
    pub data: DATA,
    /// The view this vote was cast for
    pub view_number: TYPES::View,
}

impl<TYPES: NodeType, DATA: Voteable<TYPES> + 'static> HasViewNumber<TYPES>
    for SimpleVote<TYPES, DATA>
{
    fn view_number(&self) -> <TYPES as NodeType>::View {
        self.view_number
    }
}

impl<TYPES: NodeType, DATA: Voteable<TYPES> + 'static> Vote<TYPES> for SimpleVote<TYPES, DATA> {
    type Commitment = DATA;

    fn signing_key(&self) -> <TYPES as NodeType>::SignatureKey {
        self.signature.0.clone()
    }

    fn signature(&self) -> <TYPES::SignatureKey as SignatureKey>::PureAssembledSignatureType {
        self.signature.1.clone()
    }

    fn date(&self) -> &DATA {
        &self.data
    }

    fn data_commitment(&self) -> Commitment<DATA> {
        self.data.commit()
    }
}

impl<TYPES: NodeType, DATA: Voteable<TYPES> + 'static> SimpleVote<TYPES, DATA> {
    /// Creates and signs a simple vote
    /// # Errors
    /// If we are unable to sign the data
    pub async fn create_signed_vote<V: Versions>(
        data: DATA,
        view: TYPES::View,
        pub_key: &TYPES::SignatureKey,
        private_key: &<TYPES::SignatureKey as SignatureKey>::PrivateKey,
        upgrade_lock: &UpgradeLock<TYPES, V>,
    ) -> Result<Self> {
        let commit = VersionedVoteData::new(data.clone(), view, upgrade_lock)
            .await?
            .commit();

        let signature = (
            pub_key.clone(),
            TYPES::SignatureKey::sign(private_key, commit.as_ref())
                .wrap()
                .context(error!("Failed to sign vote"))?,
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
pub struct VersionedVoteData<TYPES: NodeType, DATA: Voteable<TYPES>, V: Versions> {
    /// underlying vote data
    data: DATA,

    /// view number
    view: TYPES::View,

    /// version applied to the view number
    version: Version,

    /// phantom data
    _pd: PhantomData<V>,
}

impl<TYPES: NodeType, DATA: Voteable<TYPES>, V: Versions> VersionedVoteData<TYPES, DATA, V> {
    /// Create a new `VersionedVoteData` struct
    ///
    /// # Errors
    ///
    /// Returns an error if `upgrade_lock.version(view)` is unable to return a version we support
    pub async fn new(
        data: DATA,
        view: TYPES::View,
        upgrade_lock: &UpgradeLock<TYPES, V>,
    ) -> Result<Self> {
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
        view: TYPES::View,
        upgrade_lock: &UpgradeLock<TYPES, V>,
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

impl<TYPES: NodeType, DATA: Voteable<TYPES>, V: Versions> Committable
    for VersionedVoteData<TYPES, DATA, V>
{
    fn commit(&self) -> Commitment<Self> {
        committable::RawCommitmentBuilder::new("Vote")
            .var_size_bytes(self.data.commit().as_ref())
            .u64(*self.view)
            .finalize()
    }
}

impl<TYPES: NodeType> Committable for QuorumData<TYPES> {
    fn commit(&self) -> Commitment<Self> {
        committable::RawCommitmentBuilder::new("Quorum data")
            .var_size_bytes(self.leaf_commit.as_ref())
            .finalize()
    }
}

impl<TYPES: NodeType> Committable for QuorumData2<TYPES> {
    fn commit(&self) -> Commitment<Self> {
        let QuorumData2 {
            leaf_commit,
            epoch: _,
        } = self;

        committable::RawCommitmentBuilder::new("Quorum data")
            .var_size_bytes(leaf_commit.as_ref())
            .finalize()
    }
}

impl<TYPES: NodeType> Committable for TimeoutData<TYPES> {
    fn commit(&self) -> Commitment<Self> {
        committable::RawCommitmentBuilder::new("Timeout data")
            .u64(*self.view)
            .finalize()
    }
}

impl<TYPES: NodeType> Committable for TimeoutData2<TYPES> {
    fn commit(&self) -> Commitment<Self> {
        let TimeoutData2 { view, epoch: _ } = self;

        committable::RawCommitmentBuilder::new("Timeout data")
            .u64(**view)
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

impl<TYPES: NodeType> Committable for DaData2<TYPES> {
    fn commit(&self) -> Commitment<Self> {
        let DaData2 {
            payload_commit,
            epoch: _,
        } = self;

        committable::RawCommitmentBuilder::new("DA data")
            .var_size_bytes(payload_commit.as_ref())
            .finalize()
    }
}

impl<TYPES: NodeType> Committable for UpgradeProposalData<TYPES> {
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

impl<TYPES: NodeType> Committable for UpgradeData2<TYPES> {
    fn commit(&self) -> Commitment<Self> {
        let UpgradeData2 {
            old_version,
            new_version,
            hash,
            epoch,
        } = self;

        committable::RawCommitmentBuilder::new("Upgrade data")
            .u16(old_version.minor)
            .u16(old_version.major)
            .u16(new_version.minor)
            .u16(new_version.major)
            .var_size_bytes(hash.as_slice())
            .u64(**epoch)
            .finalize()
    }
}

/// This implements commit for all the types which contain a view and relay public key.
fn view_and_relay_commit<TYPES: NodeType, T: Committable>(
    view: TYPES::View,
    relay: u64,
    tag: &str,
) -> Commitment<T> {
    let builder = committable::RawCommitmentBuilder::new(tag);
    builder.u64(*view).u64(relay).finalize()
}

impl<TYPES: NodeType> Committable for ViewSyncPreCommitData<TYPES> {
    fn commit(&self) -> Commitment<Self> {
        view_and_relay_commit::<TYPES, Self>(self.round, self.relay, "View Sync Precommit")
    }
}

impl<TYPES: NodeType> Committable for ViewSyncPreCommitData2<TYPES> {
    fn commit(&self) -> Commitment<Self> {
        let ViewSyncPreCommitData2 {
            relay,
            round,
            epoch: _,
        } = self;

        view_and_relay_commit::<TYPES, Self>(*round, *relay, "View Sync Precommit")
    }
}

impl<TYPES: NodeType> Committable for ViewSyncFinalizeData<TYPES> {
    fn commit(&self) -> Commitment<Self> {
        view_and_relay_commit::<TYPES, Self>(self.round, self.relay, "View Sync Finalize")
    }
}

impl<TYPES: NodeType> Committable for ViewSyncFinalizeData2<TYPES> {
    fn commit(&self) -> Commitment<Self> {
        let ViewSyncFinalizeData2 {
            relay,
            round,
            epoch: _,
        } = self;

        view_and_relay_commit::<TYPES, Self>(*round, *relay, "View Sync Finalize")
    }
}

impl<TYPES: NodeType> Committable for ViewSyncCommitData<TYPES> {
    fn commit(&self) -> Commitment<Self> {
        view_and_relay_commit::<TYPES, Self>(self.round, self.relay, "View Sync Commit")
    }
}

impl<TYPES: NodeType> Committable for ViewSyncCommitData2<TYPES> {
    fn commit(&self) -> Commitment<Self> {
        let ViewSyncCommitData2 {
            relay,
            round,
            epoch: _,
        } = self;

        view_and_relay_commit::<TYPES, Self>(*round, *relay, "View Sync Commit")
    }
}

/// A trait for types belonging for specific epoch
pub trait HasEpoch<TYPES: NodeType> {
    /// Returns `Epoch`
    fn epoch(&self) -> TYPES::Epoch;
}

/// Helper macro for trivial implementation of the `HasEpoch` trait
#[macro_export]
macro_rules! impl_has_epoch {
    ($($t:ty),*) => {
        $(
            impl<TYPES: NodeType> HasEpoch<TYPES> for $t {
                fn epoch(&self) -> TYPES::Epoch {
                    self.epoch
                }
            }
        )*
    };
}

impl_has_epoch!(
    QuorumData2<TYPES>,
    DaData2<TYPES>,
    TimeoutData2<TYPES>,
    ViewSyncPreCommitData2<TYPES>,
    ViewSyncCommitData2<TYPES>,
    ViewSyncFinalizeData2<TYPES>
);

// impl votable for all the data types in this file sealed marker should ensure nothing is accidentally
// implemented for structs that aren't "voteable"
impl<
        TYPES: NodeType,
        V: sealed::Sealed + Committable + Clone + Serialize + Debug + PartialEq + Hash + Eq,
    > Voteable<TYPES> for V
{
}

// impl votable for all the data types in this file sealed marker should ensure nothing is accidently
// implemented for structs that aren't "voteable"
impl<
        TYPES: NodeType,
        V: sealed::Sealed
            + HasEpoch<TYPES>
            + Committable
            + Clone
            + Serialize
            + Debug
            + PartialEq
            + Hash
            + Eq,
    > Voteable2<TYPES> for V
{
}

impl<TYPES: NodeType> QuorumVote<TYPES> {
    /// Convert a `QuorumVote` to a `QuorumVote2`
    pub fn to_vote2(self) -> QuorumVote2<TYPES> {
        let bytes: [u8; 32] = self.data.leaf_commit.into();

        let signature = self.signature;
        let data = QuorumData2 {
            leaf_commit: Commitment::from_raw(bytes),
            epoch: TYPES::Epoch::genesis(),
        };
        let view_number = self.view_number;

        SimpleVote {
            signature,
            data,
            view_number,
        }
    }
}

impl<TYPES: NodeType> QuorumVote2<TYPES> {
    /// Convert a `QuorumVote2` to a `QuorumVote`
    pub fn to_vote(self) -> QuorumVote<TYPES> {
        let bytes: [u8; 32] = self.data.leaf_commit.into();

        let signature = self.signature;
        let data = QuorumData {
            leaf_commit: Commitment::from_raw(bytes),
        };
        let view_number = self.view_number;

        SimpleVote {
            signature,
            data,
            view_number,
        }
    }
}

impl<TYPES: NodeType> DaVote<TYPES> {
    /// Convert a `QuorumVote` to a `QuorumVote2`
    pub fn to_vote2(self) -> DaVote2<TYPES> {
        let signature = self.signature;
        let data = DaData2 {
            payload_commit: self.data.payload_commit,
            epoch: TYPES::Epoch::new(0),
        };
        let view_number = self.view_number;

        SimpleVote {
            signature,
            data,
            view_number,
        }
    }
}

impl<TYPES: NodeType> DaVote2<TYPES> {
    /// Convert a `QuorumVote2` to a `QuorumVote`
    pub fn to_vote(self) -> DaVote<TYPES> {
        let signature = self.signature;
        let data = DaData {
            payload_commit: self.data.payload_commit,
        };
        let view_number = self.view_number;

        SimpleVote {
            signature,
            data,
            view_number,
        }
    }
}

impl<TYPES: NodeType> TimeoutVote<TYPES> {
    /// Convert a `TimeoutVote` to a `TimeoutVote2`
    pub fn to_vote2(self) -> TimeoutVote2<TYPES> {
        let signature = self.signature;
        let data = TimeoutData2 {
            view: self.data.view,
            epoch: TYPES::Epoch::new(0),
        };
        let view_number = self.view_number;

        SimpleVote {
            signature,
            data,
            view_number,
        }
    }
}

impl<TYPES: NodeType> TimeoutVote2<TYPES> {
    /// Convert a `QuorumVote2` to a `QuorumVote`
    pub fn to_vote(self) -> TimeoutVote<TYPES> {
        let signature = self.signature;
        let data = TimeoutData {
            view: self.data.view,
        };
        let view_number = self.view_number;

        SimpleVote {
            signature,
            data,
            view_number,
        }
    }
}

impl<TYPES: NodeType> ViewSyncPreCommitVote<TYPES> {
    /// Convert a `ViewSyncPreCommitVote` to a `ViewSyncPreCommitVote2`
    pub fn to_vote2(self) -> ViewSyncPreCommitVote2<TYPES> {
        let signature = self.signature;
        let data = ViewSyncPreCommitData2 {
            relay: self.data.relay,
            round: self.data.round,
            epoch: TYPES::Epoch::new(0),
        };
        let view_number = self.view_number;

        SimpleVote {
            signature,
            data,
            view_number,
        }
    }
}

impl<TYPES: NodeType> ViewSyncPreCommitVote2<TYPES> {
    /// Convert a `ViewSyncPreCommitVote2` to a `ViewSyncPreCommitVote`
    pub fn to_vote(self) -> ViewSyncPreCommitVote<TYPES> {
        let signature = self.signature;
        let data = ViewSyncPreCommitData {
            relay: self.data.relay,
            round: self.data.round,
        };
        let view_number = self.view_number;

        SimpleVote {
            signature,
            data,
            view_number,
        }
    }
}

impl<TYPES: NodeType> ViewSyncCommitVote<TYPES> {
    /// Convert a `ViewSyncCommitVote` to a `ViewSyncCommitVote2`
    pub fn to_vote2(self) -> ViewSyncCommitVote2<TYPES> {
        let signature = self.signature;
        let data = ViewSyncCommitData2 {
            relay: self.data.relay,
            round: self.data.round,
            epoch: TYPES::Epoch::new(0),
        };
        let view_number = self.view_number;

        SimpleVote {
            signature,
            data,
            view_number,
        }
    }
}

impl<TYPES: NodeType> ViewSyncCommitVote2<TYPES> {
    /// Convert a `ViewSyncCommitVote2` to a `ViewSyncCommitVote`
    pub fn to_vote(self) -> ViewSyncCommitVote<TYPES> {
        let signature = self.signature;
        let data = ViewSyncCommitData {
            relay: self.data.relay,
            round: self.data.round,
        };
        let view_number = self.view_number;

        SimpleVote {
            signature,
            data,
            view_number,
        }
    }
}

impl<TYPES: NodeType> ViewSyncFinalizeVote<TYPES> {
    /// Convert a `ViewSyncFinalizeVote` to a `ViewSyncFinalizeVote2`
    pub fn to_vote2(self) -> ViewSyncFinalizeVote2<TYPES> {
        let signature = self.signature;
        let data = ViewSyncFinalizeData2 {
            relay: self.data.relay,
            round: self.data.round,
            epoch: TYPES::Epoch::new(0),
        };
        let view_number = self.view_number;

        SimpleVote {
            signature,
            data,
            view_number,
        }
    }
}

impl<TYPES: NodeType> ViewSyncFinalizeVote2<TYPES> {
    /// Convert a `ViewSyncFinalizeVote2` to a `ViewSyncFinalizeVote`
    pub fn to_vote(self) -> ViewSyncFinalizeVote<TYPES> {
        let signature = self.signature;
        let data = ViewSyncFinalizeData {
            relay: self.data.relay,
            round: self.data.round,
        };
        let view_number = self.view_number;

        SimpleVote {
            signature,
            data,
            view_number,
        }
    }
}

// Type aliases for simple use of all the main votes.  We should never see `SimpleVote` outside this file

/// Quorum vote Alias
pub type QuorumVote<TYPES> = SimpleVote<TYPES, QuorumData<TYPES>>;
// Type aliases for simple use of all the main votes.  We should never see `SimpleVote` outside this file
/// Quorum vote Alias
pub type QuorumVote2<TYPES> = SimpleVote<TYPES, QuorumData2<TYPES>>;

/// DA vote type alias
pub type DaVote<TYPES> = SimpleVote<TYPES, DaData>;
/// DA vote 2 type alias
pub type DaVote2<TYPES> = SimpleVote<TYPES, DaData2<TYPES>>;

/// Timeout Vote type alias
pub type TimeoutVote<TYPES> = SimpleVote<TYPES, TimeoutData<TYPES>>;
/// Timeout Vote 2 type alias
pub type TimeoutVote2<TYPES> = SimpleVote<TYPES, TimeoutData2<TYPES>>;

/// View Sync Pre Commit Vote type alias
pub type ViewSyncPreCommitVote<TYPES> = SimpleVote<TYPES, ViewSyncPreCommitData<TYPES>>;
/// View Sync Pre Commit Vote 2 type alias
pub type ViewSyncPreCommitVote2<TYPES> = SimpleVote<TYPES, ViewSyncPreCommitData2<TYPES>>;
/// View Sync Finalize Vote type alias
pub type ViewSyncFinalizeVote<TYPES> = SimpleVote<TYPES, ViewSyncFinalizeData<TYPES>>;
/// View Sync Finalize Vote 2 type alias
pub type ViewSyncFinalizeVote2<TYPES> = SimpleVote<TYPES, ViewSyncFinalizeData2<TYPES>>;
/// View Sync Commit Vote type alias
pub type ViewSyncCommitVote<TYPES> = SimpleVote<TYPES, ViewSyncCommitData<TYPES>>;
/// View Sync Commit Vote 2 type alias
pub type ViewSyncCommitVote2<TYPES> = SimpleVote<TYPES, ViewSyncCommitData2<TYPES>>;

/// Upgrade proposal vote
pub type UpgradeVote<TYPES> = SimpleVote<TYPES, UpgradeProposalData<TYPES>>;
/// Upgrade proposal 2 vote
pub type UpgradeVote2<TYPES> = SimpleVote<TYPES, UpgradeData2<TYPES>>;
