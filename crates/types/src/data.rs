// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

//! Provides types useful for representing `HotShot`'s data structures
//!
//! This module provides types for representing consensus internal state, such as leaves,
//! `HotShot`'s version of a block, and proposals, messages upon which to reach the consensus.

use std::{
    collections::BTreeMap,
    fmt::{Debug, Display},
    hash::Hash,
    marker::PhantomData,
    sync::Arc,
};

use async_lock::RwLock;
use bincode::Options;
use committable::{Commitment, CommitmentBoundsArkless, Committable, RawCommitmentBuilder};
use jf_vid::{VidDisperse as JfVidDisperse, VidScheme};
use rand::Rng;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::task::spawn_blocking;
use utils::anytrace::*;
use vec1::Vec1;

use crate::{
    drb::DrbResult,
    impl_has_epoch, impl_has_none_epoch,
    message::{Proposal, UpgradeLock},
    simple_certificate::{
        NextEpochQuorumCertificate2, QuorumCertificate, QuorumCertificate2, TimeoutCertificate,
        TimeoutCertificate2, UpgradeCertificate, ViewSyncFinalizeCertificate,
        ViewSyncFinalizeCertificate2,
    },
    simple_vote::{HasEpoch, QuorumData, QuorumData2, UpgradeProposalData, VersionedVoteData},
    traits::{
        block_contents::{
            vid_commitment, BlockHeader, BuilderFee, EncodeBytes, TestableBlock,
            GENESIS_VID_NUM_STORAGE_NODES,
        },
        election::Membership,
        node_implementation::{ConsensusTime, NodeType, Versions},
        signature_key::SignatureKey,
        states::TestableState,
        BlockPayload,
    },
    utils::{bincode_opts, genesis_epoch_from_version, option_epoch_from_block_number},
    vid::{vid_scheme, VidCommitment, VidCommon, VidPrecomputeData, VidSchemeType, VidShare},
    vote::{Certificate, HasViewNumber},
};

