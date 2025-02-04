// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

//! Abstract storage type for storing DA proposals and VID shares
//!
//! This modules provides the [`Storage`] trait.
//!

use std::collections::BTreeMap;

use anyhow::Result;
use async_trait::async_trait;
use committable::Commitment;
use jf_vid::VidScheme;

use super::node_implementation::NodeType;
use crate::{
    consensus::{CommitmentMap, View},
    data::{
        vid_disperse::{ADVZDisperseShare, VidDisperseShare2},
        DaProposal, DaProposal2, Leaf, Leaf2, QuorumProposal, QuorumProposal2,
        QuorumProposalWrapper,
    },
    event::HotShotAction,
    message::{convert_proposal, Proposal},
    simple_certificate::{
        NextEpochQuorumCertificate2, QuorumCertificate, QuorumCertificate2, UpgradeCertificate,
    },
    vid::VidSchemeType,
};

/// Abstraction for storing a variety of consensus payload datum.
#[async_trait]
pub trait Storage<TYPES: NodeType>: Send + Sync + Clone {
    /// Add a proposal to the stored VID proposals.
    async fn append_vid(&self, proposal: &Proposal<TYPES, ADVZDisperseShare<TYPES>>) -> Result<()>;
    /// Add a proposal to the stored VID proposals.
    /// TODO(Chengyu): change here because in the future disperse share types might not be convertible.
    async fn append_vid2(
        &self,
        proposal: &Proposal<TYPES, VidDisperseShare2<TYPES>>,
    ) -> Result<()> {
        self.append_vid(&convert_proposal(proposal.clone())).await
    }
    /// Add a proposal to the stored DA proposals.
    async fn append_da(
        &self,
        proposal: &Proposal<TYPES, DaProposal<TYPES>>,
        vid_commit: <VidSchemeType as VidScheme>::Commit,
    ) -> Result<()>;
    /// Add a proposal to the stored DA proposals.
    async fn append_da2(
        &self,
        proposal: &Proposal<TYPES, DaProposal2<TYPES>>,
        vid_commit: <VidSchemeType as VidScheme>::Commit,
    ) -> Result<()> {
        self.append_da(&convert_proposal(proposal.clone()), vid_commit)
            .await
    }
    /// Add a proposal we sent to the store
    async fn append_proposal(
        &self,
        proposal: &Proposal<TYPES, QuorumProposal<TYPES>>,
    ) -> Result<()>;
    /// Add a proposal we sent to the store
    async fn append_proposal2(
        &self,
        proposal: &Proposal<TYPES, QuorumProposal2<TYPES>>,
    ) -> Result<()> {
        self.append_proposal(&convert_proposal(proposal.clone()))
            .await
    }
    /// Add a proposal we sent to the store
    async fn append_proposal_wrapper(
        &self,
        proposal: &Proposal<TYPES, QuorumProposalWrapper<TYPES>>,
    ) -> Result<()> {
        self.append_proposal(&convert_proposal(proposal.clone()))
            .await
    }
    /// Record a HotShotAction taken.
    async fn record_action(
        &self,
        view: TYPES::View,
        epoch: Option<TYPES::Epoch>,
        action: HotShotAction,
    ) -> Result<()>;
    /// Update the current high QC in storage.
    async fn update_high_qc(&self, high_qc: QuorumCertificate<TYPES>) -> Result<()>;
    /// Update the current high QC in storage.
    async fn update_high_qc2(&self, high_qc: QuorumCertificate2<TYPES>) -> Result<()> {
        self.update_high_qc(high_qc.to_qc()).await
    }
    /// Update the current high QC in storage.
    async fn update_next_epoch_high_qc2(
        &self,
        _next_epoch_high_qc: NextEpochQuorumCertificate2<TYPES>,
    ) -> Result<()> {
        Ok(())
    }
    /// Update the currently undecided state of consensus.  This includes the undecided leaf chain,
    /// and the undecided state.
    async fn update_undecided_state(
        &self,
        leaves: CommitmentMap<Leaf<TYPES>>,
        state: BTreeMap<TYPES::View, View<TYPES>>,
    ) -> Result<()>;
    /// Update the currently undecided state of consensus.  This includes the undecided leaf chain,
    /// and the undecided state.
    async fn update_undecided_state2(
        &self,
        leaves: CommitmentMap<Leaf2<TYPES>>,
        state: BTreeMap<TYPES::View, View<TYPES>>,
    ) -> Result<()> {
        self.update_undecided_state(
            leaves
                .iter()
                .map(|(&commitment, leaf)| {
                    (
                        Commitment::from_raw(commitment.into()),
                        leaf.clone().to_leaf_unsafe(),
                    )
                })
                .collect(),
            state,
        )
        .await
    }
    /// Upgrade the current decided upgrade certificate in storage.
    async fn update_decided_upgrade_certificate(
        &self,
        decided_upgrade_certificate: Option<UpgradeCertificate<TYPES>>,
    ) -> Result<()>;
    /// Migrate leaves from `Leaf` to `Leaf2`, and proposals from `QuorumProposal` to `QuorumProposal2`
    async fn migrate_consensus(
        &self,
        _convert_leaf: fn(Leaf<TYPES>) -> Leaf2<TYPES>,
        _convert_proposal: fn(
            Proposal<TYPES, QuorumProposal<TYPES>>,
        ) -> Proposal<TYPES, QuorumProposal2<TYPES>>,
    ) -> Result<()> {
        Ok(())
    }
}
