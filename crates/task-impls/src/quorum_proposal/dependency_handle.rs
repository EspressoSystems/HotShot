use std::sync::Arc;

use async_broadcast::Sender;
use async_lock::RwLock;
use hotshot_task::dependency_task::HandleDepOutput;
use hotshot_types::{
    consensus::{CommitmentAndMetadata, Consensus},
    event::Event,
    hotshot_event::HotShotEvent,
    traits::{
        block_contents::BlockHeader, node_implementation::NodeType, signature_key::SignatureKey,
    },
};
use tracing::error;

use crate::consensus::helpers::publish_proposal_if_able;

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
}

/// Handler for the proposal dependency
pub(crate) struct ProposalDependencyHandle<TYPES: NodeType> {
    /// Latest view number that has been proposed for.
    pub latest_proposed_view: TYPES::Time,

    /// The view number to propose for.
    pub view_number: TYPES::Time,

    /// The event sender.
    pub sender: Sender<Arc<HotShotEvent<TYPES>>>,

    /// Reference to consensus. The replica will require a write lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES>>>,

    /// Immutable instance state
    pub instance_state: Arc<TYPES::InstanceState>,

    /// Output events to application
    pub output_event_stream: async_broadcast::Sender<Event<TYPES>>,

    /// Membership for Timeout votes/certs
    pub timeout_membership: Arc<TYPES::Membership>,

    /// Membership for Quorum Certs/votes
    pub quorum_membership: Arc<TYPES::Membership>,

    /// Our public key
    pub public_key: TYPES::SignatureKey,

    /// Our Private Key
    pub private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,

    /// View timeout from config.
    pub timeout: u64,

    /// Round start delay from config, in milliseconds.
    pub round_start_delay: u64,

    /// The node's id
    pub id: u64,
}

impl<TYPES: NodeType> ProposalDependencyHandle<TYPES> {
    async fn publish_proposal(&self) -> Result<()> {
        Ok(())
    }
}
impl<TYPES: NodeType> HandleDepOutput for ProposalDependencyHandle<TYPES> {
    type Output = Vec<Vec<Vec<Arc<HotShotEvent<TYPES>>>>>;

    #[allow(clippy::no_effect_underscore_binding)]
    async fn handle_dep_result(self, res: Self::Output) {
        let mut payload_commitment = None;
        let mut commit_and_metadata: Option<CommitmentAndMetadata<TYPES>> = None;
        let mut _quorum_certificate = None;
        let mut _timeout_certificate = None;
        let mut _view_sync_finalize_cert = None;
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
                        _timeout_certificate = Some(timeout.clone());
                    }
                    either::Left(qc) => {
                        _quorum_certificate = Some(qc.clone());
                    }
                },
                HotShotEvent::ViewSyncFinalizeCertificate2Recv(cert) => {
                    _view_sync_finalize_cert = Some(cert.clone());
                }
                HotShotEvent::ProposeNow(_, pdd) => {
                    commit_and_metadata = Some(pdd.commitment_and_metadata.clone());
                    match &pdd.secondary_proposal_information {
                        hotshot_types::consensus::SecondaryProposalInformation::QuorumProposalAndCertificate(quorum_proposal, quorum_certificate) => {
                            _quorum_certificate = Some(quorum_certificate.clone());
                            payload_commitment = Some(quorum_proposal.block_header.payload_commitment());
                        },
                        hotshot_types::consensus::SecondaryProposalInformation::Timeout(tc) => {
                            _timeout_certificate = Some(tc.clone());
                        }
                        hotshot_types::consensus::SecondaryProposalInformation::ViewSync(vsc) => {
                            _view_sync_finalize_cert = Some(vsc.clone());
                        },
                    }
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
    }
}