/// Implements `ConsensusTime`, `Display`, `Add`, `AddAssign`, `Deref` and `Sub`
/// for the given thing wrapper type around u64.
macro_rules! impl_u64_wrapper {
    ($t:ty) => {
        impl ConsensusTime for $t {
            /// Create a genesis number (0)
            fn genesis() -> Self {
                Self(0)
            }
            /// Create a new number with the given value.
            fn new(n: u64) -> Self {
                Self(n)
            }
            /// Return the u64 format
            fn u64(&self) -> u64 {
                self.0
            }
        }

        impl Display for $t {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl std::ops::Add<u64> for $t {
            type Output = $t;

            fn add(self, rhs: u64) -> Self::Output {
                Self(self.0 + rhs)
            }
        }

        impl std::ops::AddAssign<u64> for $t {
            fn add_assign(&mut self, rhs: u64) {
                self.0 += rhs;
            }
        }

        impl std::ops::Deref for $t {
            type Target = u64;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl std::ops::Sub<u64> for $t {
            type Output = $t;
            fn sub(self, rhs: u64) -> Self::Output {
                Self(self.0 - rhs)
            }
        }
    };
}

/// Type-safe wrapper around `u64` so we know the thing we're talking about is a view number.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ViewNumber(u64);

impl Committable for ViewNumber {
    fn commit(&self) -> Commitment<Self> {
        let builder = RawCommitmentBuilder::new("View Number Commitment");
        builder.u64(self.0).finalize()
    }
}

impl_u64_wrapper!(ViewNumber);

/// Type-safe wrapper around `u64` so we know the thing we're talking about is a epoch number.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EpochNumber(u64);

impl Committable for EpochNumber {
    fn commit(&self) -> Commitment<Self> {
        let builder = RawCommitmentBuilder::new("Epoch Number Commitment");
        builder.u64(self.0).finalize()
    }
}

impl_u64_wrapper!(EpochNumber);

/// A proposal to start providing data availability for a block.
#[derive(derive_more::Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(bound = "TYPES: NodeType")]
pub struct DaProposal<TYPES: NodeType> {
    /// Encoded transactions in the block to be applied.
    pub encoded_transactions: Arc<[u8]>,
    /// Metadata of the block to be applied.
    pub metadata: <TYPES::BlockPayload as BlockPayload<TYPES>>::Metadata,
    /// View this proposal applies to
    pub view_number: TYPES::View,
}

/// A proposal to start providing data availability for a block.
#[derive(derive_more::Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(bound = "TYPES: NodeType")]
pub struct DaProposal2<TYPES: NodeType> {
    /// Encoded transactions in the block to be applied.
    pub encoded_transactions: Arc<[u8]>,
    /// Metadata of the block to be applied.
    pub metadata: <TYPES::BlockPayload as BlockPayload<TYPES>>::Metadata,
    /// View this proposal applies to
    pub view_number: TYPES::View,
    /// Epoch this proposal applies to
    pub epoch: Option<TYPES::Epoch>,
}

impl<TYPES: NodeType> From<DaProposal<TYPES>> for DaProposal2<TYPES> {
    fn from(da_proposal: DaProposal<TYPES>) -> Self {
        Self {
            encoded_transactions: da_proposal.encoded_transactions,
            metadata: da_proposal.metadata,
            view_number: da_proposal.view_number,
            epoch: None,
        }
    }
}

impl<TYPES: NodeType> From<DaProposal2<TYPES>> for DaProposal<TYPES> {
    fn from(da_proposal2: DaProposal2<TYPES>) -> Self {
        Self {
            encoded_transactions: da_proposal2.encoded_transactions,
            metadata: da_proposal2.metadata,
            view_number: da_proposal2.view_number,
        }
    }
}

/// A proposal to upgrade the network
#[derive(derive_more::Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(bound = "TYPES: NodeType")]
pub struct UpgradeProposal<TYPES>
where
    TYPES: NodeType,
{
    /// The information about which version we are upgrading to.
    pub upgrade_proposal: UpgradeProposalData<TYPES>,
    /// View this proposal applies to
    pub view_number: TYPES::View,
}

/// VID dispersal data
///
/// Like [`DaProposal`].
///
/// TODO move to vid.rs?
#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct VidDisperse<TYPES: NodeType> {
    /// The view number for which this VID data is intended
    pub view_number: TYPES::View,
    /// Epoch the data of this proposal belongs to
    pub epoch: Option<TYPES::Epoch>,
    /// Epoch to which the recipients of this VID belong to
    pub target_epoch: Option<TYPES::Epoch>,
    /// VidCommitment calculated based on the number of nodes in `target_epoch`.
    pub payload_commitment: VidCommitment,
    /// VidCommitment calculated based on the number of nodes in `epoch`. Needed during epoch transition.
    pub data_epoch_payload_commitment: Option<VidCommitment>,
    /// A storage node's key and its corresponding VID share
    pub shares: BTreeMap<TYPES::SignatureKey, VidShare>,
    /// VID common data sent to all storage nodes
    pub common: VidCommon,
}

impl<TYPES: NodeType> VidDisperse<TYPES> {
    /// Create VID dispersal from a specified membership for the target epoch.
    /// Uses the specified function to calculate share dispersal
    /// Allows for more complex stake table functionality
    pub async fn from_membership(
        view_number: TYPES::View,
        mut vid_disperse: JfVidDisperse<VidSchemeType>,
        membership: &Arc<RwLock<TYPES::Membership>>,
        target_epoch: Option<TYPES::Epoch>,
        data_epoch: Option<TYPES::Epoch>,
        data_epoch_payload_commitment: Option<VidCommitment>,
    ) -> Self {
        let shares = membership
            .read()
            .await
            .committee_members(view_number, target_epoch)
            .iter()
            .map(|node| (node.clone(), vid_disperse.shares.remove(0)))
            .collect();

        Self {
            view_number,
            shares,
            common: vid_disperse.common,
            payload_commitment: vid_disperse.commit,
            data_epoch_payload_commitment,
            epoch: data_epoch,
            target_epoch,
        }
    }

    /// Calculate the vid disperse information from the payload given a view, epoch and membership,
    /// optionally using precompute data from builder.
    /// If the sender epoch is missing, it means it's the same as the target epoch.
    ///
    /// # Errors
    /// Returns an error if the disperse or commitment calculation fails
    #[allow(clippy::panic)]
    pub async fn calculate_vid_disperse(
        payload: &TYPES::BlockPayload,
        membership: &Arc<RwLock<TYPES::Membership>>,
        view: TYPES::View,
        target_epoch: Option<TYPES::Epoch>,
        data_epoch: Option<TYPES::Epoch>,
    ) -> Result<Self> {
        let num_nodes = membership.read().await.total_nodes(target_epoch);

        let txns = payload.encode();
        let txns_clone = Arc::clone(&txns);
        let num_txns = txns.len();

        let vid_disperse = spawn_blocking(move || vid_scheme(num_nodes).disperse(&txns_clone))
            .await
            .wrap()
            .context(error!("Join error"))?
            .wrap()
            .context(|err| error!("Failed to calculate VID disperse. Error: {}", err))?;

        let payload_commitment = if target_epoch == data_epoch {
            None
        } else {
            let num_nodes = membership.read().await.total_nodes(data_epoch);

            Some(
              spawn_blocking(move || vid_scheme(num_nodes).commit_only(&txns))
                .await
                .wrap()
                .context(error!("Join error"))?
                .wrap()
                .context(|err| error!("Failed to calculate VID commitment with (num_storage_nodes, payload_byte_len) = ({}, {}). Error: {}", num_nodes, num_txns, err))?
            )
        };

        Ok(Self::from_membership(
            view,
            vid_disperse,
            membership,
            target_epoch,
            data_epoch,
            payload_commitment,
        )
        .await)
    }
}

/// Helper type to encapsulate the various ways that proposal certificates can be captured and
/// stored.
#[derive(derive_more::Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(bound(deserialize = ""))]
pub enum ViewChangeEvidence<TYPES: NodeType> {
    /// Holds a timeout certificate.
    Timeout(TimeoutCertificate<TYPES>),
    /// Holds a view sync finalized certificate.
    ViewSync(ViewSyncFinalizeCertificate<TYPES>),
}

impl<TYPES: NodeType> ViewChangeEvidence<TYPES> {
    /// Check that the given ViewChangeEvidence is relevant to the current view.
    pub fn is_valid_for_view(&self, view: &TYPES::View) -> bool {
        match self {
            ViewChangeEvidence::Timeout(timeout_cert) => timeout_cert.data().view == *view - 1,
            ViewChangeEvidence::ViewSync(view_sync_cert) => view_sync_cert.view_number == *view,
        }
    }

    /// Convert to ViewChangeEvidence2
    pub fn to_evidence2(self) -> ViewChangeEvidence2<TYPES> {
        match self {
            ViewChangeEvidence::Timeout(timeout_cert) => {
                ViewChangeEvidence2::Timeout(timeout_cert.to_tc2())
            }
            ViewChangeEvidence::ViewSync(view_sync_cert) => {
                ViewChangeEvidence2::ViewSync(view_sync_cert.to_vsc2())
            }
        }
    }
}

/// Helper type to encapsulate the various ways that proposal certificates can be captured and
/// stored.
#[derive(derive_more::Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(bound(deserialize = ""))]
pub enum ViewChangeEvidence2<TYPES: NodeType> {
    /// Holds a timeout certificate.
    Timeout(TimeoutCertificate2<TYPES>),
    /// Holds a view sync finalized certificate.
    ViewSync(ViewSyncFinalizeCertificate2<TYPES>),
}

impl<TYPES: NodeType> ViewChangeEvidence2<TYPES> {
    /// Check that the given ViewChangeEvidence2 is relevant to the current view.
    pub fn is_valid_for_view(&self, view: &TYPES::View) -> bool {
        match self {
            ViewChangeEvidence2::Timeout(timeout_cert) => timeout_cert.data().view == *view - 1,
            ViewChangeEvidence2::ViewSync(view_sync_cert) => view_sync_cert.view_number == *view,
        }
    }

    /// Convert to ViewChangeEvidence
    pub fn to_evidence(self) -> ViewChangeEvidence<TYPES> {
        match self {
            ViewChangeEvidence2::Timeout(timeout_cert) => {
                ViewChangeEvidence::Timeout(timeout_cert.to_tc())
            }
            ViewChangeEvidence2::ViewSync(view_sync_cert) => {
                ViewChangeEvidence::ViewSync(view_sync_cert.to_vsc())
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
/// VID share and associated metadata for a single node
pub struct VidDisperseShare<TYPES: NodeType> {
    /// The view number for which this VID data is intended
    pub view_number: TYPES::View,
    /// Block payload commitment
    pub payload_commitment: VidCommitment,
    /// A storage node's key and its corresponding VID share
    pub share: VidShare,
    /// VID common data sent to all storage nodes
    pub common: VidCommon,
    /// a public key of the share recipient
    pub recipient_key: TYPES::SignatureKey,
}

impl<TYPES: NodeType> VidDisperseShare<TYPES> {
    /// Create a vector of `VidDisperseShare` from `VidDisperse`
    pub fn from_vid_disperse(vid_disperse: VidDisperse<TYPES>) -> Vec<Self> {
        vid_disperse
            .shares
            .into_iter()
            .map(|(recipient_key, share)| Self {
                share,
                recipient_key,
                view_number: vid_disperse.view_number,
                common: vid_disperse.common.clone(),
                payload_commitment: vid_disperse.payload_commitment,
            })
            .collect()
    }

    /// Consume `self` and return a `Proposal`
    pub fn to_proposal(
        self,
        private_key: &<TYPES::SignatureKey as SignatureKey>::PrivateKey,
    ) -> Option<Proposal<TYPES, Self>> {
        let Ok(signature) =
            TYPES::SignatureKey::sign(private_key, self.payload_commitment.as_ref())
        else {
            tracing::error!("VID: failed to sign dispersal share payload");
            return None;
        };
        Some(Proposal {
            signature,
            _pd: PhantomData,
            data: self,
        })
    }

    /// Create `VidDisperse` out of an iterator to `VidDisperseShare`s
    pub fn to_vid_disperse<'a, I>(mut it: I) -> Option<VidDisperse<TYPES>>
    where
        I: Iterator<Item = &'a Self>,
    {
        let first_vid_disperse_share = it.next()?.clone();
        let mut share_map = BTreeMap::new();
        share_map.insert(
            first_vid_disperse_share.recipient_key,
            first_vid_disperse_share.share,
        );
        let mut vid_disperse = VidDisperse {
            view_number: first_vid_disperse_share.view_number,
            epoch: None,
            target_epoch: None,
            payload_commitment: first_vid_disperse_share.payload_commitment,
            data_epoch_payload_commitment: None,
            common: first_vid_disperse_share.common,
            shares: share_map,
        };
        let _ = it.map(|vid_disperse_share| {
            vid_disperse.shares.insert(
                vid_disperse_share.recipient_key.clone(),
                vid_disperse_share.share.clone(),
            )
        });
        Some(vid_disperse)
    }

    /// Split a VID share proposal into a proposal for each recipient.
    pub fn to_vid_share_proposals(
        vid_disperse_proposal: Proposal<TYPES, VidDisperse<TYPES>>,
    ) -> Vec<Proposal<TYPES, Self>> {
        vid_disperse_proposal
            .data
            .shares
            .into_iter()
            .map(|(recipient_key, share)| Proposal {
                data: Self {
                    share,
                    recipient_key,
                    view_number: vid_disperse_proposal.data.view_number,
                    common: vid_disperse_proposal.data.common.clone(),
                    payload_commitment: vid_disperse_proposal.data.payload_commitment,
                },
                signature: vid_disperse_proposal.signature.clone(),
                _pd: vid_disperse_proposal._pd,
            })
            .collect()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
/// VID share and associated metadata for a single node
pub struct VidDisperseShare2<TYPES: NodeType> {
    /// The view number for which this VID data is intended
    pub view_number: TYPES::View,
    /// The epoch number for which this VID data belongs to
    pub epoch: Option<TYPES::Epoch>,
    /// The epoch number to which the recipient of this VID belongs to
    pub target_epoch: Option<TYPES::Epoch>,
    /// Block payload commitment
    pub payload_commitment: VidCommitment,
    /// VidCommitment calculated based on the number of nodes in `epoch`. Needed during epoch transition.
    pub data_epoch_payload_commitment: Option<VidCommitment>,
    /// A storage node's key and its corresponding VID share
    pub share: VidShare,
    /// VID common data sent to all storage nodes
    pub common: VidCommon,
    /// a public key of the share recipient
    pub recipient_key: TYPES::SignatureKey,
}

impl<TYPES: NodeType> From<VidDisperseShare2<TYPES>> for VidDisperseShare<TYPES> {
    fn from(vid_disperse2: VidDisperseShare2<TYPES>) -> Self {
        let VidDisperseShare2 {
            view_number,
            epoch: _,
            target_epoch: _,
            payload_commitment,
            data_epoch_payload_commitment: _,
            share,
            common,
            recipient_key,
        } = vid_disperse2;

        Self {
            view_number,
            payload_commitment,
            share,
            common,
            recipient_key,
        }
    }
}

impl<TYPES: NodeType> From<VidDisperseShare<TYPES>> for VidDisperseShare2<TYPES> {
    fn from(vid_disperse: VidDisperseShare<TYPES>) -> Self {
        let VidDisperseShare {
            view_number,
            payload_commitment,
            share,
            common,
            recipient_key,
        } = vid_disperse;

        Self {
            view_number,
            epoch: None,
            target_epoch: None,
            payload_commitment,
            data_epoch_payload_commitment: None,
            share,
            common,
            recipient_key,
        }
    }
}

impl<TYPES: NodeType> VidDisperseShare2<TYPES> {
    /// Create a vector of `VidDisperseShare` from `VidDisperse`
    pub fn from_vid_disperse(vid_disperse: VidDisperse<TYPES>) -> Vec<Self> {
        vid_disperse
            .shares
            .into_iter()
            .map(|(recipient_key, share)| Self {
                share,
                recipient_key,
                view_number: vid_disperse.view_number,
                common: vid_disperse.common.clone(),
                payload_commitment: vid_disperse.payload_commitment,
                data_epoch_payload_commitment: vid_disperse.data_epoch_payload_commitment,
                epoch: vid_disperse.epoch,
                target_epoch: vid_disperse.target_epoch,
            })
            .collect()
    }

    /// Consume `self` and return a `Proposal`
    pub fn to_proposal(
        self,
        private_key: &<TYPES::SignatureKey as SignatureKey>::PrivateKey,
    ) -> Option<Proposal<TYPES, Self>> {
        let Ok(signature) =
            TYPES::SignatureKey::sign(private_key, self.payload_commitment.as_ref())
        else {
            tracing::error!("VID: failed to sign dispersal share payload");
            return None;
        };
        Some(Proposal {
            signature,
            _pd: PhantomData,
            data: self,
        })
    }

    /// Create `VidDisperse` out of an iterator to `VidDisperseShare`s
    pub fn to_vid_disperse<'a, I>(mut it: I) -> Option<VidDisperse<TYPES>>
    where
        I: Iterator<Item = &'a Self>,
    {
        let first_vid_disperse_share = it.next()?.clone();
        let mut share_map = BTreeMap::new();
        share_map.insert(
            first_vid_disperse_share.recipient_key,
            first_vid_disperse_share.share,
        );
        let mut vid_disperse = VidDisperse {
            view_number: first_vid_disperse_share.view_number,
            epoch: first_vid_disperse_share.epoch,
            target_epoch: first_vid_disperse_share.target_epoch,
            payload_commitment: first_vid_disperse_share.payload_commitment,
            data_epoch_payload_commitment: first_vid_disperse_share.data_epoch_payload_commitment,
            common: first_vid_disperse_share.common,
            shares: share_map,
        };
        let _ = it.map(|vid_disperse_share| {
            vid_disperse.shares.insert(
                vid_disperse_share.recipient_key.clone(),
                vid_disperse_share.share.clone(),
            )
        });
        Some(vid_disperse)
    }

    /// Split a VID share proposal into a proposal for each recipient.
    pub fn to_vid_share_proposals(
        vid_disperse_proposal: Proposal<TYPES, VidDisperse<TYPES>>,
    ) -> Vec<Proposal<TYPES, Self>> {
        vid_disperse_proposal
            .data
            .shares
            .into_iter()
            .map(|(recipient_key, share)| Proposal {
                data: Self {
                    share,
                    recipient_key,
                    view_number: vid_disperse_proposal.data.view_number,
                    common: vid_disperse_proposal.data.common.clone(),
                    payload_commitment: vid_disperse_proposal.data.payload_commitment,
                    data_epoch_payload_commitment: vid_disperse_proposal
                        .data
                        .data_epoch_payload_commitment,
                    epoch: vid_disperse_proposal.data.epoch,
                    target_epoch: vid_disperse_proposal.data.target_epoch,
                },
                signature: vid_disperse_proposal.signature.clone(),
                _pd: vid_disperse_proposal._pd,
            })
            .collect()
    }
}

/// Proposal to append a block.
#[derive(derive_more::Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(bound(deserialize = ""))]
pub struct QuorumProposal<TYPES: NodeType> {
    /// The block header to append
    pub block_header: TYPES::BlockHeader,

    /// CurView from leader when proposing leaf
    pub view_number: TYPES::View,

    /// Per spec, justification
    pub justify_qc: QuorumCertificate<TYPES>,

    /// Possible upgrade certificate, which the leader may optionally attach.
    pub upgrade_certificate: Option<UpgradeCertificate<TYPES>>,

    /// Possible timeout or view sync certificate.
    /// - A timeout certificate is only present if the justify_qc is not for the preceding view
    /// - A view sync certificate is only present if the justify_qc and timeout_cert are not
    ///   present.
    pub proposal_certificate: Option<ViewChangeEvidence<TYPES>>,
}

/// Proposal to append a block.
#[derive(derive_more::Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(bound(deserialize = ""))]
pub struct QuorumProposal2<TYPES: NodeType> {
    /// The block header to append
    pub block_header: TYPES::BlockHeader,

    /// view number for the proposal
    pub view_number: TYPES::View,

    /// certificate that the proposal is chaining from
    pub justify_qc: QuorumCertificate2<TYPES>,

    /// certificate that the proposal is chaining from formed by the next epoch nodes
    pub next_epoch_justify_qc: Option<NextEpochQuorumCertificate2<TYPES>>,

    /// Possible upgrade certificate, which the leader may optionally attach.
    pub upgrade_certificate: Option<UpgradeCertificate<TYPES>>,

    /// Possible timeout or view sync certificate. If the `justify_qc` is not for a proposal in the immediately preceding view, then either a timeout or view sync certificate must be attached.
    pub view_change_evidence: Option<ViewChangeEvidence2<TYPES>>,

    /// The DRB result for the next epoch.
    ///
    /// This is required only for the last block of the epoch. Nodes will verify that it's
    /// consistent with the result from their computations.
    #[serde(with = "serde_bytes")]
    pub next_drb_result: Option<DrbResult>,
}

/// Wrapper around a proposal to append a block
#[derive(derive_more::Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(bound(deserialize = ""))]
pub struct QuorumProposalWrapper<TYPES: NodeType> {
    /// The wrapped proposal
    pub proposal: QuorumProposal2<TYPES>,

    /// Indicates whether or not epochs were enabled for this proposal. This is checked when building a QuorumProposalWrapper
    pub with_epoch: bool,
}

impl<TYPES: NodeType> QuorumProposalWrapper<TYPES> {
    /// Helper function to get the proposal's block_header
    pub fn block_header(&self) -> &TYPES::BlockHeader {
        &self.proposal.block_header
    }

    /// Helper function to get the proposal's view_number
    pub fn view_number(&self) -> TYPES::View {
        self.proposal.view_number
    }

    /// Helper function to get the proposal's justify_qc
    pub fn justify_qc(&self) -> &QuorumCertificate2<TYPES> {
        &self.proposal.justify_qc
    }

    /// Helper function to get the proposal's next_epoch_justify_qc
    pub fn next_epoch_justify_qc(&self) -> &Option<NextEpochQuorumCertificate2<TYPES>> {
        &self.proposal.next_epoch_justify_qc
    }

    /// Helper function to get the proposal's upgrade_certificate
    pub fn upgrade_certificate(&self) -> &Option<UpgradeCertificate<TYPES>> {
        &self.proposal.upgrade_certificate
    }

    /// Helper function to get the proposal's view_change_evidence
    pub fn view_change_evidence(&self) -> &Option<ViewChangeEvidence2<TYPES>> {
        &self.proposal.view_change_evidence
    }

    /// Helper function to get the proposal's next_drb_result
    pub fn next_drb_result(&self) -> &Option<DrbResult> {
        &self.proposal.next_drb_result
    }
}

impl<TYPES: NodeType> From<QuorumProposal<TYPES>> for QuorumProposalWrapper<TYPES> {
    fn from(quorum_proposal: QuorumProposal<TYPES>) -> Self {
        Self {
            proposal: quorum_proposal.into(),
            with_epoch: false,
        }
    }
}

impl<TYPES: NodeType> From<QuorumProposal2<TYPES>> for QuorumProposalWrapper<TYPES> {
    fn from(quorum_proposal2: QuorumProposal2<TYPES>) -> Self {
        Self {
            proposal: quorum_proposal2,
            with_epoch: true,
        }
    }
}

impl<TYPES: NodeType> From<QuorumProposalWrapper<TYPES>> for QuorumProposal<TYPES> {
    fn from(quorum_proposal_wrapper: QuorumProposalWrapper<TYPES>) -> Self {
        quorum_proposal_wrapper.proposal.into()
    }
}

impl<TYPES: NodeType> From<QuorumProposalWrapper<TYPES>> for QuorumProposal2<TYPES> {
    fn from(quorum_proposal_wrapper: QuorumProposalWrapper<TYPES>) -> Self {
        quorum_proposal_wrapper.proposal
    }
}

impl<TYPES: NodeType> From<QuorumProposal<TYPES>> for QuorumProposal2<TYPES> {
    fn from(quorum_proposal: QuorumProposal<TYPES>) -> Self {
        Self {
            block_header: quorum_proposal.block_header,
            view_number: quorum_proposal.view_number,
            justify_qc: quorum_proposal.justify_qc.to_qc2(),
            next_epoch_justify_qc: None,
            upgrade_certificate: quorum_proposal.upgrade_certificate,
            view_change_evidence: quorum_proposal
                .proposal_certificate
                .map(ViewChangeEvidence::to_evidence2),
            next_drb_result: None,
        }
    }
}

impl<TYPES: NodeType> From<QuorumProposal2<TYPES>> for QuorumProposal<TYPES> {
    fn from(quorum_proposal2: QuorumProposal2<TYPES>) -> Self {
        Self {
            block_header: quorum_proposal2.block_header,
            view_number: quorum_proposal2.view_number,
            justify_qc: quorum_proposal2.justify_qc.to_qc(),
            upgrade_certificate: quorum_proposal2.upgrade_certificate,
            proposal_certificate: quorum_proposal2
                .view_change_evidence
                .map(ViewChangeEvidence2::to_evidence),
        }
    }
}

impl<TYPES: NodeType> From<Leaf<TYPES>> for Leaf2<TYPES> {
    fn from(leaf: Leaf<TYPES>) -> Self {
        let bytes: [u8; 32] = leaf.parent_commitment.into();

        Self {
            view_number: leaf.view_number,
            justify_qc: leaf.justify_qc.to_qc2(),
            next_epoch_justify_qc: None,
            parent_commitment: Commitment::from_raw(bytes),
            block_header: leaf.block_header,
            upgrade_certificate: leaf.upgrade_certificate,
            block_payload: leaf.block_payload,
            view_change_evidence: None,
            next_drb_result: None,
            with_epoch: false,
        }
    }
}

impl<TYPES: NodeType> HasViewNumber<TYPES> for DaProposal<TYPES> {
    fn view_number(&self) -> TYPES::View {
        self.view_number
    }
}

impl<TYPES: NodeType> HasViewNumber<TYPES> for DaProposal2<TYPES> {
    fn view_number(&self) -> TYPES::View {
        self.view_number
    }
}

impl<TYPES: NodeType> HasViewNumber<TYPES> for VidDisperse<TYPES> {
    fn view_number(&self) -> TYPES::View {
        self.view_number
    }
}

impl<TYPES: NodeType> HasViewNumber<TYPES> for VidDisperseShare<TYPES> {
    fn view_number(&self) -> TYPES::View {
        self.view_number
    }
}

impl<TYPES: NodeType> HasViewNumber<TYPES> for VidDisperseShare2<TYPES> {
    fn view_number(&self) -> TYPES::View {
        self.view_number
    }
}

impl<TYPES: NodeType> HasViewNumber<TYPES> for QuorumProposal<TYPES> {
    fn view_number(&self) -> TYPES::View {
        self.view_number
    }
}

impl<TYPES: NodeType> HasViewNumber<TYPES> for QuorumProposal2<TYPES> {
    fn view_number(&self) -> TYPES::View {
        self.view_number
    }
}

impl<TYPES: NodeType> HasViewNumber<TYPES> for QuorumProposalWrapper<TYPES> {
    fn view_number(&self) -> TYPES::View {
        self.proposal.view_number
    }
}

impl<TYPES: NodeType> HasViewNumber<TYPES> for UpgradeProposal<TYPES> {
    fn view_number(&self) -> TYPES::View {
        self.view_number
    }
}

impl_has_epoch!(
    DaProposal2<TYPES>,
    VidDisperse<TYPES>,
    VidDisperseShare2<TYPES>
);

impl_has_none_epoch!(
    QuorumProposal<TYPES>,
    DaProposal<TYPES>,
    VidDisperseShare<TYPES>,
    UpgradeProposal<TYPES>
);

impl<TYPES: NodeType> HasEpoch<TYPES> for QuorumProposal2<TYPES> {
    /// Never call this trait method for QuorumProposal2
    /// # Panics
    /// Always. Calling this trait method for QuorumProposal2 is incorrect.
    /// QuorumProposal2 has an associated epoch but it needs to be calculated based on
    /// the block number and the epoch height.
    #[allow(clippy::panic)]
    fn epoch(&self) -> Option<TYPES::Epoch> {
        panic!("Never call this trait method for QuorumProposal2")
    }
}

impl<TYPES: NodeType> HasEpoch<TYPES> for QuorumProposalWrapper<TYPES> {
    /// Never call this trait method for QuorumProposalWrapper
    /// # Panics
    /// Always. Calling this trait method for QuorumProposalWrapper is incorrect.
    /// QuorumProposalWrapper might have an associated epoch but it needs to be calculated based on
    /// the block number and the epoch height.
    #[allow(clippy::panic)]
    fn epoch(&self) -> Option<TYPES::Epoch> {
        panic!("Never call this trait method for QuorumProposal2")
    }
}

/// The error type for block and its transactions.
#[derive(Error, Debug, Serialize, Deserialize)]
pub enum BlockError {
    /// The block header is invalid
    #[error("Invalid block header: {0}")]
    InvalidBlockHeader(String),

    /// The payload commitment does not match the block header's payload commitment
    #[error("Inconsistent payload commitment")]
    InconsistentPayloadCommitment,
}

/// Additional functions required to use a [`Leaf`] with hotshot-testing.
pub trait TestableLeaf {
    /// Type of nodes participating in the network.
    type NodeType: NodeType;

    /// Create a transaction that can be added to the block contained in this leaf.
    fn create_random_transaction(
        &self,
        rng: &mut dyn rand::RngCore,
        padding: u64,
    ) -> <<Self::NodeType as NodeType>::BlockPayload as BlockPayload<Self::NodeType>>::Transaction;
}

/// This is the consensus-internal analogous concept to a block, and it contains the block proper,
/// as well as the hash of its parent `Leaf`.
/// NOTE: `State` is constrained to implementing `BlockContents`, is `TypeMap::BlockPayload`
#[derive(Serialize, Deserialize, Clone, Debug, Eq)]
#[serde(bound(deserialize = ""))]
pub struct Leaf<TYPES: NodeType> {
    /// CurView from leader when proposing leaf
    view_number: TYPES::View,

    /// Per spec, justification
    justify_qc: QuorumCertificate<TYPES>,

    /// The hash of the parent `Leaf`
    /// So we can ask if it extends
    parent_commitment: Commitment<Self>,

    /// Block header.
    block_header: TYPES::BlockHeader,

    /// Optional upgrade certificate, if one was attached to the quorum proposal for this view.
    upgrade_certificate: Option<UpgradeCertificate<TYPES>>,

    /// Optional block payload.
    ///
    /// It may be empty for nodes not in the DA committee.
    block_payload: Option<TYPES::BlockPayload>,
}

/// This is the consensus-internal analogous concept to a block, and it contains the block proper,
/// as well as the hash of its parent `Leaf`.
#[derive(Serialize, Deserialize, Clone, Debug, Eq)]
#[serde(bound(deserialize = ""))]
pub struct Leaf2<TYPES: NodeType> {
    /// CurView from leader when proposing leaf
    view_number: TYPES::View,

    /// Per spec, justification
    justify_qc: QuorumCertificate2<TYPES>,

    /// certificate that the proposal is chaining from formed by the next epoch nodes
    next_epoch_justify_qc: Option<NextEpochQuorumCertificate2<TYPES>>,

    /// The hash of the parent `Leaf`
    /// So we can ask if it extends
    parent_commitment: Commitment<Self>,

    /// Block header.
    block_header: TYPES::BlockHeader,

    /// Optional upgrade certificate, if one was attached to the quorum proposal for this view.
    upgrade_certificate: Option<UpgradeCertificate<TYPES>>,

    /// Optional block payload.
    ///
    /// It may be empty for nodes not in the DA committee.
    block_payload: Option<TYPES::BlockPayload>,

    /// Possible timeout or view sync certificate. If the `justify_qc` is not for a proposal in the immediately preceding view, then either a timeout or view sync certificate must be attached.
    pub view_change_evidence: Option<ViewChangeEvidence2<TYPES>>,

    /// The DRB result for the next epoch.
    ///
    /// This is required only for the last block of the epoch. Nodes will verify that it's
    /// consistent with the result from their computations.
    #[serde(with = "serde_bytes")]
    pub next_drb_result: Option<DrbResult>,

    /// Indicates whether or not epochs were enabled.
    pub with_epoch: bool,
}

impl<TYPES: NodeType> Leaf2<TYPES> {
    /// Create a new leaf from its components.
    ///
    /// # Panics
    ///
    /// Panics if the genesis payload (`TYPES::BlockPayload::genesis()`) is malformed (unable to be
    /// interpreted as bytes).
    #[must_use]
    pub async fn genesis<V: Versions>(
        validated_state: &TYPES::ValidatedState,
        instance_state: &TYPES::InstanceState,
    ) -> Self {
        let epoch = genesis_epoch_from_version::<V, TYPES>();

        let (payload, metadata) =
            TYPES::BlockPayload::from_transactions([], validated_state, instance_state)
                .await
                .unwrap();
        let builder_commitment = payload.builder_commitment(&metadata);
        let payload_bytes = payload.encode();

        let payload_commitment = vid_commitment(&payload_bytes, GENESIS_VID_NUM_STORAGE_NODES);

        let block_header = TYPES::BlockHeader::genesis(
            instance_state,
            payload_commitment,
            builder_commitment,
            metadata,
        );

        let null_quorum_data = QuorumData2 {
            leaf_commit: Commitment::<Leaf2<TYPES>>::default_commitment_no_preimage(),
            epoch,
        };

        let justify_qc = QuorumCertificate2::new(
            null_quorum_data.clone(),
            null_quorum_data.commit(),
            <TYPES::View as ConsensusTime>::genesis(),
            None,
            PhantomData,
        );

        Self {
            view_number: TYPES::View::genesis(),
            justify_qc,
            next_epoch_justify_qc: None,
            parent_commitment: null_quorum_data.leaf_commit,
            upgrade_certificate: None,
            block_header: block_header.clone(),
            block_payload: Some(payload),
            view_change_evidence: None,
            next_drb_result: None,
            with_epoch: epoch.is_some(),
        }
    }
    /// Time when this leaf was created.
    pub fn view_number(&self) -> TYPES::View {
        self.view_number
    }
    /// Epoch in which this leaf was created.
    pub fn epoch(&self, epoch_height: u64) -> Option<TYPES::Epoch> {
        option_epoch_from_block_number::<TYPES>(
            self.with_epoch,
            self.block_header.block_number(),
            epoch_height,
        )
    }
    /// Height of this leaf in the chain.
    ///
    /// Equivalently, this is the number of leaves before this one in the chain.
    pub fn height(&self) -> u64 {
        self.block_header.block_number()
    }
    /// The QC linking this leaf to its parent in the chain.
    pub fn justify_qc(&self) -> QuorumCertificate2<TYPES> {
        self.justify_qc.clone()
    }
    /// The QC linking this leaf to its parent in the chain.
    pub fn upgrade_certificate(&self) -> Option<UpgradeCertificate<TYPES>> {
        self.upgrade_certificate.clone()
    }
    /// Commitment to this leaf's parent.
    pub fn parent_commitment(&self) -> Commitment<Self> {
        self.parent_commitment
    }
    /// The block header contained in this leaf.
    pub fn block_header(&self) -> &<TYPES as NodeType>::BlockHeader {
        &self.block_header
    }

    /// Get a mutable reference to the block header contained in this leaf.
    pub fn block_header_mut(&mut self) -> &mut <TYPES as NodeType>::BlockHeader {
        &mut self.block_header
    }
    /// Fill this leaf with the block payload.
    ///
    /// # Errors
    ///
    /// Fails if the payload commitment doesn't match `self.block_header.payload_commitment()`
    /// or if the transactions are of invalid length
    pub fn fill_block_payload(
        &mut self,
        block_payload: TYPES::BlockPayload,
        num_storage_nodes: usize,
    ) -> std::result::Result<(), BlockError> {
        let encoded_txns = block_payload.encode();
        let commitment = vid_commitment(&encoded_txns, num_storage_nodes);
        if commitment != self.block_header.payload_commitment() {
            return Err(BlockError::InconsistentPayloadCommitment);
        }
        self.block_payload = Some(block_payload);
        Ok(())
    }

    /// Take the block payload from the leaf and return it if it is present
    pub fn unfill_block_payload(&mut self) -> Option<TYPES::BlockPayload> {
        self.block_payload.take()
    }

    /// Fill this leaf with the block payload, without checking
    /// header and payload consistency
    pub fn fill_block_payload_unchecked(&mut self, block_payload: TYPES::BlockPayload) {
        self.block_payload = Some(block_payload);
    }

    /// Optional block payload.
    pub fn block_payload(&self) -> Option<TYPES::BlockPayload> {
        self.block_payload.clone()
    }

    /// A commitment to the block payload contained in this leaf.
    pub fn payload_commitment(&self) -> VidCommitment {
        self.block_header().payload_commitment()
    }

    /// Validate that a leaf has the right upgrade certificate to be the immediate child of another leaf
    ///
    /// This may not be a complete function. Please double-check that it performs the checks you expect before substituting validation logic with it.
    ///
    /// # Errors
    /// Returns an error if the certificates are not identical, or that when we no longer see a
    /// cert, it's for the right reason.
    pub async fn extends_upgrade(
        &self,
        parent: &Self,
        decided_upgrade_certificate: &Arc<RwLock<Option<UpgradeCertificate<TYPES>>>>,
    ) -> Result<()> {
        match (self.upgrade_certificate(), parent.upgrade_certificate()) {
            // Easiest cases are:
            //   - no upgrade certificate on either: this is the most common case, and is always fine.
            //   - if the parent didn't have a certificate, but we see one now, it just means that we have begun an upgrade: again, this is always fine.
            (None | Some(_), None) => {}
            // If we no longer see a cert, we have to make sure that we either:
            //    - no longer care because we have passed new_version_first_view, or
            //    - no longer care because we have passed `decide_by` without deciding the certificate.
            (None, Some(parent_cert)) => {
                let decided_upgrade_certificate_read = decided_upgrade_certificate.read().await;
                ensure!(self.view_number() > parent_cert.data.new_version_first_view
                    || (self.view_number() > parent_cert.data.decide_by && decided_upgrade_certificate_read.is_none()),
                       "The new leaf is missing an upgrade certificate that was present in its parent, and should still be live."
                );
            }
            // If we both have a certificate, they should be identical.
            // Technically, this prevents us from initiating a new upgrade in the view immediately following an upgrade.
            // I think this is a fairly lax restriction.
            (Some(cert), Some(parent_cert)) => {
                ensure!(cert == parent_cert, "The new leaf does not extend the parent leaf, because it has attached a different upgrade certificate.");
            }
        }

        // This check should be added once we sort out the genesis leaf/justify_qc issue.
        // ensure!(self.parent_commitment() == parent_leaf.commit(), "The commitment of the parent leaf does not match the specified parent commitment.");

        Ok(())
    }

    /// Converts a `Leaf2` to a `Leaf`. This operation is fundamentally unsafe and should not be used.
    pub fn to_leaf_unsafe(self) -> Leaf<TYPES> {
        let bytes: [u8; 32] = self.parent_commitment.into();

        Leaf {
            view_number: self.view_number,
            justify_qc: self.justify_qc.to_qc(),
            parent_commitment: Commitment::from_raw(bytes),
            block_header: self.block_header,
            upgrade_certificate: self.upgrade_certificate,
            block_payload: self.block_payload,
        }
    }
}

impl<TYPES: NodeType> Committable for Leaf2<TYPES> {
    fn commit(&self) -> committable::Commitment<Self> {
        RawCommitmentBuilder::new("leaf commitment")
            .u64_field("view number", *self.view_number)
            .field("parent leaf commitment", self.parent_commitment)
            .field("block header", self.block_header.commit())
            .field("justify qc", self.justify_qc.commit())
            .optional("upgrade certificate", &self.upgrade_certificate)
            .finalize()
    }
}

impl<TYPES: NodeType> Leaf<TYPES> {
    #[allow(clippy::unused_async)]
    /// Calculate the leaf commitment,
    /// which is gated on the version to include the block header.
    pub async fn commit<V: Versions>(
        &self,
        _upgrade_lock: &UpgradeLock<TYPES, V>,
    ) -> Commitment<Self> {
        <Self as Committable>::commit(self)
    }
}

impl<TYPES: NodeType> PartialEq for Leaf<TYPES> {
    fn eq(&self, other: &Self) -> bool {
        self.view_number == other.view_number
            && self.justify_qc == other.justify_qc
            && self.parent_commitment == other.parent_commitment
            && self.block_header == other.block_header
    }
}

impl<TYPES: NodeType> PartialEq for Leaf2<TYPES> {
    fn eq(&self, other: &Self) -> bool {
        let Leaf2 {
            view_number,
            justify_qc,
            next_epoch_justify_qc,
            parent_commitment,
            block_header,
            upgrade_certificate,
            block_payload: _,
            view_change_evidence,
            next_drb_result,
            with_epoch,
        } = self;

        *view_number == other.view_number
            && *justify_qc == other.justify_qc
            && *next_epoch_justify_qc == other.next_epoch_justify_qc
            && *parent_commitment == other.parent_commitment
            && *block_header == other.block_header
            && *upgrade_certificate == other.upgrade_certificate
            && *view_change_evidence == other.view_change_evidence
            && *next_drb_result == other.next_drb_result
            && *with_epoch == other.with_epoch
    }
}

impl<TYPES: NodeType> Hash for Leaf<TYPES> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.view_number.hash(state);
        self.justify_qc.hash(state);
        self.parent_commitment.hash(state);
        self.block_header.hash(state);
    }
}

impl<TYPES: NodeType> Hash for Leaf2<TYPES> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.commit().hash(state);
        self.view_number.hash(state);
        self.justify_qc.hash(state);
        self.parent_commitment.hash(state);
        self.block_header.hash(state);
    }
}

impl<TYPES: NodeType> Display for Leaf<TYPES> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "view: {:?}, height: {:?}, justify: {}",
            self.view_number,
            self.height(),
            self.justify_qc
        )
    }
}

