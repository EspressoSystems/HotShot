//! This module holds the dependency task for the QuorumProposalTask. It is spawned whenever an event that could
//! initiate a proposal occurs.

use std::{
    collections::{BTreeMap, HashMap},
    marker::PhantomData,
    sync::Arc,
    time::Duration,
};

use anyhow::{ensure, Context, Result};
use async_broadcast::Sender;
use async_compatibility_layer::art::async_sleep;
use committable::{Commitment, Committable};
use hotshot_task::dependency_task::HandleDepOutput;
use hotshot_types::{
    consensus::CommitmentAndMetadata,
    data::{Leaf, QuorumProposal, VidDisperseShare, ViewChangeEvidence},
    message::Proposal,
    simple_certificate::QuorumCertificate,
    traits::{
        block_contents::BlockHeader, node_implementation::NodeType, signature_key::SignatureKey,
    },
    utils::View,
    vote::HasViewNumber,
};
use tracing::{debug, error};

use crate::{events::HotShotEvent, helpers::broadcast_event};

use super::helpers::get_parent_leaf_and_state;

/// Proposal dependency types. These types represent events that precipitate a proposal.
#[derive(PartialEq, Debug)]
pub(crate) enum ProposalDependency {
    /// For the `SendPayloadCommitmentAndMetadata` event.
    PayloadAndMetadata,

    /// For the `QCFormed` event.
    QC,

    /// For the `ViewSyncFinalizeCertificate2Recv` event.
    ViewSyncCert,

    /// For the `QCFormed` event timeout branch.
    TimeoutCert,

    /// For the `QuroumProposalValidated` event after validating `QuorumProposalRecv`.
    Proposal,

    /// For the `ProposeNow` event.
    ProposeNow,

    /// For the `VIDShareValidated` event.
    VIDShare,

    /// For the `ValidatedStateUpdated` event.
    ValidatedState,
}

/// Handler for the proposal dependency
pub(crate) struct ProposalDependencyHandle<TYPES: NodeType> {
    /// Latest view number that has been proposed for (proxy for cur_view).
    pub latest_proposed_view: TYPES::Time,

    /// The view number to propose for.
    pub view_number: TYPES::Time,

    /// The event sender.
    pub sender: Sender<Arc<HotShotEvent<TYPES>>>,

    /// Immutable instance state
    pub instance_state: Arc<TYPES::InstanceState>,

    /// Membership for Quorum Certs/votes
    pub quorum_membership: Arc<TYPES::Membership>,

    /// Our public key
    pub public_key: TYPES::SignatureKey,

    /// Our Private Key
    pub private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,

    /// Round start delay from config, in milliseconds.
    pub round_start_delay: u64,

    /// The map of undecided leaves. This mapping is from hash -> leaf.
    /// This contains *only* undecided leaves.
    pub undecided_leaves: HashMap<Commitment<Leaf<TYPES>>, Leaf<TYPES>>,

    /// The most recently decided view.
    pub last_decided_view: TYPES::Time,

    /// The last high qc that we've seen in this node.
    pub high_qc: QuorumCertificate<TYPES>,

    /// The validated states (i.e. states that the blocks modify) that we have received so far.
    pub validated_states: BTreeMap<TYPES::Time, View<TYPES>>,
}

impl<TYPES: NodeType> ProposalDependencyHandle<TYPES> {
    /// Publishes a proposal given the [`CommitmentAndMetadata`], [`VidDisperseShare`]
    /// and high qc [`QuorumCertificate`], with optional [`ViewChangeEvidence`].
    async fn publish_proposal(
        &self,
        commitment_and_metadata: CommitmentAndMetadata<TYPES>,
        vid_share: Proposal<TYPES, VidDisperseShare<TYPES>>,
        high_qc: QuorumCertificate<TYPES>,
        view_change_evidence: Option<ViewChangeEvidence<TYPES>>,
    ) -> Result<()> {
        let (parent_leaf, state) =
            get_parent_leaf_and_state(self.latest_proposed_view, self.view_number, self).await?;

        let proposal_certificate = view_change_evidence
            .as_ref()
            .filter(|cert| cert.is_valid_for_view(&self.view_number))
            .cloned();

        ensure!(
            commitment_and_metadata.block_view == self.view_number,
            "Cannot propose because our VID payload commitment and metadata is for an older view."
        );

        let block_header = TYPES::BlockHeader::new(
            state.as_ref(),
            self.instance_state.as_ref(),
            &parent_leaf,
            commitment_and_metadata.commitment,
            commitment_and_metadata.builder_commitment,
            commitment_and_metadata.metadata,
            commitment_and_metadata.fee,
            vid_share.data.common.clone(),
        )
        .await
        .context("Failed to construct block header")?;

        let proposal = QuorumProposal {
            block_header,
            view_number: self.view_number,
            justify_qc: high_qc,
            proposal_certificate,
            upgrade_certificate: None,
        };

        let proposed_leaf = Leaf::from_quorum_proposal(&proposal);
        ensure!(
            proposed_leaf.get_parent_commitment() == parent_leaf.commit(),
            "Proposed leaf parent does not equal high qc"
        );

        let signature =
            TYPES::SignatureKey::sign(&self.private_key, proposed_leaf.commit().as_ref())
                .context("Failed to compute proposed_leaf.commit()")?;

        let message = Proposal {
            data: proposal,
            signature,
            _pd: PhantomData,
        };
        debug!(
            "Sending proposal for view {:?}",
            proposed_leaf.get_view_number(),
        );
        async_sleep(Duration::from_millis(self.round_start_delay)).await;
        broadcast_event(
            Arc::new(HotShotEvent::QuorumProposalSend(
                message.clone(),
                self.public_key.clone(),
            )),
            &self.sender,
        )
        .await;

        Ok(())
    }
}
impl<TYPES: NodeType> HandleDepOutput for ProposalDependencyHandle<TYPES> {
    type Output = Vec<Vec<Vec<Arc<HotShotEvent<TYPES>>>>>;

