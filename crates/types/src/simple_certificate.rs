// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

//! Implementations of the simple certificate type.  Used for Quorum, DA, and Timeout Certificates

use std::{
    fmt::{self, Debug, Display, Formatter},
    hash::Hash,
    marker::PhantomData,
    sync::Arc,
};

use anyhow::{ensure, Result};
use async_lock::RwLock;
use committable::{Commitment, Committable};
use ethereum_types::U256;
use serde::{Deserialize, Serialize};

use crate::{
    data::serialize_signature2,
    message::UpgradeLock,
    simple_vote::{
        DaData, QuorumData, TimeoutData, UpgradeProposalData, VersionedVoteData,
        ViewSyncCommitData, ViewSyncFinalizeData, ViewSyncPreCommitData, Voteable,
    },
    traits::{
        election::Membership,
        node_implementation::{ConsensusTime, NodeType, Versions},
        signature_key::SignatureKey,
    },
    vote::{Certificate, HasViewNumber},
};

/// Trait which allows use to inject different threshold calculations into a Certificate type
pub trait Threshold<Key: SignatureKey, Time: ConsensusTime + Display> {
    /// Calculate a threshold based on the membership
    fn threshold<MEMBERSHIP: Membership<Key, Time>>(membership: &MEMBERSHIP) -> u64;
}

/// Defines a threshold which is 2f + 1 (Amount needed for Quorum)
#[derive(Serialize, Deserialize, Eq, Hash, PartialEq, Debug, Clone)]
pub struct SuccessThreshold {}

impl<Key: SignatureKey, Time: ConsensusTime + Display> Threshold<Key, Time> for SuccessThreshold {
    fn threshold<MEMBERSHIP: Membership<Key, Time>>(membership: &MEMBERSHIP) -> u64 {
        membership.success_threshold().into()
    }
}

/// Defines a threshold which is f + 1 (i.e at least one of the stake is honest)
#[derive(Serialize, Deserialize, Eq, Hash, PartialEq, Debug, Clone)]
pub struct OneHonestThreshold {}

impl<Key: SignatureKey, Time: ConsensusTime + Display> Threshold<Key, Time> for OneHonestThreshold {
    fn threshold<MEMBERSHIP: Membership<Key, Time>>(membership: &MEMBERSHIP) -> u64 {
        membership.failure_threshold().into()
    }
}

/// Defines a threshold which is 0.9n + 1 (i.e. over 90% of the nodes with stake)
#[derive(Serialize, Deserialize, Eq, Hash, PartialEq, Debug, Clone)]
pub struct UpgradeThreshold {}

impl<Key: SignatureKey, Time: ConsensusTime + Display> Threshold<Key, Time> for UpgradeThreshold {
    fn threshold<MEMBERSHIP: Membership<Key, Time>>(membership: &MEMBERSHIP) -> u64 {
        membership.upgrade_threshold().into()
    }
}

/// A certificate which can be created by aggregating many simple votes on the commitment.
#[derive(Serialize, Deserialize, Eq, Hash, PartialEq, Debug, Clone)]
pub struct SimpleCertificate<
    Key: SignatureKey,
    Time: ConsensusTime + Display,
    VOTEABLE: Voteable,
    THRESHOLD: Threshold<Key, Time>,
> {
    /// The data this certificate is for.  I.e the thing that was voted on to create this Certificate
    pub data: VOTEABLE,
    /// commitment of all the votes this cert should be signed over
    vote_commitment: Commitment<VOTEABLE>,
    /// Which view this QC relates to
    pub view_number: Time,
    /// assembled signature for certificate aggregation
    pub signatures: Option<<Key as SignatureKey>::QcType>,
    /// phantom data for `THRESHOLD` and `TYPES`
    pub _pd: PhantomData<(THRESHOLD)>,
}

impl<
        Key: SignatureKey,
        Time: ConsensusTime + Display,
        VOTEABLE: Voteable,
        THRESHOLD: Threshold<Key, Time>,
    > SimpleCertificate<Key, Time, VOTEABLE, THRESHOLD>
{
    /// Creates a new instance of `SimpleCertificate`
    pub fn new(
        data: VOTEABLE,
        vote_commitment: Commitment<VOTEABLE>,
        view_number: Time,
        signatures: Option<<Key as SignatureKey>::QcType>,
        pd: PhantomData<(THRESHOLD)>,
    ) -> Self {
        Self {
            data,
            vote_commitment,
            view_number,
            signatures,
            _pd: pd,
        }
    }
}

impl<
        Key: SignatureKey,
        Time: ConsensusTime + Display,
        VOTEABLE: Voteable + Committable,
        THRESHOLD: Threshold<Key, Time>,
    > Committable for SimpleCertificate<Key, Time, VOTEABLE, THRESHOLD>
{
    fn commit(&self) -> Commitment<Self> {
        let signature_bytes = match self.signatures.as_ref() {
            Some(sigs) => serialize_signature2::<Key>(sigs),
            None => vec![],
        };
        committable::RawCommitmentBuilder::new("Certificate")
            .field("data", self.data.commit())
            .field("vote_commitment", self.vote_commitment)
            .field("view number", self.view_number.commit())
            .var_size_field("signatures", &signature_bytes)
            .finalize()
    }
}