impl<TYPES: NodeType> QuorumCertificate<TYPES> {
    #[must_use]
    /// Creat the Genesis certificate
    pub async fn genesis<V: Versions>(
        validated_state: &TYPES::ValidatedState,
        instance_state: &TYPES::InstanceState,
    ) -> Self {
        // since this is genesis, we should never have a decided upgrade certificate.
        let upgrade_lock = UpgradeLock::<TYPES, V>::new();

        let genesis_view = <TYPES::View as ConsensusTime>::genesis();

        let data = QuorumData {
            leaf_commit: Leaf::genesis(validated_state, instance_state)
                .await
                .commit(&upgrade_lock)
                .await,
        };

        let versioned_data =
            VersionedVoteData::<_, _, V>::new_infallible(data.clone(), genesis_view, &upgrade_lock)
                .await;

        let bytes: [u8; 32] = versioned_data.commit().into();

        Self::new(
            data,
            Commitment::from_raw(bytes),
            genesis_view,
            None,
            PhantomData,
        )
    }
}

impl<TYPES: NodeType> QuorumCertificate2<TYPES> {
    #[must_use]
    /// Create the Genesis certificate
    pub async fn genesis<V: Versions>(
        validated_state: &TYPES::ValidatedState,
        instance_state: &TYPES::InstanceState,
    ) -> Self {
        // since this is genesis, we should never have a decided upgrade certificate.
        let upgrade_lock = UpgradeLock::<TYPES, V>::new();

        let genesis_view = <TYPES::View as ConsensusTime>::genesis();

        let data = QuorumData2 {
            leaf_commit: Leaf2::genesis::<V>(validated_state, instance_state)
                .await
                .commit(),
            epoch: genesis_epoch_from_version::<V, TYPES>(), // #3967 make sure this is enough of a gate for epochs
        };

        let versioned_data =
            VersionedVoteData::<_, _, V>::new_infallible(data.clone(), genesis_view, &upgrade_lock)
                .await;

        let bytes: [u8; 32] = versioned_data.commit().into();

        Self::new(
            data,
            Commitment::from_raw(bytes),
            genesis_view,
            None,
            PhantomData,
        )
    }
}