    #[allow(clippy::no_effect_underscore_binding)]
    async fn handle_dep_result(self, res: Self::Output) {
        let mut payload_commitment = None;
        let mut commit_and_metadata: Option<CommitmentAndMetadata<TYPES>> = None;
        let mut quorum_certificate = None;
        let mut timeout_certificate = None;
        let mut view_sync_finalize_cert = None;
        let mut vid_share = None;
        for event in res.iter().flatten().flatten() {
            match event.as_ref() {
                HotShotEvent::QuorumProposalValidated(proposal, _) => {
                    let proposal_payload_comm = proposal.block_header.payload_commitment();
                    if let Some(comm) = payload_commitment {
                        if proposal_payload_comm != comm {
                            return;
                        }
                    } else {
                        payload_commitment = Some(proposal_payload_comm);
                    }
                }
                HotShotEvent::SendPayloadCommitmentAndMetadata(
                    payload_commitment,
                    builder_commitment,
                    metadata,
                    view,
                    fee,
                ) => {
                    commit_and_metadata = Some(CommitmentAndMetadata {
                        commitment: *payload_commitment,
                        builder_commitment: builder_commitment.clone(),
                        metadata: metadata.clone(),
                        fee: fee.clone(),
                        block_view: *view,
                    });
                }
                HotShotEvent::QCFormed(cert) => match cert {
                    either::Right(timeout) => {
                        timeout_certificate = Some(timeout.clone());
                    }
                    either::Left(qc) => {
                        quorum_certificate = Some(qc.clone());
                    }
                },
                HotShotEvent::ViewSyncFinalizeCertificate2Recv(cert) => {
                    view_sync_finalize_cert = Some(cert.clone());
                }
                HotShotEvent::ProposeNow(_, pdd) => {
                    commit_and_metadata = Some(pdd.commitment_and_metadata.clone());
                    match &pdd.secondary_proposal_information {
                        hotshot_types::consensus::SecondaryProposalInformation::QuorumProposalAndCertificate(quorum_proposal, _) => {
                            payload_commitment = Some(quorum_proposal.block_header.payload_commitment());
                        },
                        hotshot_types::consensus::SecondaryProposalInformation::Timeout(tc) => {
                            timeout_certificate = Some(tc.clone());
                        }
                        hotshot_types::consensus::SecondaryProposalInformation::ViewSync(vsc) => {
                            view_sync_finalize_cert = Some(vsc.clone());
                        },
                    }
                }
                HotShotEvent::VIDShareValidated(share) => {
                    vid_share = Some(share.clone());
                }
                _ => {}
            }
        }

        if commit_and_metadata.is_none() {
            error!(
                "Somehow completed the proposal dependency task without a commitment and metadata"
            );
            return;
        }

        if vid_share.is_none() {
            error!("Somehow completed the proposal dependency task without a VID share");
            return;
        }

        // Take the high qc from the quorum certificate, or memory, whatever is newest.
        let high_qc = match quorum_certificate {
            Some(qc) => {
                if self.high_qc.get_view_number() > qc.get_view_number() {
                    self.high_qc.clone()
                } else {
                    qc
                }
            }
            None => self.high_qc.clone(),
        };

        let proposal_cert = if let Some(view_sync_cert) = view_sync_finalize_cert {
            Some(ViewChangeEvidence::ViewSync(view_sync_cert))
        } else {
            timeout_certificate.map(ViewChangeEvidence::Timeout)
        };

        if let Err(e) = self
            .publish_proposal(
                commit_and_metadata.unwrap(),
                vid_share.unwrap(),
                high_qc,
                proposal_cert,
            )
            .await
        {
            debug!("Failed to publish proposal; error = {e}");
        }
    }
}