impl<
        Key: SignatureKey,
        Time: ConsensusTime + Display,
        VOTEABLE: Voteable + 'static,
        THRESHOLD: Threshold<Key, Time>,
    > Certificate<Key, Time> for SimpleCertificate<Key, Time, VOTEABLE, THRESHOLD>
{
    type Voteable = VOTEABLE;
    type Threshold = THRESHOLD;

    fn create_signed_certificate<V: Versions>(
        vote_commitment: Commitment<VersionedVoteData<Key, Time, VOTEABLE, V>>,
        data: Self::Voteable,
        sig: <Key as SignatureKey>::QcType,
        view: Time,
    ) -> Self {
        let vote_commitment_bytes: [u8; 32] = vote_commitment.into();

        SimpleCertificate {
            data,
            vote_commitment: Commitment::from_raw(vote_commitment_bytes),
            view_number: view,
            signatures: Some(sig),
            _pd: PhantomData,
        }
    }
    async fn is_valid_cert<MEMBERSHIP: Membership<Key, Time>, V: Versions>(
        &self,
        membership: &MEMBERSHIP,
        upgrade_lock: &UpgradeLock<Key, Time, V>,
    ) -> bool {
        if self.view_number == TYPES::Time::genesis() {
            return true;
        }
        let real_qc_pp = <Key as SignatureKey>::public_parameter(
            membership.stake_table(),
            U256::from(Self::threshold(membership)),
        );
        let Ok(commit) = self.date_commitment(upgrade_lock).await else {
            return false;
        };
        <Key as SignatureKey>::check(
            &real_qc_pp,
            commit.as_ref(),
            self.signatures.as_ref().unwrap(),
        )
    }
    fn threshold<MEMBERSHIP: Membership<Key, Time>>(membership: &MEMBERSHIP) -> u64 {
        THRESHOLD::threshold(membership)
    }
    fn date(&self) -> &Self::Voteable {
        &self.data
    }
    async fn date_commitment<V: Versions>(
        &self,
        upgrade_lock: &UpgradeLock<Key, Time, V>,
    ) -> Result<Commitment<VersionedVoteData<Key, Time, VOTEABLE, V>>> {
        Ok(
            VersionedVoteData::new(self.data.clone(), self.view_number, upgrade_lock)
                .await?
                .commit(),
        )
    }
}

impl<
        Key: SignatureKey,
        Time: ConsensusTime + Display,
        VOTEABLE: Voteable + 'static,
        THRESHOLD: Threshold<Key, Time>,
    > HasViewNumber<Time> for SimpleCertificate<Key, Time, VOTEABLE, THRESHOLD>
{
    fn view_number(&self) -> Time {
        self.view_number
    }
}
impl<Key: SignatureKey, Time: ConsensusTime + Display> Display for QuorumCertificate<Key, Time> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "view: {:?}", self.view_number)
    }
}

impl<Key: SignatureKey, Time: ConsensusTime + Display> UpgradeCertificate<Time> {
    /// Determines whether or not a certificate is relevant (i.e. we still have time to reach a
    /// decide)
    ///
    /// # Errors
    /// Returns an error when the certificate is no longer relevant
    pub async fn is_relevant(
        &self,
        view_number: Time,
        decided_upgrade_certificate: Arc<RwLock<Option<Self>>>,
    ) -> Result<()> {
        let decided_upgrade_certificate_read = decided_upgrade_certificate.read().await;
        ensure!(
            self.data.decide_by >= view_number
                || decided_upgrade_certificate_read
                    .clone()
                    .is_some_and(|cert| cert == *self),
            "Upgrade certificate is no longer relevant."
        );

        Ok(())
    }

    /// Validate an upgrade certificate.
    /// # Errors
    /// Returns an error when the upgrade certificate is invalid.
    pub async fn validate<V: Versions>(
        upgrade_certificate: &Option<Self>,
        quorum_membership: &Membership<Key, Time>,
        upgrade_lock: &UpgradeLock<Key, Time, V>,
    ) -> Result<()> {
        if let Some(ref cert) = upgrade_certificate {
            ensure!(
                cert.is_valid_cert(quorum_membership, upgrade_lock).await,
                "Invalid upgrade certificate."
            );
            Ok(())
        } else {
            Ok(())
        }
    }

    /// Given an upgrade certificate and a view, tests whether the view is in the period
    /// where we are upgrading, which requires that we propose with null blocks.
    pub fn upgrading_in(&self, view: Time) -> bool {
        view > self.data.old_version_last_view && view < self.data.new_version_first_view
    }
}

/// Type alias for a `QuorumCertificate`, which is a `SimpleCertificate` of `QuorumVotes`
pub type QuorumCertificate<Key, Time> =
    SimpleCertificate<Key, Time, QuorumData<Key, Time>, SuccessThreshold>;
/// Type alias for a DA certificate over `DaData`
pub type DaCertificate<Key, Time> = SimpleCertificate<Key, Time, DaData, SuccessThreshold>;
/// Type alias for a Timeout certificate over a view number
pub type TimeoutCertificate<Key, Time> =
    SimpleCertificate<Key, Time, TimeoutData<Key, Time>, SuccessThreshold>;
/// Type alias for a `ViewSyncPreCommit` certificate over a view number
pub type ViewSyncPreCommitCertificate2<Key, Time> =
    SimpleCertificate<Key, Time, ViewSyncPreCommitData<Key, Time>, OneHonestThreshold>;
/// Type alias for a `ViewSyncCommit` certificate over a view number
pub type ViewSyncCommitCertificate2<Key, Time> =
    SimpleCertificate<Key, Time, ViewSyncCommitData<Key, Time>, SuccessThreshold>;
/// Type alias for a `ViewSyncFinalize` certificate over a view number
pub type ViewSyncFinalizeCertificate2<Key, Time> =
    SimpleCertificate<Key, Time, ViewSyncFinalizeData<Key, Time>, SuccessThreshold>;
/// Type alias for a `UpgradeCertificate`, which is a `SimpleCertificate` of `UpgradeProposalData`
pub type UpgradeCertificate<Time> =
    SimpleCertificate<Key, Time, UpgradeProposalData<Time>, UpgradeThreshold>;