impl<TYPES: NodeType> Leaf<TYPES> {
    /// Create a new leaf from its components.
    ///
    /// # Panics
    ///
    /// Panics if the genesis payload (`TYPES::BlockPayload::genesis()`) is malformed (unable to be
    /// interpreted as bytes).
    #[must_use]
    pub async fn genesis(
        validated_state: &TYPES::ValidatedState,
        instance_state: &TYPES::InstanceState,
    ) -> Self {
        let (payload, metadata) =
            TYPES::BlockPayload::from_transactions([], validated_state, instance_state)
                .await
                .unwrap();
        let builder_commitment = payload.builder_commitment(&metadata);
        let payload_bytes = payload.encode();

        let payload_commitment = vid_commitment(&payload_bytes, GENESIS_VID_NUM_STORAGE_NODES);

        let block_header = TYPES::BlockHeader::genesis(
            instance_state,
            payload_commitment,
            builder_commitment,
            metadata,
        );

        let null_quorum_data = QuorumData {
            leaf_commit: Commitment::<Leaf<TYPES>>::default_commitment_no_preimage(),
        };

        let justify_qc = QuorumCertificate::new(
            null_quorum_data.clone(),
            null_quorum_data.commit(),
            <TYPES::View as ConsensusTime>::genesis(),
            None,
            PhantomData,
        );

        Self {
            view_number: TYPES::View::genesis(),
            justify_qc,
            parent_commitment: null_quorum_data.leaf_commit,
            upgrade_certificate: None,
            block_header: block_header.clone(),
            block_payload: Some(payload),
        }
    }

    /// Time when this leaf was created.
    pub fn view_number(&self) -> TYPES::View {
        self.view_number
    }
    /// Height of this leaf in the chain.
    ///
    /// Equivalently, this is the number of leaves before this one in the chain.
    pub fn height(&self) -> u64 {
        self.block_header.block_number()
    }
    /// The QC linking this leaf to its parent in the chain.
    pub fn justify_qc(&self) -> QuorumCertificate<TYPES> {
        self.justify_qc.clone()
    }
    /// The QC linking this leaf to its parent in the chain.
    pub fn upgrade_certificate(&self) -> Option<UpgradeCertificate<TYPES>> {
        self.upgrade_certificate.clone()
    }
    /// Commitment to this leaf's parent.
    pub fn parent_commitment(&self) -> Commitment<Self> {
        self.parent_commitment
    }
    /// The block header contained in this leaf.
    pub fn block_header(&self) -> &<TYPES as NodeType>::BlockHeader {
        &self.block_header
    }

    /// Get a mutable reference to the block header contained in this leaf.
    pub fn block_header_mut(&mut self) -> &mut <TYPES as NodeType>::BlockHeader {
        &mut self.block_header
    }
    /// Fill this leaf with the block payload.
    ///
    /// # Errors
    ///
    /// Fails if the payload commitment doesn't match `self.block_header.payload_commitment()`
    /// or if the transactions are of invalid length
    pub fn fill_block_payload(
        &mut self,
        block_payload: TYPES::BlockPayload,
        num_storage_nodes: usize,
    ) -> std::result::Result<(), BlockError> {
        let encoded_txns = block_payload.encode();
        let commitment = vid_commitment(&encoded_txns, num_storage_nodes);
        if commitment != self.block_header.payload_commitment() {
            return Err(BlockError::InconsistentPayloadCommitment);
        }
        self.block_payload = Some(block_payload);
        Ok(())
    }

    /// Take the block payload from the leaf and return it if it is present
    pub fn unfill_block_payload(&mut self) -> Option<TYPES::BlockPayload> {
        self.block_payload.take()
    }

    /// Fill this leaf with the block payload, without checking
    /// header and payload consistency
    pub fn fill_block_payload_unchecked(&mut self, block_payload: TYPES::BlockPayload) {
        self.block_payload = Some(block_payload);
    }

    /// Optional block payload.
    pub fn block_payload(&self) -> Option<TYPES::BlockPayload> {
        self.block_payload.clone()
    }

    /// A commitment to the block payload contained in this leaf.
    pub fn payload_commitment(&self) -> VidCommitment {
        self.block_header().payload_commitment()
    }

    /// Validate that a leaf has the right upgrade certificate to be the immediate child of another leaf
    ///
    /// This may not be a complete function. Please double-check that it performs the checks you expect before substituting validation logic with it.
    ///
    /// # Errors
    /// Returns an error if the certificates are not identical, or that when we no longer see a
    /// cert, it's for the right reason.
    pub async fn extends_upgrade(
        &self,
        parent: &Self,
        decided_upgrade_certificate: &Arc<RwLock<Option<UpgradeCertificate<TYPES>>>>,
    ) -> Result<()> {
        match (self.upgrade_certificate(), parent.upgrade_certificate()) {
            // Easiest cases are:
            //   - no upgrade certificate on either: this is the most common case, and is always fine.
            //   - if the parent didn't have a certificate, but we see one now, it just means that we have begun an upgrade: again, this is always fine.
            (None | Some(_), None) => {}
            // If we no longer see a cert, we have to make sure that we either:
            //    - no longer care because we have passed new_version_first_view, or
            //    - no longer care because we have passed `decide_by` without deciding the certificate.
            (None, Some(parent_cert)) => {
                let decided_upgrade_certificate_read = decided_upgrade_certificate.read().await;
                ensure!(self.view_number() > parent_cert.data.new_version_first_view
                    || (self.view_number() > parent_cert.data.decide_by && decided_upgrade_certificate_read.is_none()),
                       "The new leaf is missing an upgrade certificate that was present in its parent, and should still be live."
                );
            }
            // If we both have a certificate, they should be identical.
            // Technically, this prevents us from initiating a new upgrade in the view immediately following an upgrade.
            // I think this is a fairly lax restriction.
            (Some(cert), Some(parent_cert)) => {
                ensure!(cert == parent_cert, "The new leaf does not extend the parent leaf, because it has attached a different upgrade certificate.");
            }
        }

        // This check should be added once we sort out the genesis leaf/justify_qc issue.
        // ensure!(self.parent_commitment() == parent_leaf.commit(), "The commitment of the parent leaf does not match the specified parent commitment.");

        Ok(())
    }
}

impl<TYPES: NodeType> TestableLeaf for Leaf<TYPES>
where
    TYPES::ValidatedState: TestableState<TYPES>,
    TYPES::BlockPayload: TestableBlock<TYPES>,
{
    type NodeType = TYPES;

    fn create_random_transaction(
        &self,
        rng: &mut dyn rand::RngCore,
        padding: u64,
    ) -> <<Self::NodeType as NodeType>::BlockPayload as BlockPayload<Self::NodeType>>::Transaction
    {
        TYPES::ValidatedState::create_random_transaction(None, rng, padding)
    }
}
impl<TYPES: NodeType> TestableLeaf for Leaf2<TYPES>
where
    TYPES::ValidatedState: TestableState<TYPES>,
    TYPES::BlockPayload: TestableBlock<TYPES>,
{
    type NodeType = TYPES;

    fn create_random_transaction(
        &self,
        rng: &mut dyn rand::RngCore,
        padding: u64,
    ) -> <<Self::NodeType as NodeType>::BlockPayload as BlockPayload<Self::NodeType>>::Transaction
    {
        TYPES::ValidatedState::create_random_transaction(None, rng, padding)
    }
}
/// Fake the thing a genesis block points to. Needed to avoid infinite recursion
#[must_use]
pub fn fake_commitment<S: Committable>() -> Commitment<S> {
    RawCommitmentBuilder::new("Dummy commitment for arbitrary genesis").finalize()
}

/// create a random commitment
#[must_use]
pub fn random_commitment<S: Committable>(rng: &mut dyn rand::RngCore) -> Commitment<S> {
    let random_array: Vec<u8> = (0u8..100u8).map(|_| rng.gen_range(0..255)).collect();
    RawCommitmentBuilder::new("Random Commitment")
        .constant_str("Random Field")
        .var_size_bytes(&random_array)
        .finalize()
}

/// Serialization for the QC assembled signature
/// # Panics
/// if serialization fails
pub fn serialize_signature2<TYPES: NodeType>(
    signatures: &<TYPES::SignatureKey as SignatureKey>::QcType,
) -> Vec<u8> {
    let mut signatures_bytes = vec![];
    signatures_bytes.extend("Yes".as_bytes());

    let (sig, proof) = TYPES::SignatureKey::sig_proof(signatures);
    let proof_bytes = bincode_opts()
        .serialize(&proof.as_bitslice())
        .expect("This serialization shouldn't be able to fail");
    signatures_bytes.extend("bitvec proof".as_bytes());
    signatures_bytes.extend(proof_bytes.as_slice());
    let sig_bytes = bincode_opts()
        .serialize(&sig)
        .expect("This serialization shouldn't be able to fail");
    signatures_bytes.extend("aggregated signature".as_bytes());
    signatures_bytes.extend(sig_bytes.as_slice());
    signatures_bytes
}

impl<TYPES: NodeType> Committable for Leaf<TYPES> {
    fn commit(&self) -> committable::Commitment<Self> {
        RawCommitmentBuilder::new("leaf commitment")
            .u64_field("view number", *self.view_number)
            .field("parent leaf commitment", self.parent_commitment)
            .field("block header", self.block_header.commit())
            .field("justify qc", self.justify_qc.commit())
            .optional("upgrade certificate", &self.upgrade_certificate)
            .finalize()
    }
}

impl<TYPES: NodeType> Leaf2<TYPES> {
    /// Constructs a leaf from a given quorum proposal.
    pub fn from_quorum_proposal(quorum_proposal: &QuorumProposalWrapper<TYPES>) -> Self {
        // WARNING: Do NOT change this to a wildcard match, or reference the fields directly in the construction of the leaf.
        // The point of this match is that we will get a compile-time error if we add a field without updating this.
        let QuorumProposalWrapper {
            proposal:
                QuorumProposal2 {
                    view_number,
                    justify_qc,
                    next_epoch_justify_qc,
                    block_header,
                    upgrade_certificate,
                    view_change_evidence,
                    next_drb_result,
                },
            with_epoch,
        } = quorum_proposal;

        Self {
            view_number: *view_number,
            justify_qc: justify_qc.clone(),
            next_epoch_justify_qc: next_epoch_justify_qc.clone(),
            parent_commitment: justify_qc.data().leaf_commit,
            block_header: block_header.clone(),
            upgrade_certificate: upgrade_certificate.clone(),
            block_payload: None,
            view_change_evidence: view_change_evidence.clone(),
            next_drb_result: *next_drb_result,
            with_epoch: *with_epoch,
        }
    }
}

impl<TYPES: NodeType> Leaf<TYPES> {
    /// Constructs a leaf from a given quorum proposal.
    pub fn from_quorum_proposal(quorum_proposal: &QuorumProposal<TYPES>) -> Self {
        // WARNING: Do NOT change this to a wildcard match, or reference the fields directly in the construction of the leaf.
        // The point of this match is that we will get a compile-time error if we add a field without updating this.
        let QuorumProposal {
            view_number,
            justify_qc,
            block_header,
            upgrade_certificate,
            proposal_certificate: _,
        } = quorum_proposal;

        Self {
            view_number: *view_number,
            justify_qc: justify_qc.clone(),
            parent_commitment: justify_qc.data().leaf_commit,
            block_header: block_header.clone(),
            upgrade_certificate: upgrade_certificate.clone(),
            block_payload: None,
        }
    }
}

pub mod null_block {
    #![allow(missing_docs)]

    use jf_vid::VidScheme;
    use memoize::memoize;
    use vbs::version::StaticVersionType;

    use crate::{
        traits::{
            block_contents::BuilderFee,
            node_implementation::{NodeType, Versions},
            signature_key::BuilderSignatureKey,
            BlockPayload,
        },
        vid::{vid_scheme, VidCommitment},
    };

    /// The commitment for a null block payload.
    ///
    /// Note: the commitment depends on the network (via `num_storage_nodes`),
    /// and may change (albeit rarely) during execution.
    ///
    /// We memoize the result to avoid having to recalculate it.
    #[memoize(SharedCache, Capacity: 10)]
    #[must_use]
    pub fn commitment(num_storage_nodes: usize) -> Option<VidCommitment> {
        let vid_result = vid_scheme(num_storage_nodes).commit_only(Vec::new());

        match vid_result {
            Ok(r) => Some(r),
            Err(_) => None,
        }
    }

    /// Builder fee data for a null block payload
    #[must_use]
    pub fn builder_fee<TYPES: NodeType, V: Versions>(
        num_storage_nodes: usize,
        version: vbs::version::Version,
        view_number: u64,
    ) -> Option<BuilderFee<TYPES>> {
        /// Arbitrary fee amount, this block doesn't actually come from a builder
        const FEE_AMOUNT: u64 = 0;

        let (pub_key, priv_key) =
            <TYPES::BuilderSignatureKey as BuilderSignatureKey>::generated_from_seed_indexed(
                [0_u8; 32], 0,
            );

        if version >= V::Marketplace::VERSION {
            match TYPES::BuilderSignatureKey::sign_sequencing_fee_marketplace(
                &priv_key,
                FEE_AMOUNT,
                view_number,
            ) {
                Ok(sig) => Some(BuilderFee {
                    fee_amount: FEE_AMOUNT,
                    fee_account: pub_key,
                    fee_signature: sig,
                }),
                Err(_) => None,
            }
        } else {
            let (_null_block, null_block_metadata) =
                <TYPES::BlockPayload as BlockPayload<TYPES>>::empty();

            match TYPES::BuilderSignatureKey::sign_fee(
                &priv_key,
                FEE_AMOUNT,
                &null_block_metadata,
                &commitment(num_storage_nodes)?,
            ) {
                Ok(sig) => Some(BuilderFee {
                    fee_amount: FEE_AMOUNT,
                    fee_account: pub_key,
                    fee_signature: sig,
                }),
                Err(_) => None,
            }
        }
    }
}

/// A packed bundle constructed from a sequence of bundles.
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct PackedBundle<TYPES: NodeType> {
    /// The combined transactions as bytes.
    pub encoded_transactions: Arc<[u8]>,

    /// The metadata of the block.
    pub metadata: <TYPES::BlockPayload as BlockPayload<TYPES>>::Metadata,

    /// The view number that this block is associated with.
    pub view_number: TYPES::View,

    /// The view number that this block is associated with.
    pub epoch_number: Option<TYPES::Epoch>,

    /// The sequencing fee for submitting bundles.
    pub sequencing_fees: Vec1<BuilderFee<TYPES>>,

    /// The Vid precompute for the block.
    pub vid_precompute: Option<VidPrecomputeData>,

    /// The auction results for the block, if it was produced as the result of an auction
    pub auction_result: Option<TYPES::AuctionResult>,
}

impl<TYPES: NodeType> PackedBundle<TYPES> {
    /// Create a new [`PackedBundle`].
    pub fn new(
        encoded_transactions: Arc<[u8]>,
        metadata: <TYPES::BlockPayload as BlockPayload<TYPES>>::Metadata,
        view_number: TYPES::View,
        epoch_number: Option<TYPES::Epoch>,
        sequencing_fees: Vec1<BuilderFee<TYPES>>,
        vid_precompute: Option<VidPrecomputeData>,
        auction_result: Option<TYPES::AuctionResult>,
    ) -> Self {
        Self {
            encoded_transactions,
            metadata,
            view_number,
            epoch_number,
            sequencing_fees,
            vid_precompute,
            auction_result,
        }
    }
}
